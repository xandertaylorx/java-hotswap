use crate::{
    process::{execute, Emit},
    Config,
};
use regex::Regex;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::AtomicBool,
};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Module {
    pub name: String,
    pub sources: Vec<PathBuf>,
    pub classes: Vec<PathBuf>,
    pub classpath: Vec<PathBuf>,
    #[serde(default)]
    pub artifacts: Vec<PathBuf>,
    #[serde(default)]
    pub compile_tasks: Vec<String>,
    #[serde(default)]
    pub main_class: Option<String>,
    #[serde(default)]
    pub jvm_args: Vec<String>,
    #[serde(default)]
    pub java: Option<String>,
    #[serde(default)]
    pub run_tasks: Vec<RunTask>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunTask {
    pub name: String,
    pub main_class: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Launch {
    pub task: String,
    pub main_class: String,
    pub java: String,
    pub classpath: Vec<PathBuf>,
    pub jvm_args: Vec<String>,
    pub args: Vec<String>,
    pub working_directory: PathBuf,
    pub environment: BTreeMap<String, String>,
}
#[derive(Debug)]
pub struct Project {
    pub root: PathBuf,
    pub kind: String,
    pub modules: Vec<Module>,
    pub tool: PathBuf,
    pub gradle_runtime: Option<crate::gradle_runtime::Runtime>,
}
pub fn jdk_tool(home: Option<&Path>, name: &str) -> PathBuf {
    let home = home
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("JAVA_HOME").map(PathBuf::from));
    let name = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.into()
    };
    home.map(|h| h.join("bin").join(&name))
        .unwrap_or_else(|| name.into())
}
fn build_tool(root: &Path, name: &str) -> PathBuf {
    let wrapper = root.join(match (name, cfg!(windows)) {
        ("gradle", false) => "gradlew",
        ("gradle", true) => "gradlew.bat",
        (_, false) => "mvnw",
        (_, true) => "mvnw.cmd",
    });
    if wrapper.exists() {
        wrapper
    } else {
        std::env::var_os(if name == "gradle" {
            "JAVA_HOTSWAP_GRADLE"
        } else {
            "JAVA_HOTSWAP_MAVEN"
        })
        .map(PathBuf::from)
        .unwrap_or_else(|| name.into())
    }
}

fn build_kind(root: &Path) -> Result<Option<&'static str>, String> {
    let gradle = [
        "build.gradle",
        "build.gradle.kts",
        "settings.gradle",
        "settings.gradle.kts",
        "gradlew",
        "gradlew.bat",
    ]
    .iter()
    .any(|name| root.join(name).is_file());
    let maven = ["pom.xml", "mvnw", "mvnw.cmd"]
        .iter()
        .any(|name| root.join(name).is_file());
    match (gradle, maven) {
        (true, true) => Err(format!(
            "Both Gradle and Maven markers found in {}; choose an unambiguous build root",
            root.display()
        )),
        (true, false) => Ok(Some("gradle")),
        (false, true) => Ok(Some("maven")),
        _ => Ok(None),
    }
}

// A recognized build owns its module tree. Do not classify Gradle subprojects or
// Maven reactor modules as independent builds, or scan their generated outputs.
fn find_builds(root: &Path, builds: &mut Vec<(PathBuf, &'static str)>) -> Result<(), String> {
    if let Some(kind) = build_kind(root)? {
        builds.push((root.to_path_buf(), kind));
        return Ok(());
    }
    for entry in fs::read_dir(root)
        .map_err(|e| format!("Cannot discover projects in {}: {e}", root.display()))?
    {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            continue; // Includes symlinks; avoid cycles and leaving the workspace.
        }
        if matches!(
            entry.file_name().to_str(),
            Some(
                ".git"
                    | ".gradle"
                    | ".java-hotswap"
                    | "target"
                    | "build"
                    | "build.nosync"
                    | "node_modules"
                    | ".tools"
                    | ".idea"
                    | ".venv"
            )
        ) {
            continue;
        }
        find_builds(&entry.path(), builds)?;
    }
    Ok(())
}

fn select_build(
    workspace: &Path,
    selected: Option<&Path>,
) -> Result<(PathBuf, &'static str), String> {
    if let Some(selected) = selected {
        let root = fs::canonicalize(workspace.join(selected))
            .map_err(|e| format!("Invalid projectRoot {}: {e}", selected.display()))?;
        let kind = build_kind(&root)?.ok_or_else(|| {
            format!(
                "projectRoot {} has no Gradle or Maven build; refusing plain Java fallback",
                root.display()
            )
        })?;
        return Ok((root, kind));
    }
    let mut builds = Vec::new();
    find_builds(workspace, &mut builds)?;
    builds.sort_by(|a, b| a.0.cmp(&b.0));
    match builds.len() {
        0 => Ok((workspace.to_path_buf(), "plain")),
        1 => Ok(builds.remove(0)),
        _ => Err(format!(
            "Multiple Java build projects found: {}. Set projectRoot in .java-hotswap.json or the Zed launch scenario (relative to cwd), or set cwd/--project to the desired build root. Refusing plain Java fallback.",
            builds.iter().map(|(p, kind)| format!("{} ({kind})", p.strip_prefix(workspace).unwrap_or(p).display())).collect::<Vec<_>>().join(", ")
        )),
    }
}
impl Project {
    pub fn discover(
        config: &Config,
        state: &Path,
        emit: &Emit,
        stop: &AtomicBool,
    ) -> Result<Self, String> {
        let workspace = fs::canonicalize(&config.cwd).map_err(|e| e.to_string())?;
        let (root, kind) = select_build(&workspace, config.project_root.as_deref())?;
        emit(
            "console",
            &format!(
                "[Java HotSwap] SELECTED {kind} project {}\n",
                root.display()
            ),
        );
        let mut project = Self {
            root: root.clone(),
            kind: kind.into(),
            modules: vec![],
            tool: build_tool(&root, if kind == "gradle" { "gradle" } else { "mvn" }),
            gradle_runtime: None,
        };
        if kind == "gradle" {
            project.gradle_runtime = Some(crate::gradle_runtime::prepare(
                &root,
                &project.tool,
                config,
                emit,
                stop,
            )?);
            fs::write(
                state.join("init.gradle"),
                include_str!("../gradle.init.gradle"),
            )
            .map_err(|e| e.to_string())?;
            let mut command = project.command(config);
            command
                .args(["--console=plain", "--no-configuration-cache", "-I"])
                .arg(state.join("init.gradle"))
                .arg(format!(
                    "-DjavaHotSwap.metadata={}",
                    state.join("gradle.json").display()
                ))
                .arg("__javaHotSwapMetadata");
            execute(&mut command, emit, stop)?;
            project.modules = serde_json::from_slice(
                &fs::read(state.join("gradle.json")).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        } else if kind == "maven" {
            project.maven_modules(&root, state, config, emit, stop)?;
            // Dependency resolution is delegated to Maven; do not guess JAR names or scan ~/.m2.
            // Reactor dependencies need packaged artifacts. Run package once (without tests), never install into ~/.m2.
            let mut command = project.command(config);
            command.args([
                "-B",
                "-DskipTests",
                "package",
                "org.apache.maven.plugins:maven-dependency-plugin:3.8.1:build-classpath",
                "-DincludeScope=runtime",
                "-Dmdep.outputFile=.java-hotswap/classpath",
            ]);
            execute(&mut command, emit, stop)?;
            let outputs = project
                .modules
                .iter()
                .flat_map(|m| m.artifacts.iter().map(|a| (a.clone(), m.classes.clone())))
                .collect::<BTreeMap<_, _>>();
            for module in &mut project.modules {
                let basedir = PathBuf::from(&module.name);
                let cp =
                    fs::read_to_string(basedir.join(".java-hotswap/classpath")).unwrap_or_default();
                for path in std::env::split_paths(cp.trim()).filter(|p| !p.as_os_str().is_empty()) {
                    if let Some(classes) = outputs.get(&path) {
                        module.classpath.extend(classes.clone());
                    } else {
                        module.classpath.push(path);
                    }
                }
                let _ = fs::remove_file(basedir.join(".java-hotswap/classpath"));
            }
        } else {
            let src = if root.join("src/main/java").exists() {
                root.join("src/main/java")
            } else if root.join("src").exists() {
                root.join("src")
            } else {
                root.clone()
            };
            let classes = state.join("classes");
            let mut classpath = vec![classes.clone()];
            classpath.extend(config.classpath.iter().map(|p| {
                if p.is_absolute() {
                    p.clone()
                } else {
                    root.join(p)
                }
            }));
            project.modules.push(Module {
                name: "plain".into(),
                sources: vec![src],
                classes: vec![classes],
                classpath,
                artifacts: vec![],
                compile_tasks: vec![],
                main_class: None,
                jvm_args: vec![],
                java: None,
                run_tasks: vec![],
            });
        }
        if project.modules.is_empty() {
            return Err("No Java main source sets found. Custom source sets require explicit project support.".into());
        }
        Ok(project)
    }
    fn command(&self, config: &Config) -> Command {
        let mut cmd = Command::new(&self.tool);
        cmd.current_dir(&self.root);
        if let Some(runtime) = &self.gradle_runtime {
            runtime.configure(&mut cmd);
        } else if let Some(home) = &config.java_home {
            cmd.env("JAVA_HOME", home);
        }
        cmd.args(&config.build_args);
        cmd
    }
    fn maven_modules(
        &mut self,
        root: &Path,
        state: &Path,
        config: &Config,
        emit: &Emit,
        stop: &AtomicBool,
    ) -> Result<(), String> {
        let index = self.modules.len();
        let output = state.join(format!("effective-{index}-{}.xml", self.modules.len()));
        let mut cmd = self.command(config);
        cmd.current_dir(root)
            .args([
                "-B",
                "-N",
                "org.apache.maven.plugins:maven-help-plugin:3.5.1:effective-pom",
            ])
            .arg(format!("-Doutput={}", output.display()));
        execute(&mut cmd, emit, stop)?;
        let xml = fs::read_to_string(&output).map_err(|e| e.to_string())?;
        let doc = roxmltree::Document::parse(&xml).map_err(|e| e.to_string())?;
        let p = doc.root_element();
        fn child<'a, 'input>(
            n: roxmltree::Node<'a, 'input>,
            name: &str,
        ) -> Option<roxmltree::Node<'a, 'input>> {
            n.children().find(|c| c.has_tag_name(name))
        }
        let build = child(p, "build").ok_or("Effective POM missing build")?;
        let value = |name: &str, fallback: &str| {
            child(build, name)
                .and_then(|n| n.text())
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join(fallback))
        };
        let classes = value("outputDirectory", "target/classes");
        self.modules.push(Module {
            name: root.to_string_lossy().into_owned(),
            sources: vec![value("sourceDirectory", "src/main/java")],
            classes: vec![classes.clone()],
            classpath: vec![classes],
            artifacts: vec![{
                let directory = value("directory", "target");
                let final_name = child(build, "finalName")
                    .and_then(|n| n.text())
                    .ok_or("Effective POM missing finalName")?;
                directory.join(format!("{final_name}.jar"))
            }],
            compile_tasks: vec![],
            main_class: None,
            jvm_args: vec![],
            java: None,
            run_tasks: vec![],
        });
        if let Some(modules) = child(p, "modules") {
            for module in modules.children().filter(|n| n.has_tag_name("module")) {
                self.maven_modules(
                    &root.join(module.text().ok_or("Empty Maven module")?),
                    state,
                    config,
                    emit,
                    stop,
                )?;
            }
        }
        Ok(())
    }
    pub fn compile(&self, config: &Config, emit: &Emit, stop: &AtomicBool) -> Result<(), String> {
        let mut cmd;
        match self.kind.as_str() {
            "gradle" => {
                cmd = self.command(config);
                cmd.args(["--console=plain", "--no-configuration-cache"]);
                for m in &self.modules {
                    cmd.args(&m.compile_tasks);
                }
            }
            "maven" => {
                cmd = self.command(config);
                cmd.args(["-B", "-DskipTests", "compile"]);
            }
            _ => {
                let m = &self.modules[0];
                let staged = m.classes[0].with_file_name("classes-next");
                if staged.exists() {
                    fs::remove_dir_all(&staged).map_err(|e| e.to_string())?;
                }
                fs::create_dir_all(&staged).map_err(|e| e.to_string())?;
                let files = java_files(&m.sources)?;
                if files.is_empty() {
                    return Err("No Java sources found".into());
                }
                cmd = Command::new(jdk_tool(config.java_home.as_deref(), "javac"));
                cmd.current_dir(&self.root)
                    .args(["-g", "-d"])
                    .arg(&staged)
                    .arg("-classpath")
                    .arg(
                        std::env::join_paths(
                            std::iter::once(&staged).chain(m.classpath.iter().skip(1)),
                        )
                        .map_err(|e| e.to_string())?,
                    )
                    .args(files);
                execute(&mut cmd, emit, stop)?;
                let old = m.classes[0].with_file_name("classes-old");
                if old.exists() {
                    fs::remove_dir_all(&old).map_err(|e| e.to_string())?;
                }
                let had_output = m.classes[0].exists();
                if had_output {
                    fs::rename(&m.classes[0], &old).map_err(|e| e.to_string())?;
                }
                if let Err(error) = fs::rename(&staged, &m.classes[0]) {
                    if had_output {
                        let _ = fs::rename(&old, &m.classes[0]);
                    }
                    return Err(error.to_string());
                }
                if had_output {
                    fs::remove_dir_all(old).map_err(|e| e.to_string())?;
                }
                return Ok(());
            }
        }
        execute(&mut cmd, emit, stop)
    }
    pub fn entry(&self, config: &Config) -> Result<(String, &Module), String> {
        if self.kind == "gradle" {
            let (task, module) = self.run_task(config)?;
            return Ok((
                task.main_class.clone().unwrap_or_else(|| {
                    format!(
                        "{} (mainClass resolved during launch preparation)",
                        task.name
                    )
                }),
                module,
            ));
        }
        let modules: Vec<_> = self
            .modules
            .iter()
            .filter(|m| config.module.as_ref().is_none_or(|name| name == &m.name))
            .collect();
        if modules.is_empty() {
            return Err("Selected module was not found".into());
        }
        let main = Regex::new(r"\b(?:public\s+static|static\s+public)\s+void\s+main\s*\(\s*(?:java\.lang\.)?String\s*(?:\[\s*\]|\.\.\.)").unwrap();
        let package = Regex::new(r"\bpackage\s+([\w.]+)\s*;").unwrap();
        if let Some(main) = &config.main_class {
            let relative = format!("{}.java", main.replace('.', "/"));
            let matching: Vec<_> = modules
                .iter()
                .copied()
                .filter(|m| m.sources.iter().any(|s| s.join(&relative).is_file()))
                .collect();
            if matching.len() == 1 {
                return Ok((main.clone(), matching[0]));
            }
            if modules.len() == 1 {
                return Ok((main.clone(), modules[0]));
            }
            return Err("mainClass is ambiguous across modules; set module explicitly".into());
        }
        let mut entries = vec![];
        for m in modules {
            if let Some(main) = &m.main_class {
                entries.push((main.clone(), m));
                continue;
            }
            for path in java_files(&m.sources)? {
                let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
                // Conventional main only; ambiguity is explicit, never silently choose a process.
                let cleaned = strip_comments(&text);
                if main.is_match(&cleaned) {
                    let class = path.file_stem().unwrap().to_string_lossy();
                    let name = package
                        .captures(&cleaned)
                        .map(|c| format!("{}.{}", &c[1], class))
                        .unwrap_or_else(|| class.into_owned());
                    entries.push((name, m));
                }
            }
        }
        if entries.len() == 1 {
            Ok(entries.remove(0))
        } else {
            Err(format!(
                "Found {} entry points {:?}; set mainClass and module in .java-hotswap.json",
                entries.len(),
                entries
                    .iter()
                    .map(|(c, m)| format!("{}:{c}", m.name))
                    .collect::<Vec<_>>()
            ))
        }
    }
    pub fn run_task(&self, config: &Config) -> Result<(&RunTask, &Module), String> {
        let tasks: Vec<_> = self
            .modules
            .iter()
            .filter(|m| config.module.as_ref().is_none_or(|name| name == &m.name))
            .flat_map(|m| m.run_tasks.iter().map(move |t| (t, m)))
            .filter(|(t, _)| {
                config.gradle_run_task.as_ref().is_none_or(|name| {
                    name == &t.name || t.name.rsplit(':').next() == Some(name.as_str())
                })
            })
            .collect();
        match tasks.as_slice() {
            [selected] => Ok(*selected),
            [] => Err("No usable Gradle JavaExec launch task found (or gradleRunTask/module did not match). Configure a JavaExec task or the application plugin with mainClass, runtime classpath and required application arguments, then select gradleRunTask. HotSwap will not guess required application arguments or launch a source main directly.".into()),
            _ => Err(format!("Multiple Gradle JavaExec launch tasks found: {}. Select gradleRunTask in the Zed launch scenario/.java-hotswap.json or --gradle-run-task on the CLI.", tasks.iter().map(|(t, _)| t.name.as_str()).collect::<Vec<_>>().join(", "))),
        }
    }
    pub fn prepare_launch(
        &self,
        config: &Config,
        state: &Path,
        emit: &Emit,
        stop: &AtomicBool,
    ) -> Result<Launch, String> {
        let (task, _) = self.run_task(config)?;
        let path = state.join("launch.json");
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        // Remove stale data before recreating a private, ephemeral launch file.
        if path.exists() {
            fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        drop(options.open(&path).map_err(|e| e.to_string())?);
        let result = (|| {
            let mut command = self.command(config);
            command
                .args(["--console=plain", "--no-configuration-cache", "-I"])
                .arg(state.join("init.gradle"))
                .arg(format!("-DjavaHotSwap.runTask={}", task.name))
                .arg(format!("-DjavaHotSwap.launch={}", path.display()))
                .arg(&task.name);
            execute(&mut command, emit, stop)?;
            let launch: Launch = serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
                .map_err(|_| "Selected Gradle run task did not produce a usable launch configuration (it may be disabled/skipped)".to_string())?;
            if config
                .main_class
                .as_ref()
                .is_some_and(|main| main != &launch.main_class)
            {
                return Err("mainClass conflicts with the selected Gradle run task; select the matching gradleRunTask instead".into());
            }
            if launch.classpath.is_empty() {
                return Err("Selected Gradle JavaExec task has no runtime classpath; configure the task's classpath".into());
            }
            Ok(launch)
        })();
        let cleanup = fs::remove_file(&path)
            .map_err(|e| format!("Cannot remove private Gradle launch metadata: {e}"));
        cleanup?;
        result
    }
}
fn strip_comments(s: &str) -> String {
    Regex::new(r#"(?s)/\*.*?\*/|//[^\n]*|""".*?"""|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'"#)
        .unwrap()
        .replace_all(s, " ")
        .into_owned()
}
pub fn java_files(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    fn walk(path: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
        if !path.exists() {
            return Ok(());
        }
        for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let p = entry.path();
            if entry.file_type().map_err(|e| e.to_string())?.is_symlink() {
                continue;
            }
            if p.is_dir() {
                if !matches!(
                    entry.file_name().to_str(),
                    Some(
                        ".git" | ".gradle" | ".java-hotswap" | "target" | "build" | "node_modules"
                    )
                ) {
                    walk(&p, out)?;
                }
            } else if p.extension().is_some_and(|e| e == "java") {
                out.push(p);
            }
        }
        Ok(())
    }
    let mut out = vec![];
    for root in roots {
        walk(root, &mut out)?;
    }
    out.sort();
    out.dedup();
    Ok(out)
}
pub fn snapshot(roots: &[PathBuf]) -> Result<BTreeMap<PathBuf, u64>, String> {
    use std::hash::{Hash, Hasher};
    let mut result = BTreeMap::new();
    for p in java_files(roots)? {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        fs::read(&p).map_err(|e| e.to_string())?.hash(&mut h);
        result.insert(p, h.finish());
    }
    Ok(result)
}

#[cfg(test)]
mod discovery_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn gradle_run_task_selection_does_not_guess_from_source_main() {
        let module: Module = serde_json::from_value(serde_json::json!({
            "name":":app", "sources":[], "classes":[], "classpath":[],
            "runTasks":[{"name":":app:runClient", "mainClass":"demo.Main"},
                        {"name":":app:runServer", "mainClass":"demo.Main"}]
        }))
        .unwrap();
        let mut project = Project {
            root: ".".into(),
            kind: "gradle".into(),
            modules: vec![module],
            tool: "gradle".into(),
            gradle_runtime: None,
        };
        assert!(project
            .run_task(&Config::default())
            .unwrap_err()
            .contains("Multiple Gradle JavaExec"));
        let config = Config {
            gradle_run_task: Some("runClient".into()),
            ..Config::default()
        };
        assert_eq!(project.run_task(&config).unwrap().0.name, ":app:runClient");
        project.modules[0].run_tasks.clear();
        assert!(project
            .run_task(&config)
            .unwrap_err()
            .contains("No usable Gradle JavaExec"));
    }

    struct Workspace(PathBuf);
    impl Workspace {
        fn new() -> Self {
            static ID: AtomicUsize = AtomicUsize::new(0);
            let p = std::env::temp_dir().join(format!(
                "hotswap-discovery-{}-{}",
                std::process::id(),
                ID.fetch_add(1, Ordering::SeqCst)
            ));
            fs::create_dir_all(&p).unwrap();
            Self(fs::canonicalize(p).unwrap())
        }
        fn file(&self, name: &str) {
            let p = self.0.join(name);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, "").unwrap();
        }
    }
    impl Drop for Workspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn recursive_kotlin_gradle_and_reactor_ownership() {
        let w = Workspace::new();
        w.file("deep/java/settings.gradle.kts");
        w.file("deep/java/app/build.gradle.kts");
        assert_eq!(
            select_build(&w.0, None).unwrap(),
            (w.0.join("deep/java"), "gradle")
        );
        let w = Workspace::new();
        w.file("java/pom.xml");
        w.file("java/module/pom.xml");
        assert_eq!(
            select_build(&w.0, None).unwrap(),
            (w.0.join("java"), "maven")
        );
    }

    #[test]
    fn ambiguity_requires_explicit_valid_build_selection() {
        let w = Workspace::new();
        w.file("game/build.gradle");
        w.file("tools/pom.xml");
        let error = select_build(&w.0, None).unwrap_err();
        assert!(error.contains("game (gradle), tools (maven)"));
        assert_eq!(
            select_build(&w.0, Some(Path::new("tools"))).unwrap(),
            (w.0.join("tools"), "maven")
        );
        assert!(select_build(&w.0, Some(Path::new(".")))
            .unwrap_err()
            .contains("refusing plain Java fallback"));
        assert!(select_build(&w.0, Some(Path::new("missing"))).is_err());
    }

    #[test]
    fn plain_only_without_builds_and_generated_trees_are_ignored() {
        let w = Workspace::new();
        w.file("src/Main.java");
        for ignored in [
            ".git",
            ".gradle",
            ".java-hotswap",
            "build",
            "build.nosync",
            "target",
            "node_modules",
        ] {
            w.file(&format!("{ignored}/copy/build.gradle"));
        }
        assert_eq!(select_build(&w.0, None).unwrap(), (w.0.clone(), "plain"));
        w.file("java/gradlew");
        assert_eq!(
            select_build(&w.0, None).unwrap(),
            (w.0.join("java"), "gradle")
        );
    }

    #[test]
    fn conflicting_build_tools_fail_explicitly() {
        let w = Workspace::new();
        w.file("java/build.gradle");
        w.file("java/pom.xml");
        assert!(select_build(&w.0, None)
            .unwrap_err()
            .contains("Both Gradle and Maven"));
    }
}
