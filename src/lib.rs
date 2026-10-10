use zed_extension_api::{
    self as zed, DebugAdapterBinary, DebugTaskDefinition, StartDebuggingRequestArguments,
    StartDebuggingRequestArgumentsRequest, Worktree,
};

struct JavaHotSwap;
impl zed::Extension for JavaHotSwap {
    fn new() -> Self {
        Self
    }
    fn get_dap_binary(
        &mut self,
        name: String,
        config: DebugTaskDefinition,
        path: Option<String>,
        worktree: &Worktree,
    ) -> zed::Result<DebugAdapterBinary, String> {
        if name != "Java HotSwap" {
            return Err(format!("Unknown adapter: {name}"));
        }
        let command = path.or_else(|| worktree.which("java-hotswap")).ok_or_else(|| "Install the Java HotSwap companion and put it on PATH (see README), then restart Zed.".to_string())?;
        Ok(DebugAdapterBinary {
            command: Some(command),
            arguments: vec!["dap".into()],
            cwd: Some(worktree.root_path()),
            envs: vec![],
            connection: None,
            request_args: StartDebuggingRequestArguments {
                request: StartDebuggingRequestArgumentsRequest::Launch,
                configuration: config.config,
            },
        })
    }
    fn dap_request_kind(
        &mut self,
        _: String,
        config: serde_json::Value,
    ) -> zed::Result<StartDebuggingRequestArgumentsRequest, String> {
        if config.get("request").and_then(|v| v.as_str()) == Some("launch") {
            Ok(StartDebuggingRequestArgumentsRequest::Launch)
        } else {
            Err("Java HotSwap supports launch only. Use the Java adapter for JDWP attach.".into())
        }
    }
}
zed::register_extension!(JavaHotSwap);
