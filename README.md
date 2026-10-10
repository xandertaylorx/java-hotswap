# Java HotSwap for Zed

Save Java source, compile with the project's build tool, and redefine compatible classes in the **same running JVM**. Rust Zed extension + Rust companion + dependency-free Java instrumentation agent. MIT licensed.

This is the first working release, not a promise that every Java launch environment is supported. Standard Gradle Java/application projects, Maven projects/reactors, and conventional plain Java entry points are implemented. See [tested scope and limitations](docs/compatibility.md). There is no automatic restart.

## Install once

Prerequisites: Rust (rustup), JDK 17 or newer, and the project's Gradle/Maven wrapper or an installed build tool. Use a Gradle version compatible with your JDK; Gradle 9.1 supports running on Java 25. Existing Gradle toolchains are respected.

From this repository:

```sh
cargo install --locked --path companion
rustup target add wasm32-wasip2
cargo build --locked --release --target wasm32-wasip2
```

The agent JAR is embedded in the companion; no manual agent installation or project build-file edits are needed. Ensure `~/.cargo/bin` is on the PATH visible to Zed and restart Zed after installation. In Zed run **zed: install dev extension** and select this repository root. Keep the existing **Java** extension installed for editing, diagnostics, and Java debugging.

**Marketplace installation alone is insufficient in this release.** The extension locates `java-hotswap` on PATH; it does not download unpublished binaries. This repository is not yet published in the marketplace. [Publication checklist](docs/publishing.md).

## Use in any supported project

One time, run `java-hotswap tasks`, then use **zed: open tasks** to add the entries from [zed-tasks.json](zed-tasks.json) to your global task array. Preserve existing tasks. These tasks are available in every worktree; no per-project task file is required.

1. Open the Java project at its build root.
2. Run **task: spawn → Java HotSwap: Run**.
3. Change `return 0;` to `return 10;` in an existing method and save.
4. Watch `COMPILED`, `RELOADED`, and the changed application output in the task terminal.

Use **Java HotSwap: Stop**, Ctrl-C in the Run terminal, or close the debugger session to stop. **Clear status** clears the active task terminal using ANSI terminal control. It does not erase the debugger's output history; use Zed's output controls for that. **Inspect project** shows the build system, directories, toolchain, and entry-point candidates.

JVM redefinition updates method code. Changing a field initializer does not update values already stored in objects or rerun initialization. Changes to literals or assignments inside methods take effect when those methods execute again.

Compilation failures are printed with the build tool's file/line diagnostics. Existing JDTLS supplies editor diagnostics. This extension does not add a second Java language server or a separate Problems provider.

For native launch/output UI, add a scenario to your project's `.zed/debug.json` array:

```json
{
  "label": "Run with Java HotSwap",
  "adapter": "Java HotSwap",
  "request": "launch",
  "cwd": "$ZED_WORKTREE_ROOT"
}
```

Then use **debugger: start**. This adapter manages launching, output, and stopping; it reports breakpoints as unverified. For stepping and breakpoints, use the existing Java adapter with JDWP attach as described below. Automatic scenario generation from Java gutter runnables is not implemented.

## Optional project configuration

A single conventional `public static void main(String[] args)` is detected automatically. Gradle's `application.mainClass` takes precedence within its module. Multiple applications produce a clear selection error rather than launching an arbitrary entry point. Create `.java-hotswap.json` only when needed:

```json
{
  "mainClass": "demo.Main",
  "module": ":app",
  "jvmArgs": ["-XstartOnFirstThread"],
  "args": [],
  "debounceMs": 250
}
```

`module` is a Gradle project path or an absolute Maven module directory. Use **Inspect project** for its exact value. For multiple modules, set both `module` and `mainClass` when overriding detection. Optional `javaHome` chooses a JDK (relative paths resolve from the project); otherwise Gradle's selected launcher or `JAVA_HOME`/PATH is used. `buildArgs` passes build-tool options, e.g. Maven profiles. `classpath` adds directories/JARs for plain Java. Unknown configuration fields fail explicitly. Build configuration changes require stopping and starting the session to rediscover the project.

Gradle application default JVM arguments are inherited. The LWJGL example includes `-XstartOnFirstThread` on macOS. Add that argument yourself when a custom Java/application setup doesn't declare it.

To enable ordinary Java debugging, add `"jdwpPort": 5005` to this config, start HotSwap, then launch an existing Java extension attach scenario:

```json
{
  "label": "Attach Java debugger",
  "adapter": "Java",
  "request": "attach",
  "hostName": "127.0.0.1",
  "port": 5005
}
```

JDWP binds to loopback. Java attach is an integration path, not a debugger reimplementation; live Zed attach has not been verified here. Avoid another tool simultaneously redefining the same classes.

## Build and test

```sh
cargo fmt --check
cargo fmt --manifest-path companion/Cargo.toml --check
cargo clippy --all-targets -- -D warnings
cargo clippy --manifest-path companion/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path companion/Cargo.toml
cargo build --manifest-path companion/Cargo.toml
python3 tests/e2e.py -v
cargo build --locked --release --target wasm32-wasip2
```

E2E tests require Gradle and Maven on PATH, or `JAVA_HOTSWAP_GRADLE` / `JAVA_HOTSWAP_MAVEN` absolute paths. They copy fixtures to paths containing spaces, start real JVMs, edit sources, verify changed output **and unchanged PID**, reject structural changes, recover from invalid source, check duplicate sessions, and verify stop cleanup. DAP tests exercise actual framed stdin/stdout and SIGTERM cleanup. Another test saves during an active build and cancels a subsequent build. CI defines macOS/Linux and Java 17/21/25 jobs; workflow presence alone does not establish passing results. Local results are in [validation.md](docs/validation.md).

Examples: [plain](examples/plain), [Gradle](examples/gradle), [Maven](examples/maven), [Gradle multi-module](examples/gradle-multi), [Maven multi-module](examples/maven-multi), [LWJGL](examples/lwjgl).

To test the native GLFW window on an active desktop, run `JAVA_HOTSWAP_TEST_LWJGL=1 python3 tests/e2e.py HotSwap.test_lwjgl -v`. Headless CI skips that graphical test.

Read [architecture](docs/architecture.md), [API research](docs/api-research.md), and [contributing](CONTRIBUTING.md). Add `.java-hotswap/` to your own project's ignore rules if you want generated session data hidden from Git; the tool never edits your ignore file.
