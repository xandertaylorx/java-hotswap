use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};
pub type Emit = Arc<dyn Fn(&str, &str) + Send + Sync>;
pub struct Managed(pub Child, Vec<thread::JoinHandle<()>>);
impl Managed {
    pub fn spawn(command: &mut Command, emit: &Emit) -> Result<Self, String> {
        command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command.spawn().map_err(|e| {
            format!(
                "Cannot start {}: {e}",
                command.get_program().to_string_lossy()
            )
        })?;
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let mut readers = vec![];
        for (reader, category) in [
            (Box::new(stdout) as Box<dyn std::io::Read + Send>, "stdout"),
            (Box::new(stderr), "stderr"),
        ] {
            let emit = emit.clone();
            readers.push(thread::spawn(move || {
                for line in BufReader::new(reader).lines() {
                    match line {
                        Ok(line) => emit(category, &format!("{line}\n")),
                        Err(_) => break,
                    }
                }
            }));
        }
        Ok(Self(child, readers))
    }
    pub fn wait(&mut self, stop: &AtomicBool) -> Result<bool, String> {
        loop {
            if stop.load(Ordering::SeqCst) {
                return Err("Session cancelled".into());
            }
            if let Some(status) = self.0.try_wait().map_err(|e| e.to_string())? {
                return Ok(status.success());
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
}
/// Capture small preflight command output while retaining cancellation and owned
/// process-group cleanup. Joining readers avoids racing the final output lines.
pub fn capture(command: &mut Command, stop: &AtomicBool) -> Result<String, String> {
    let output = Arc::new(std::sync::Mutex::new(String::new()));
    let collected = output.clone();
    let emit: Emit = Arc::new(move |_, text| {
        let mut output = collected.lock().unwrap();
        if output.len() < 1024 * 1024 {
            output.push_str(text);
        }
    });
    let mut child = Managed::spawn(command, &emit)?;
    let success = child.wait(stop)?;
    for reader in child.1.drain(..) {
        let _ = reader.join();
    }
    let text = output.lock().unwrap().clone();
    if success {
        Ok(text)
    } else {
        Err(format!(
            "Preflight command failed: {}\n{text}",
            command.get_program().to_string_lossy()
        ))
    }
}
impl Drop for Managed {
    fn drop(&mut self) {
        // Own process group: grandchildren (wrappers, JVMs) are cleaned up as well.
        #[cfg(unix)]
        unsafe {
            let group = -(self.0.id() as i32);
            libc::kill(group, libc::SIGTERM);
            // Give Java shutdown hooks up to two seconds before forcing cleanup.
            for _ in 0..40 {
                if libc::kill(group, 0) != 0 {
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }
            libc::kill(group, libc::SIGKILL);
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
pub fn execute(command: &mut Command, emit: &Emit, stop: &AtomicBool) -> Result<(), String> {
    let mut child = Managed::spawn(command, emit)?;
    if child.wait(stop)? {
        Ok(())
    } else {
        Err("Compilation or project discovery failed; see build output above".into())
    }
}
