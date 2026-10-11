use crate::{
    process::{Emit, Managed},
    project::{jdk_tool, snapshot, Project},
    Config,
};
use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    path::Path,
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

pub fn lock(state: &Path) -> Result<File, String> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(state.join("session.lock"))
        .map_err(|e| e.to_string())?;
    file.try_lock_exclusive().map_err(|_| {
        "A Java HotSwap session or discovery is already running for this project".to_string()
    })?;
    Ok(file)
}
fn status(emit: &Emit, text: &str) {
    emit("stderr", &format!("[Java HotSwap] {text}\n"));
}
pub fn run(config: Config, stop: Arc<AtomicBool>, emit: Emit) -> Result<(), String> {
    let root = fs::canonicalize(&config.cwd).map_err(|e| e.to_string())?;
    let state = root.join(".java-hotswap");
    fs::create_dir_all(&state).map_err(|e| e.to_string())?;
    let _lock = lock(&state)?;
    for name in ["stop", "clear", "commit", "ack"] {
        let _ = fs::remove_file(state.join(name));
    }
    let _ = fs::remove_dir_all(state.join("reloads"));
    // Control requests must also cancel a long-running build, not just the idle watcher.
    let control_stop = stop.clone();
    let control_state = state.clone();
    let control_emit = emit.clone();
    let control = thread::spawn(move || {
        while !control_stop.load(Ordering::SeqCst) {
            if control_state.join("stop").exists() {
                control_stop.store(true, Ordering::SeqCst);
            }
            if control_state.join("clear").exists() {
                control_emit("stdout", "\x1b[2J\x1b[H");
                let _ = fs::remove_file(control_state.join("clear"));
            }
            thread::sleep(Duration::from_millis(100));
        }
    });
    let result = run_inner(&config, &state, &stop, &emit);
    stop.store(true, Ordering::SeqCst);
    let _ = control.join();
    status(&emit, "STOPPED");
    if result.as_ref().is_err_and(|e| e == "Session cancelled") {
        Ok(())
    } else {
        result
    }
}
fn run_inner(config: &Config, state: &Path, stop: &AtomicBool, emit: &Emit) -> Result<(), String> {
    status(
        emit,
        "DISCOVERING project (build scripts execute as with a normal build)",
    );
    let project = Project::discover(config, state, emit, stop)?;
    status(
        emit,
        &format!(
            "BUILD TOOL {} (cwd={})",
            project.tool.display(),
            project.root.display()
        ),
    );
    let (main, module) = project.entry(config)?;
    status(emit, &format!("BUILDING {} / {main}", project.kind));
    let sources = project
        .modules
        .iter()
        .flat_map(|m| m.sources.clone())
        .collect::<Vec<_>>();
    let mut seen = snapshot(&sources)?;
    project.compile(config, emit, stop)?;
    let launch = if project.kind == "gradle" {
        let launch = project.prepare_launch(config, state, emit, stop)?;
        status(
            emit,
            &format!("LAUNCH TASK {} / {}", launch.task, launch.main_class),
        );
        Some(launch)
    } else {
        None
    };
    fs::write(
        state.join("agent.jar"),
        include_bytes!(concat!(env!("OUT_DIR"), "/java-hotswap-agent.jar")),
    )
    .map_err(|e| e.to_string())?;
    let roots = project
        .modules
        .iter()
        .flat_map(|m| m.classes.iter())
        .collect::<Vec<_>>();
    let mut agent_config = format!("{}\n", state.join("commit").display());
    for root in roots {
        agent_config.push_str(&format!("{}\n", root.display()));
    }
    if agent_config.contains('\r')
        || agent_config.lines().count()
            != 1 + project
                .modules
                .iter()
                .map(|m| m.classes.len())
                .sum::<usize>()
    {
        return Err("Newlines in project paths are unsupported".into());
    }
    fs::write(state.join("agent.conf"), agent_config).map_err(|e| e.to_string())?;
    let java = if config.java_home.is_some() {
        jdk_tool(config.java_home.as_deref(), "java")
    } else if let Some(launch) = &launch {
        launch.java.clone().into()
    } else {
        module
            .java
            .as_ref()
            .map(Into::into)
            .unwrap_or_else(|| jdk_tool(None, "java"))
    };
    let mut cmd = Command::new(java);
    cmd.current_dir(
        launch
            .as_ref()
            .map(|l| l.working_directory.as_path())
            .unwrap_or(&project.root),
    )
    .args(
        launch
            .as_ref()
            .map(|l| l.jvm_args.as_slice())
            .unwrap_or(&module.jvm_args),
    )
    .args(&config.jvm_args);
    if let Some(launch) = &launch {
        cmd.env_clear().envs(&launch.environment);
    }
    if let Some(port) = config.jdwp_port {
        if port == 0 {
            return Err("jdwpPort must be between 1 and 65535".into());
        }
        cmd.arg(format!(
            "-agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=127.0.0.1:{port}"
        ));
    }
    cmd.arg(format!(
        "-javaagent:{}={}",
        state.join("agent.jar").display(),
        state.join("agent.conf").display()
    ));
    let cp = launch
        .as_ref()
        .map(|l| &l.classpath)
        .unwrap_or(&module.classpath);
    cmd.arg("-cp")
        .arg(std::env::join_paths(cp).map_err(|e| e.to_string())?)
        .arg(
            launch
                .as_ref()
                .map(|l| l.main_class.as_str())
                .unwrap_or(&main),
        );
    if let Some(launch) = &launch {
        cmd.args(&launch.args);
    }
    cmd.args(&config.args);
    let mut app = Managed::spawn(&mut cmd, emit)?;
    status(
        emit,
        &format!("RUNNING pid={} (save a Java source to compile)", app.0.id()),
    );
    let mut dirty_since = None;
    let mut generation = 0u64;
    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        if let Some(exit) = app.0.try_wait().map_err(|e| e.to_string())? {
            status(emit, &format!("EXITED {exit}"));
            if !exit.success() {
                return Err(format!("Application exited with {exit}"));
            }
            break;
        }
        let next = match snapshot(&sources) {
            Ok(next) => next,
            Err(error) => {
                status(
                    emit,
                    &format!("WATCH ERROR: {error}; application keeps running"),
                );
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        if next != seen {
            seen = next;
            dirty_since = Some(Instant::now());
        }
        if dirty_since
            .is_some_and(|t| t.elapsed() >= Duration::from_millis(config.debounce_ms.max(50)))
        {
            dirty_since = None;
            status(emit, "COMPILING");
            match project.compile(config, emit, stop) {
                Ok(()) => {
                    generation += 1;
                    match publish_snapshot(&project, state, generation) {
                        Ok(()) => status(emit, "COMPILED (reload requested)"),
                        Err(error) => status(
                            emit,
                            &format!("RELOAD FAILED: {error}; application keeps running"),
                        ),
                    }
                }
                Err(e) if stop.load(Ordering::SeqCst) => return Err(e),
                Err(e) => status(
                    emit,
                    &format!("COMPILE FAILED: {e}; application keeps running"),
                ),
            }
            // `seen` remains the pre-build snapshot: edits during a build trigger a subsequent job.
        }
        thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

fn copy_classes(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|e| e.to_string())?;
    if !source.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() {
            continue;
        }
        let target = destination.join(entry.file_name());
        if kind.is_dir() {
            copy_classes(&entry.path(), &target)?;
        } else if entry.path().extension().is_some_and(|e| e == "class") {
            fs::copy(entry.path(), target).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn publish_snapshot(project: &Project, state: &Path, generation: u64) -> Result<(), String> {
    let snapshot_dir = state.join("reloads").join(generation.to_string());
    for (index, root) in project.modules.iter().flat_map(|m| &m.classes).enumerate() {
        copy_classes(root, &snapshot_dir.join(index.to_string()))?;
    }
    fs::write(
        state.join("commit.next"),
        snapshot_dir.to_string_lossy().as_bytes(),
    )
    .map_err(|e| e.to_string())?;
    #[cfg(windows)]
    {
        let _ = fs::remove_file(state.join("commit"));
    }
    fs::rename(state.join("commit.next"), state.join("commit")).map_err(|e| e.to_string())?;
    // Pruning is best effort; it must never stop a running JVM or invalidate a commit.
    if let Ok(ack) = fs::read_to_string(state.join("ack")) {
        if let Ok(ack) = ack.parse::<u64>() {
            if let Ok(entries) = fs::read_dir(state.join("reloads")) {
                for entry in entries.flatten() {
                    if entry
                        .file_name()
                        .to_string_lossy()
                        .parse::<u64>()
                        .is_ok_and(|n| n < ack)
                    {
                        let _ = fs::remove_dir_all(entry.path());
                    }
                }
            }
        }
    }
    Ok(())
}
