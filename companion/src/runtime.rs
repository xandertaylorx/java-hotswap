//! JVM classpath copies are independent of every build output and reload generation.
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub struct RuntimeSnapshot {
    directory: PathBuf,
    pub classpath: Vec<PathBuf>,
    // Same order as Project.modules[*].classes / reloads/<generation>/<index>.
    pub roots: Vec<PathBuf>,
}
impl RuntimeSnapshot {
    pub fn create(
        state: &Path,
        classpath: &[PathBuf],
        roots: &[PathBuf],
        stop: &AtomicBool,
    ) -> Result<Self, String> {
        let state = fs::canonicalize(state).map_err(|e| e.to_string())?;
        let directory = state.join("runtime");
        if directory.exists() {
            fs::remove_dir_all(&directory).map_err(|e| e.to_string())?;
        }
        fs::create_dir(&directory).map_err(|e| e.to_string())?;
        let mut result = Self {
            directory,
            classpath: vec![],
            roots: vec![],
        };
        let mut copies = BTreeMap::new();
        let mut copy = |source: &Path| -> Result<PathBuf, String> {
            // Canonicalize existing aliases so instrumentation and class loading
            // agree on CodeSource even when a build uses symlinked outputs.
            let key = if source.exists() {
                fs::canonicalize(source).map_err(|e| e.to_string())?
            } else {
                source.to_path_buf()
            };
            if let Some(path) = copies.get(&key) {
                return Ok(PathBuf::clone(path));
            }
            let target = result.directory.join(copies.len().to_string()).join(
                source
                    .file_name()
                    .unwrap_or_else(|| std::ffi::OsStr::new("classes")),
            );
            if source.exists() {
                copy_entry(source, &target, &state, &mut vec![], stop)?;
            } else {
                // Missing classpath entries are ignored by Java. An empty private
                // directory keeps them isolated if a later build creates them.
                fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            }
            copies.insert(key, target.clone());
            Ok(target)
        };
        for source in classpath {
            if source.file_name().is_some_and(|n| n == "*") {
                // Expand JVM JAR wildcards before copying; preserve all entries.
                let parent = source
                    .parent()
                    .ok_or("Classpath wildcard has no directory")?;
                if parent.exists() {
                    for entry in fs::read_dir(parent).map_err(|e| e.to_string())? {
                        let path = entry.map_err(|e| e.to_string())?.path();
                        if path
                            .extension()
                            .is_some_and(|e| e.eq_ignore_ascii_case("jar"))
                        {
                            result.classpath.push(copy(&path)?);
                        }
                    }
                }
            } else {
                result.classpath.push(copy(source)?);
            }
        }
        for source in roots {
            result.roots.push(copy(source)?);
        }
        Ok(result)
    }
}
impl Drop for RuntimeSnapshot {
    fn drop(&mut self) {
        // The owning JVM must be stopped first. Never prune this directory while
        // running; reload generation acknowledgments apply only to reloads/.
        let _ = fs::remove_dir_all(&self.directory);
    }
}
fn copy_entry(
    source: &Path,
    target: &Path,
    state: &Path,
    ancestors: &mut Vec<PathBuf>,
    stop: &AtomicBool,
) -> Result<(), String> {
    if stop.load(Ordering::SeqCst) {
        return Err("Session cancelled".into());
    }
    let resolved = fs::canonicalize(source)
        .map_err(|e| format!("Cannot snapshot {}: {e}", source.display()))?;
    if resolved == state || resolved.starts_with(state.join("runtime")) {
        return Err("Runtime classpath includes HotSwap session state; use dedicated class/resource directories".into());
    }
    let metadata = fs::metadata(&resolved).map_err(|e| e.to_string())?;
    if metadata.is_dir() {
        if ancestors.contains(&resolved) {
            return Err(format!("Classpath symlink cycle at {}", source.display()));
        }
        ancestors.push(resolved);
        fs::create_dir_all(target).map_err(|e| e.to_string())?;
        for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            copy_entry(
                &entry.path(),
                &target.join(entry.file_name()),
                state,
                ancestors,
                stop,
            )?;
        }
        ancestors.pop();
    } else if metadata.is_file() {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        // Real byte copies, never hardlinks/symlinks back into a mutable build.
        fs::copy(source, target)
            .map_err(|e| format!("Cannot snapshot {}: {e}", source.display()))?;
    } else {
        return Err(format!(
            "Unsupported classpath file type: {}",
            source.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    static ID: AtomicUsize = AtomicUsize::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "hotswap-runtime-{}-{}",
                std::process::id(),
                ID.fetch_add(1, Ordering::SeqCst)
            ));
            fs::create_dir_all(root.join(".java-hotswap")).unwrap();
            Self(root)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn snapshot_preserves_order_roots_and_bytes_when_build_outputs_change() {
        let f = Fixture::new();
        let state = f.0.join(".java-hotswap");
        let classes = f.0.join("build/classes");
        let jar = f.0.join("build/dependency.jar");
        fs::create_dir_all(&classes).unwrap();
        fs::write(classes.join("Late$1.class"), b"original class").unwrap();
        fs::write(classes.join("resource.txt"), b"resource").unwrap();
        fs::write(&jar, b"original jar").unwrap();
        let runtime = RuntimeSnapshot::create(
            &state,
            &[jar.clone(), classes.clone(), jar.clone()],
            std::slice::from_ref(&classes),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(runtime.classpath[0], runtime.classpath[2]);
        assert_eq!(runtime.classpath[1], runtime.roots[0]);
        // Overwrite in-place (would corrupt a hardlink), then delete all output.
        fs::write(&jar, b"changed jar").unwrap();
        fs::remove_dir_all(f.0.join("build")).unwrap();
        assert_eq!(fs::read(&runtime.classpath[0]).unwrap(), b"original jar");
        assert_eq!(
            fs::read(runtime.roots[0].join("Late$1.class")).unwrap(),
            b"original class"
        );
        assert_eq!(
            fs::read(runtime.roots[0].join("resource.txt")).unwrap(),
            b"resource"
        );
        drop(runtime);
        assert!(!state.join("runtime").exists());
    }

    #[test]
    fn failed_or_cancelled_copy_cleans_partial_snapshot() {
        let f = Fixture::new();
        let state = f.0.join(".java-hotswap");
        let file = f.0.join("input.jar");
        fs::write(&file, b"jar").unwrap();
        assert!(RuntimeSnapshot::create(&state, &[file], &[], &AtomicBool::new(true)).is_err());
        assert!(!state.join("runtime").exists());
        assert!(RuntimeSnapshot::create(
            &state,
            std::slice::from_ref(&state),
            &[],
            &AtomicBool::new(false)
        )
        .is_err());
        assert!(!state.join("runtime").exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_materialized_and_cycles_are_rejected() {
        use std::os::unix::fs::symlink;
        let f = Fixture::new();
        let state = f.0.join(".java-hotswap");
        let classes = f.0.join("classes");
        fs::create_dir(&classes).unwrap();
        let resource = f.0.join("external-resource");
        fs::write(&resource, b"original").unwrap();
        symlink(&resource, classes.join("resource")).unwrap();
        let runtime = RuntimeSnapshot::create(
            &state,
            std::slice::from_ref(&classes),
            &[],
            &AtomicBool::new(false),
        )
        .unwrap();
        fs::write(resource, b"changed").unwrap();
        assert!(!fs::symlink_metadata(runtime.classpath[0].join("resource"))
            .unwrap()
            .is_symlink());
        assert_eq!(
            fs::read(runtime.classpath[0].join("resource")).unwrap(),
            b"original"
        );
        drop(runtime);
        symlink(&classes, classes.join("cycle")).unwrap();
        assert!(
            RuntimeSnapshot::create(&state, &[classes], &[], &AtomicBool::new(false))
                .err()
                .unwrap()
                .contains("cycle")
        );
        assert!(!state.join("runtime").exists());
    }
}
