use std::{env, fs, path::PathBuf, process::Command};
fn main() {
    println!("cargo:rerun-if-changed=../agent/src");
    println!("cargo:rerun-if-env-changed=JAVA_HOME");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let classes = out.join("agent-classes");
    fs::create_dir_all(&classes).unwrap();
    let bin = |name: &str| {
        env::var_os("JAVA_HOME")
            .map(|h| {
                PathBuf::from(h).join("bin").join(if cfg!(windows) {
                    format!("{name}.exe")
                } else {
                    name.into()
                })
            })
            .unwrap_or_else(|| name.into())
    };
    let source = "../agent/src/dev/javahotswap/Agent.java";
    assert!(Command::new(bin("javac"))
        .args(["--release", "17", "-d"])
        .arg(&classes)
        .arg(source)
        .status()
        .expect("JDK 17+ required to build agent")
        .success());
    let manifest = out.join("MANIFEST.MF");
    fs::write(&manifest, "Manifest-Version: 1.0\nPremain-Class: dev.javahotswap.Agent\nCan-Redefine-Classes: true\n\n").unwrap();
    assert!(Command::new(bin("jar"))
        .arg("--create")
        .arg("--file")
        .arg(out.join("java-hotswap-agent.jar"))
        .arg("--manifest")
        .arg(manifest)
        .arg("-C")
        .arg(classes)
        .arg(".")
        .status()
        .unwrap()
        .success());
}
