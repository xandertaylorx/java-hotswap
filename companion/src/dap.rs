use crate::{process::Emit, session, Config};
use serde_json::{json, Value};
use std::{
    io::{self, BufRead, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
};
struct Writer {
    seq: u64,
}
impl Writer {
    fn send(&mut self, mut message: Value) {
        self.seq += 1;
        message["seq"] = json!(self.seq);
        let body = serde_json::to_vec(&message).unwrap();
        let mut out = io::stdout().lock();
        let _ = write!(out, "Content-Length: {}\r\n\r\n", body.len());
        let _ = out.write_all(&body);
        let _ = out.flush();
    }
    fn response(&mut self, request: &Value, success: bool, body: Value, message: &str) {
        self.send(json!({"type":"response", "request_seq":request["seq"], "command":request["command"], "success":success, "body":body, "message":message}));
    }
    fn event(&mut self, name: &str, body: Value) {
        self.send(json!({"type":"event","event":name,"body":body}));
    }
}
fn read_message(reader: &mut impl BufRead) -> Result<Option<Value>, String> {
    let mut length = None;
    let mut headers = 0;
    loop {
        let mut line = String::new();
        let read = std::io::Read::take(&mut *reader, (8193 - headers) as u64)
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        if read == 0 {
            return Ok(None);
        }
        headers += read;
        if headers > 8192 {
            return Err("DAP headers too large".into());
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            if key.eq_ignore_ascii_case("Content-Length") {
                length = Some(value.trim().parse::<usize>().map_err(|e| e.to_string())?);
            }
        }
    }
    let length = length.ok_or("Missing Content-Length")?;
    if length > 1024 * 1024 {
        return Err("DAP message too large".into());
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes).map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| e.to_string())
}
pub fn serve() -> Result<(), String> {
    let writer = Arc::new(Mutex::new(Writer { seq: 0 }));
    let stop = Arc::new(AtomicBool::new(false));
    let mut worker = None;
    let mut launched = false;
    let signal = stop.clone();
    ctrlc::set_handler(move || signal.store(true, Ordering::SeqCst)).map_err(|e| e.to_string())?;
    let (sender, receiver) = std::sync::mpsc::sync_channel(16);
    thread::spawn(move || {
        let mut reader = io::BufReader::new(io::stdin());
        loop {
            let message = read_message(&mut reader);
            let finished = !matches!(message, Ok(Some(_)));
            if sender.send(message).is_err() || finished {
                break;
            }
        }
    });
    let result = (|| {
        loop {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            let request = match receiver.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(Ok(Some(request))) => request,
                Ok(Ok(None)) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                Ok(Err(error)) => return Err(error),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            };
            let command = request["command"].as_str().unwrap_or("");
            match command {
                "initialize" => {
                    writer.lock().unwrap().response(&request, true, json!({"supportsTerminateRequest":true,"supportsConfigurationDoneRequest":true}), "");
                    writer.lock().unwrap().event("initialized", json!({}));
                }
                "launch" if !launched => {
                    let mut args = request["arguments"].clone();
                    let root = args.get("cwd").and_then(Value::as_str).unwrap_or(".");
                    let base = Config::load(root.into());
                    for key in ["adapter", "request", "label", "noDebug", "__restart"] { if let Some(o) = args.as_object_mut() { o.remove(key); } }
                    let config = base.and_then(|base| {
                        let mut merged = serde_json::to_value(base).unwrap();
                        for (k, v) in args.as_object().ok_or("Launch arguments must be an object")? { merged[k] = v.clone(); }
                        serde_json::from_value::<Config>(merged).map_err(|e| e.to_string())
                    });
                    match config {
                        Err(e) => writer.lock().unwrap().response(&request, false, json!({}), &e),
                        Ok(config) => {
                            launched = true;
                            let output = writer.clone(); let session_stop = stop.clone();
                            writer.lock().unwrap().response(&request, true, json!({}), "");
                            worker = Some(thread::spawn(move || {
                                let event_writer = output.clone();
                                let emit: Emit = Arc::new(move |category, text| event_writer.lock().unwrap().event("output", json!({"category":category,"output":text})));
                                let result = session::run(config, session_stop, emit);
                                if let Err(e) = &result { output.lock().unwrap().event("output", json!({"category":"stderr","output":format!("[Java HotSwap] ERROR {e}\n")})); }
                                output.lock().unwrap().event("exited", json!({"exitCode":if result.is_ok() {0} else {1}}));
                                output.lock().unwrap().event("terminated", json!({}));
                            }));
                        }
                    }
                }
                "disconnect" | "terminate" => { stop.store(true, Ordering::SeqCst); writer.lock().unwrap().response(&request, true, json!({}), ""); break; }
                "configurationDone" => writer.lock().unwrap().response(&request, true, json!({}), ""),
                "threads" => writer.lock().unwrap().response(&request, true, json!({"threads":[]}), ""),
                "setBreakpoints" => { let count = request["arguments"]["breakpoints"].as_array().map_or(0, Vec::len); writer.lock().unwrap().response(&request, true, json!({"breakpoints":(0..count).map(|_| json!({"verified":false,"message":"Use the Java adapter with JDWP attach for breakpoints"})).collect::<Vec<_>>()}), ""); }
                "setExceptionBreakpoints" => writer.lock().unwrap().response(&request, true, json!({"breakpoints":[]}), ""),
                _ => writer.lock().unwrap().response(&request, false, json!({}), "Unsupported request: Java HotSwap is a launch/output adapter; use Java attach for debugging"),
            }
        }
        Ok(())
    })();
    stop.store(true, Ordering::SeqCst);
    if let Some(worker) = worker {
        let _ = worker.join();
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framing_and_limits() {
        let body = r#"{"command":"initialize","seq":1}"#;
        let wire = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        assert_eq!(
            read_message(&mut io::Cursor::new(wire)).unwrap().unwrap()["seq"],
            1
        );
        assert!(read_message(&mut io::Cursor::new("Content-Length: 999999999\r\n\r\n")).is_err());
    }
}
