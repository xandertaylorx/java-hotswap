use crate::process::capture;
use regex::Regex;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::AtomicBool,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Jdk {
    pub home: PathBuf,
    pub major: u32,
}
pub fn executable(home: &Path, name: &str) -> PathBuf {
    home.join("bin").join(if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.into()
    })
}
pub fn from_home(path: &Path) -> Option<Jdk> {
    let home = fs::canonicalize(path).ok()?;
    if !executable(&home, "java").is_file() || !executable(&home, "javac").is_file() {
        return None;
    }
    let release = fs::read_to_string(home.join("release")).ok()?;
    let version = Regex::new(r#"(?m)^JAVA_VERSION="([^"]+)""#)
        .unwrap()
        .captures(&release)?[1]
        .to_string();
    if version.contains("-ea") {
        return None;
    }
    let major = version
        .strip_prefix("1.")
        .unwrap_or(&version)
        .split(['.', '-', '+'])
        .next()?
        .parse()
        .ok()?;
    Some(Jdk { home, major })
}
pub fn path_executable(name: &str) -> Option<PathBuf> {
    let names = if cfg!(windows) && Path::new(name).extension().is_none() {
        vec![
            format!("{name}.exe"),
            format!("{name}.com"),
            format!("{name}.bat"),
            format!("{name}.cmd"),
        ]
    } else {
        vec![name.to_owned()]
    };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .flat_map(|p| names.iter().map(move |name| p.join(name)))
        .find(|p| p.is_file())
        .and_then(|p| fs::canonicalize(p).ok())
}
fn add_tree(root: &Path, depth: usize, out: &mut Vec<Jdk>, seen: &mut BTreeSet<PathBuf>) {
    for home in [root.to_path_buf(), root.join("Contents/Home")] {
        if let Some(jdk) = from_home(&home) {
            if seen.insert(jdk.home.clone()) {
                out.push(jdk);
            }
            return;
        }
    }
    if depth == 0 {
        return;
    }
    if let Ok(entries) = fs::read_dir(root) {
        let mut dirs: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        dirs.sort();
        for dir in dirs {
            add_tree(&dir, depth - 1, out, seen);
        }
    }
}
/// Ordered candidates: current JAVA_HOME/PATH first, followed by discovered JDKs.
/// Only complete JDKs with release metadata qualify; JREs and EA builds do not.
pub fn installed(stop: &AtomicBool) -> Vec<Jdk> {
    let mut roots = vec![];
    if let Some(home) = std::env::var_os("JAVA_HOME") {
        roots.push((PathBuf::from(home), 0));
    }
    if let Some(java) = path_executable("java") {
        if let Some(home) = java.parent().and_then(Path::parent) {
            roots.push((home.to_path_buf(), 0));
        }
        // macOS /usr/bin/java and shim managers may resolve outside the JDK.
        if let Ok(output) = capture(
            Command::new(java).args(["-XshowSettings:properties", "-version"]),
            stop,
        ) {
            for line in output.lines() {
                if let Some(home) = line.trim().strip_prefix("java.home = ") {
                    roots.push((home.into(), 0));
                }
            }
        }
    }
    if let Some(paths) = std::env::var_os("JAVA_HOTSWAP_JDK_PATHS") {
        roots.extend(std::env::split_paths(&paths).map(|p| (p, 2)));
    }
    let user = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    if let Some(user) = &user {
        for dir in [
            ".sdkman/candidates/java",
            ".asdf/installs/java",
            ".jabba/jdk",
            ".jdks",
        ] {
            roots.push((user.join(dir), 2));
        }
        let gradle = std::env::var_os("GRADLE_USER_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| user.join(".gradle"));
        roots.push((gradle.join("jdks"), 2));
        #[cfg(target_os = "macos")]
        roots.push((user.join("Library/Java/JavaVirtualMachines"), 1));
    }
    #[cfg(target_os = "macos")]
    {
        roots.push(("/Library/Java/JavaVirtualMachines".into(), 1));
        // Homebrew keg directories and their opt links.
        for dir in ["/opt/homebrew/opt", "/usr/local/opt"] {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries
                    .flatten()
                    .filter(|e| e.file_name().to_string_lossy().starts_with("openjdk"))
                {
                    roots.push((entry.path().join("libexec/openjdk.jdk"), 0));
                }
            }
        }
    }
    #[cfg(target_os = "linux")]
    for dir in [
        "/usr/lib/jvm",
        "/usr/lib64/jvm",
        "/usr/java",
        "/usr/local/lib/jvm",
        "/opt/java",
        "/opt/jdk",
        "/opt/jdks",
    ] {
        roots.push((dir.into(), 2));
    }
    #[cfg(windows)]
    {
        for env in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            if let Some(base) = std::env::var_os(env) {
                for vendor in [
                    "Java",
                    "Eclipse Adoptium",
                    "Microsoft",
                    "Zulu",
                    "BellSoft",
                    "Amazon Corretto",
                    "Semeru",
                    "Programs/Eclipse Adoptium",
                ] {
                    roots.push((PathBuf::from(&base).join(vendor), 2));
                }
            }
        }
        for key in [
            r"HKLM\SOFTWARE\JavaSoft\JDK",
            r"HKLM\SOFTWARE\JavaSoft\Java Development Kit",
            r"HKLM\SOFTWARE\WOW6432Node\JavaSoft\JDK",
        ] {
            if let Ok(text) = capture(Command::new("reg").args(["query", key, "/s"]), stop) {
                for line in text
                    .lines()
                    .filter(|s| s.trim_start().starts_with("JavaHome"))
                {
                    if let Some((_, home)) = line.split_once("REG_SZ") {
                        roots.push((home.trim().into(), 0));
                    }
                }
            }
        }
    }
    let mut out = vec![];
    let mut seen = BTreeSet::new();
    for (root, depth) in roots {
        add_tree(&root, depth, &mut out, &mut seen);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn major_versions() {
        let id = format!("hotswap-jdk-layout-{}", std::process::id());
        let root = std::env::temp_dir().join(id);
        for (layout, version) in [
            ("temurin.jdk/Contents/Home", "21.0.10"),
            ("linux-java-17", "17.0.20"),
            ("Windows Java/jdk-25", "25.0.2"),
        ] {
            let home = root.join(layout);
            fs::create_dir_all(home.join("bin")).unwrap();
            fs::write(executable(&home, "java"), "").unwrap();
            fs::write(executable(&home, "javac"), "").unwrap();
            fs::write(
                home.join("release"),
                format!("JAVA_VERSION=\"{version}\"\n"),
            )
            .unwrap();
        }
        let mut out = vec![];
        add_tree(&root, 2, &mut out, &mut BTreeSet::new());
        let mut versions: Vec<_> = out.iter().map(|j| j.major).collect();
        versions.sort();
        assert_eq!(versions, [17, 21, 25]);
        fs::remove_dir_all(root).unwrap();
    }
}
