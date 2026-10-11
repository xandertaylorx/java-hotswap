use zed_extension_api::{
    self as zed, DebugAdapterBinary, DebugScenario, DebugTaskDefinition,
    StartDebuggingRequestArguments, StartDebuggingRequestArgumentsRequest, TaskTemplate, Worktree,
};

struct JavaHotSwap;
impl zed::Extension for JavaHotSwap {
    fn new() -> Self {
        Self
    }
    fn dap_locator_create_scenario(
        &mut self,
        locator_name: String,
        task: TaskTemplate,
        resolved_label: String,
        adapter: String,
    ) -> Option<DebugScenario> {
        // Automatic discovery passes the language's preferred adapter, normally
        // "Java", rather than every installed adapter. Offer our own adapter.
        if locator_name != "java-hotswap" || !matches!(adapter.as_str(), "Java" | "Java HotSwap") {
            return None;
        }
        // Java's language extension supplies this contextual task through its
        // java-main runnable query. Never execute or parse its shell command:
        // the companion owns discovery, compilation, and the instrumented JVM.
        if task.label != "Run $ZED_CUSTOM_java_class_name" {
            return None;
        }
        Some(DebugScenario {
            label: format!("Java HotSwap: {resolved_label}"),
            adapter: "Java HotSwap".into(),
            build: None,
            config: serde_json::json!({
                "request": "launch",
                "cwd": "$ZED_WORKTREE_ROOT",
                "mainSource": "$ZED_FILE",
            })
            .to_string(),
            tcp_connection: None,
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use zed::Extension;

    fn task(label: &str) -> TaskTemplate {
        TaskTemplate {
            label: label.into(),
            command: "the Java language task may contain arbitrary shell syntax".into(),
            args: vec![],
            env: vec![],
            cwd: None,
        }
    }

    #[test]
    fn java_main_registers_a_direct_hotswap_launch() {
        assert!(include_str!("../extension.toml").contains("[debug_locators.java-hotswap]"));
        let scenario = JavaHotSwap
            .dap_locator_create_scenario(
                "java-hotswap".into(),
                task("Run $ZED_CUSTOM_java_class_name"),
                "Run HotSwapTest".into(),
                "Java".into(),
            )
            .unwrap();
        assert_eq!(scenario.label, "Java HotSwap: Run HotSwapTest");
        assert_eq!(scenario.adapter, "Java HotSwap");
        assert!(scenario.build.is_none());
        let config: serde_json::Value = serde_json::from_str(&scenario.config).unwrap();
        assert_eq!(config["mainSource"], "$ZED_FILE");
        assert_eq!(config["cwd"], "$ZED_WORKTREE_ROOT");
        assert_eq!(config["request"], "launch");
    }

    #[test]
    fn locator_leaves_other_adapters_and_non_main_tasks_alone() {
        for (label, adapter, locator) in [
            ("Run $ZED_CUSTOM_java_class_name", "Python", "java-hotswap"),
            (
                "Test class $ZED_CUSTOM_java_class_name",
                "Java HotSwap",
                "java-hotswap",
            ),
            ("Run tests", "Java HotSwap", "java-hotswap"),
            ("Run $ZED_CUSTOM_java_class_name", "Java HotSwap", "other"),
        ] {
            assert!(JavaHotSwap
                .dap_locator_create_scenario(
                    locator.into(),
                    task(label),
                    "resolved".into(),
                    adapter.into()
                )
                .is_none());
        }
    }
}
