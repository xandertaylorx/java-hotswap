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
pub struct Managed(pub Child);
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
        for (reader, category) in [
            (Box::new(stdout) as Box<dyn std::io::Read + Send>, "stdout"),
            (Box::new(stderr), "stderr"),
        ] {
            let emit = emit.clone();
            thread::spawn(move || {
                for line in BufReader::new(reader).lines() {
                    match line {
                        Ok(line) => emit(category, &format!("{line}\n")),
                        Err(_) => break,
                    }
                }
            });
        }
        Ok(Self(child))
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
