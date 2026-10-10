# Official API findings

Reviewed 2026-10-10 before implementation. SDK resolved from crates.io: `zed_extension_api` 0.7.0, pinned exactly in Cargo.toml and Cargo.lock. The extension compiles for `wasm32-wasip2`.

| Requirement | Available supported mechanism | This release |
|---|---|---|
| Extension installation | extension.toml + Rust WASM extension | Separate Java HotSwap debugger adapter |
| Start and stop | Debugger adapter binary and DAP launch/terminate/disconnect | Native launch/output session |
| One action across projects | Global tasks with `$ZED_WORKTREE_ROOT` | Run task after one-time task installation |
| Save listener | No save callback in Extension trait; documented task hook is worktree creation | Companion watches filesystem saves |
| Custom editor buttons/status | No general UI/action/status-bar registration in SDK | Task terminal or debugger output |
| Compilation errors | Build output; existing Java JDTLS diagnostics | Retains Java extension |
| Breakpoints/stepping | Java extension's separate Java adapter | Optional JDWP attach; HotSwap adapter has no stepping |
| Auto-installed native executable | DAP callback can locate/download binaries | PATH lookup now; no fictitious release downloads |
| Clear output | Terminal ANSI control; editor-owned debugger output | Clear task terminal; debugger history managed by Zed |

Sources:

- [Developing extensions](https://zed.dev/docs/extensions/developing-extensions): extension features, manifest, Rust registration, WASI target.
- [Extension trait source](https://github.com/zed-industries/zed/blob/main/crates/extension_api/src/extension_api.rs): callbacks examined, including get_dap_binary and dap_request_kind. Main is mutable upstream; the exact compiled contract is the pinned crate.
- [Debugger extensions](https://zed.dev/docs/extensions/debugger-extensions): adapter schema mandatory, binary callback, request kind, optional locators/scenario conversion.
- [Tasks](https://zed.dev/docs/tasks): global tasks, worktree variables, concurrency, terminal output. No documented save task hook.
- [Existing Java extension](https://github.com/zed-extensions/java): JDTLS integration and Java debugger. Examined README and src/java.rs. Java debugger delegates to JDTLS; it is not a generic executable that this extension can safely wrap.
- [Zed Java](https://zed.dev/docs/languages/java): attach configuration.
- [DAP specification](https://microsoft.github.io/debug-adapter-protocol/specification): Content-Length framing and launch/session events.
- [Gradle init scripts](https://docs.gradle.org/current/userguide/init_scripts.html): external build introspection without modifying build scripts.
- [Maven effective POM](https://maven.apache.org/plugins/maven-help-plugin/effective-pom-mojo.html): resolved build model and active-profile modules.
- [Maven dependency classpath](https://maven.apache.org/plugins/maven-dependency-plugin/build-classpath-mojo.html): runtime dependency resolution.
- [Instrumentation Java 25](https://docs.oracle.com/en/java/javase/25/docs/api/java.instrument/java/lang/instrument/Instrumentation.html): redefineClasses restrictions and atomic batch behavior.

The architecture deliberately uses filesystem observation outside WASM. No invented custom Zed command, save callback, diagnostics API, or UI registration is used.
