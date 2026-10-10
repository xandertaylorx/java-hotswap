//! Gradle daemon compatibility is separate from the application's Java toolchain.
use crate::{
    jdks::{self, Jdk},
    process::{capture, Emit},
    properties, Config,
};
use regex::Regex;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::AtomicBool,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Version(u32, u32, u32);
impl Version {
    fn parse(text: &str) -> Result<Self, String> {
        let re = Regex::new(r"^(\d+)\.(\d+)(?:\.(\d+))?$").unwrap();
        let c = re.captures(text).ok_or_else(|| format!("Unrecognized or prerelease Gradle version '{text}'; set gradleVersion to a verified stable distribution version"))?;
        Ok(Self(
            c[1].parse().map_err(|_| "Invalid Gradle major")?,
            c[2].parse().map_err(|_| "Invalid Gradle minor")?,
            c.get(3)
                .map(|v| v.as_str().parse())
                .transpose()
                .map_err(|_| "Invalid Gradle patch")?
                .unwrap_or(0),
        ))
    }
    fn text(self) -> String {
        format!("{}.{}.{}", self.0, self.1, self.2)
    }
    fn range(self) -> Result<(u32, u32), String> {
        // Official runtime matrix (not the distinct Java compilation/toolchain matrix).
        // https://docs.gradle.org/current/userguide/compatibility.html
        if self < Self(7, 3, 0) || self > Self(9, 8, 1) {
            return Err(format!("Gradle {} is outside the verified HotSwap runtime table (7.3–9.8.1). Wrapper left unchanged; update the extension's compatibility table before using this distribution.", self.text()));
        }
        let mut max = 17;
        for (version, java) in [
            (Self(7, 5, 0), 18),
            (Self(7, 6, 0), 19),
            (Self(8, 3, 0), 20),
            (Self(8, 5, 0), 21),
            (Self(8, 8, 0), 22),
            (Self(8, 10, 0), 23),
            (Self(8, 14, 0), 24),
            (Self(9, 1, 0), 25),
            (Self(9, 4, 0), 26),
            (Self(9, 8, 0), 27),
        ] {
            if self >= version {
                max = java;
            }
        }
        Ok((if self.0 >= 9 { 17 } else { 8 }, max))
    }
}
fn distribution_version(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next()?;
    let name = path.rsplit('/').next()?;
    let c = Regex::new(r"^gradle-(\d+\.\d+(?:\.\d+)?)-(?:bin|all)\.zip$")
        .unwrap()
        .captures(name)?;
    Some(c[1].to_string())
}
fn version_hint(root: &Path, tool: &Path, configured: Option<&str>) -> Result<Version, String> {
    // Always read wrapper properties before running any wrapper command.
    let path = root.join("gradle/wrapper/gradle-wrapper.properties");
    let props = properties::read(&path)?;
    let wrapper = tool
        .file_name()
        .is_some_and(|n| n == "gradlew" || n == "gradlew.bat");
    let inferred = if wrapper {
        props
            .get("distributionUrl")
            .and_then(|url| distribution_version(url))
    } else {
        // Resolve global Gradle installations without starting Gradle on an unknown JVM.
        let resolved = if tool.is_absolute() {
            fs::canonicalize(tool).ok()
        } else {
            jdks::path_executable(&tool.to_string_lossy())
        };
        resolved
            .and_then(|p| p.parent()?.parent().map(|p| p.join("lib")))
            .and_then(|lib| fs::read_dir(lib).ok())
            .and_then(|entries| {
                entries.flatten().find_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    ["gradle-launcher-", "gradle-gradle-cli-main-"]
                        .iter()
                        .find_map(|prefix| {
                            name.strip_prefix(prefix)?
                                .strip_suffix(".jar")
                                .map(str::to_owned)
                        })
                })
            })
    };
    let inferred = inferred.as_deref().map(Version::parse).transpose()?;
    let configured = configured.map(Version::parse).transpose()?;
    if let (Some(a), Some(b)) = (inferred, configured) {
        if a != b {
            return Err("gradleVersion disagrees with the distribution version; refusing to guess runtime compatibility".into());
        }
    }
    configured.or(inferred).ok_or_else(|| format!(
        "Cannot determine Gradle version before startup ({}). Custom/unversioned distribution or launcher: set gradleVersion to its verified stable Gradle version in the HotSwap launch configuration. The wrapper will not be rewritten or upgraded.", path.display()))
}

#[derive(Debug)]
pub struct Runtime {
    pub home: PathBuf,
    pub homes: Vec<PathBuf>,
    version: Version,
    daemon_criteria: bool,
}
impl Runtime {
    pub fn configure(&self, cmd: &mut Command) {
        cmd.env("JAVA_HOME", &self.home);
        cmd.arg(format!(
            "-Dorg.gradle.java.installations.paths={}",
            self.homes
                .iter()
                .map(|p| p.to_string_lossy())
                .collect::<Vec<_>>()
                .join(",")
        ));
        // Prevent project/global org.gradle.java.home from reselecting an incompatible
        // daemon. An existing compatible override was validated and selected already.
        // Gradle daemon criteria take precedence and remain owned by Gradle.
        if !self.daemon_criteria {
            cmd.arg(format!("-Dorg.gradle.java.home={}", self.home.display()));
        }
    }
}
fn choose(version: Version, candidates: &[Jdk], explicit: Option<&Jdk>) -> Result<Jdk, String> {
    let (min, max) = version.range()?;
    let compatible = |j: &&Jdk| j.major >= min && j.major <= max;
    if let Some(jdk) = explicit {
        if jdk.major < min || jdk.major > max {
            return Err(format!("Gradle {} requires a runtime JDK in Java {min}–{max}, but its configured build JDK is Java {} at {}. Choose a compatible gradleJavaHome (application javaHome/toolchains are separate); wrapper left unchanged.", version.text(), jdk.major, jdk.home.display()));
        }
        return Ok(jdk.clone());
    }
    candidates.iter().find(compatible).cloned().ok_or_else(|| format!(
        "No compatible local JDK for Gradle {}. Runtime support: Java {min}–{max}. Detected: {}. Install a supported JDK (for example Java {}) or set gradleJavaHome to an existing supported JDK. Keep the application's toolchain unchanged; the wrapper was not modified.",
        version.text(), candidates.iter().map(|j| format!("Java {} ({})", j.major, j.home.display())).collect::<Vec<_>>().join(", "), if max >= 21 { 21 } else { 17 }))
}
pub fn prepare(
    root: &Path,
    tool: &Path,
    config: &Config,
    emit: &Emit,
    stop: &AtomicBool,
) -> Result<Runtime, String> {
    let version = version_hint(root, tool, config.gradle_version.as_deref())?;
    let (min, max) = version.range()?;
    let mut properties = std::collections::BTreeMap::new();
    if let Some(user) = std::env::var_os("GRADLE_USER_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|p| PathBuf::from(p).join(".gradle"))
        })
    {
        properties.extend(crate::properties::read(&user.join("gradle.properties"))?);
    }
    // Gradle user-home properties have precedence over project properties.
    for (key, value) in crate::properties::read(&root.join("gradle.properties"))? {
        properties.entry(key).or_insert(value);
    }
    for arg in &config.build_args {
        if let Some(home) = arg.strip_prefix("-Dorg.gradle.java.home=") {
            properties.insert("org.gradle.java.home".into(), home.into());
        }
    }
    let criteria = crate::properties::read(&root.join("gradle/gradle-daemon-jvm.properties"))?;
    if let Some(java) = criteria.get("toolchainVersion") {
        let major: u32 = java
            .parse()
            .map_err(|_| "Invalid Gradle daemon toolchainVersion")?;
        if major < min || major > max {
            return Err(format!("Gradle {} supports runtime Java {min}–{max}, but gradle-daemon-jvm.properties requests Java {major}. Use a compatible daemon JDK; HotSwap will not rewrite the project's daemon criteria or wrapper.", version.text()));
        }
    }
    let daemon_criteria = criteria.contains_key("toolchainVersion");
    let explicit_home = config
        .gradle_java_home
        .as_ref()
        .map(|p| config.cwd.join(p))
        .or_else(|| {
            if daemon_criteria {
                None
            } else {
                properties.get("org.gradle.java.home").map(PathBuf::from)
            }
        });
    let explicit = explicit_home.as_ref().map(|home| jdks::from_home(home).ok_or_else(|| format!("Configured Gradle JDK {} is not a complete local JDK. Set gradleJavaHome to a directory containing bin/java, bin/javac and release metadata.", home.display()))).transpose()?;
    let candidates = jdks::installed(stop);
    let mut usable = candidates.clone();
    let selected = loop {
        let selected = choose(version, &usable, explicit.as_ref())?;
        // An installed JDK may be for a different architecture or have broken
        // permissions. Automatic selection should try the remaining candidates.
        match capture(
            Command::new(jdks::executable(&selected.home, "java")).arg("-version"),
            stop,
        ) {
            Ok(_) => break selected,
            Err(e) if explicit.is_some() || stop.load(std::sync::atomic::Ordering::SeqCst) => {
                return Err(format!("Selected Gradle JDK cannot run: {e}"))
            }
            Err(_) => usable.retain(|j| j.home != selected.home),
        }
    };
    let mut homes: Vec<_> = candidates.iter().map(|j| j.home.clone()).collect();
    if let Some(paths) = properties.get("org.gradle.java.installations.paths") {
        homes.extend(paths.split(',').map(|p| PathBuf::from(p.trim())));
    }
    for arg in &config.build_args {
        if let Some(paths) = arg.strip_prefix("-Dorg.gradle.java.installations.paths=") {
            homes.extend(paths.split(',').map(|p| PathBuf::from(p.trim())));
        }
    }
    if !homes.contains(&selected.home) {
        homes.push(selected.home.clone());
    }
    let runtime = Runtime {
        home: selected.home.clone(),
        homes,
        version,
        daemon_criteria,
    };
    emit(
        "console",
        &format!(
            "[Java HotSwap] GRADLE {} runtime Java {} at {} (application toolchain unchanged)\n",
            version.text(),
            selected.major,
            selected.home.display()
        ),
    );
    let mut cmd = Command::new(tool);
    cmd.current_dir(root).args(&config.build_args);
    runtime.configure(&mut cmd);
    cmd.arg("--version");
    let output = capture(&mut cmd, stop)
        .map_err(|e| format!("Gradle runtime preflight failed before build scripts: {e}"))?;
    let actual = Regex::new(r"(?m)^Gradle (\S+)\s*$").unwrap().captures(&output)
        .ok_or("Wrapper did not report a recognizable Gradle version; refusing to assume distribution compatibility")?[1].to_string();
    if Version::parse(&actual)? != runtime.version {
        return Err(format!("Wrapper reported Gradle {actual}, but its configured distribution indicates {}. Verify the custom distribution/gradleVersion; no build scripts were executed.", runtime.version.text()));
    }
    // A custom wrapper may ignore JAVA_HOME. Validate its reported launcher JVM
    // rather than treating the script name or URL as proof of runtime selection.
    let reported_java = Regex::new(r"(?m)^(?:Launcher JVM|JVM):\s+(?:1\.)?(\d+)").unwrap();
    if let Some(c) = reported_java.captures(&output) {
        let java: u32 = c[1].parse().map_err(|_| "Invalid reported Gradle JVM")?;
        if java < min || java > max {
            return Err(format!("Gradle wrapper launched Java {java} despite JAVA_HOME={}, but Gradle {} requires Java {min}–{max}. Check the custom wrapper's JVM selection; build scripts were not executed.", runtime.home.display(), version.text()));
        }
    } else {
        return Err("Gradle did not report its launcher JVM; custom launchers must expose standard --version output before HotSwap can verify runtime compatibility".into());
    }
    Ok(runtime)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compatibility_boundaries() {
        for (version, range) in [
            ("7.3", (8, 17)),
            ("8.10", (8, 23)),
            ("8.14.3", (8, 24)),
            ("9.0.0", (17, 24)),
            ("9.1", (17, 25)),
            ("9.4", (17, 26)),
            ("9.8.1", (17, 27)),
        ] {
            assert_eq!(Version::parse(version).unwrap().range().unwrap(), range);
        }
        assert!(Version::parse("9.1-rc-1").is_err());
        assert!(Version::parse("10.0").unwrap().range().is_err());
    }
    #[test]
    fn java25_default_selects21_for_gradle810_but_stays25_for91() {
        let candidates = [
            Jdk {
                home: "jdk25".into(),
                major: 25,
            },
            Jdk {
                home: "jdk21".into(),
                major: 21,
            },
        ];
        assert_eq!(
            choose(Version(8, 10, 0), &candidates, None).unwrap().major,
            21
        );
        assert_eq!(
            choose(Version(9, 1, 0), &candidates, None).unwrap().major,
            25
        );
        assert!(choose(Version(8, 10, 0), &candidates[..1], None)
            .unwrap_err()
            .contains("Install a supported JDK"));
        assert!(choose(Version(8, 10, 0), &candidates, Some(&candidates[0]))
            .unwrap_err()
            .contains("configured build JDK"));
    }
    #[test]
    fn custom_distribution_versions_are_not_guessed() {
        assert_eq!(
            distribution_version("https://mirror/private/gradle-8.10-all.zip?token=x"),
            Some("8.10".into())
        );
        for url in [
            "https://mirror/custom.zip",
            "https://mirror/gradle-8.10-patched-bin.zip",
            "https://mirror/8.10/gradle.zip",
            "https://mirror/gradle-9.1-rc-1-bin.zip",
        ] {
            assert_eq!(distribution_version(url), None);
        }
    }
}
