<p align="center">
  <img src="assets/readme-banner.svg" alt="Java HotSwap for Zed — change the code, keep the JVM running" width="100%">
</p>

<p align="center">
  <a href="https://github.com/xandertaylorx/java-hotswap/actions/workflows/ci.yml"><img src="https://github.com/xandertaylorx/java-hotswap/actions/workflows/ci.yml/badge.svg" alt="Verification workflow status"></a>
  <a href="docs/validation.md"><img src="https://img.shields.io/badge/Java-17%20%7C%2021%20%7C%2025-244e45" alt="Java 17, 21 and 25"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-d6a478" alt="MIT license"></a>
  <img src="https://img.shields.io/badge/version-0.1.0-52635b" alt="Version 0.1.0">
</p>

<p align="center">
  <a href="#quick-start"><strong>Quick start</strong></a> ·
  <a href="#examples">Examples</a> ·
  <a href="docs/architecture.md">Architecture</a> ·
  <a href="docs/validation.md">Test results</a> ·
  <a href="CONTRIBUTING.md">Contributing</a>
</p>

# Java HotSwap for Zed

**Edit Java. Save. See compatible changes in the same running JVM.**

Java HotSwap connects Zed to your project's compiler and a small Java instrumentation agent. It detects supported projects, compiles saved Java changes, and automatically reloads compatible classes. Your application keeps running, and the existing Java extension continues to handle editing and diagnostics.

> [!NOTE]
> **First working release.** A one-time companion installation is required. Load the Zed extension as a dev extension; marketplace publication is pending. [Supported scope and limitations →](docs/compatibility.md)

## See the idea

Change an existing method in your application:

```diff
 static int explosionRadius() {
-    return 0;
+    return 10;
 }
```

Save the file. After compilation succeeds, the next invocation uses the new code. The console examples demonstrate this behavior:

```text
Before save:  radius=0  pid=4242
After reload: radius=10 pid=4242
```

*Illustrative output: the value changes; the JVM PID stays the same.*

## What you get

| | Behavior |
|---|---|
| **Project discovery** | Detects Gradle, Maven, and conventional plain Java projects, source directories, class outputs, and entry points. |
| **Automatic compilation** | Debounces saves and runs one build at a time. Gradle and Maven use their existing build systems. |
| **Compatible class reloads** | Reloads method bodies and logic using `Instrumentation.redefineClasses`. |
| **Failure recovery** | Compilation errors, rejected structural changes, and reload-snapshot failures leave the application running. |
| **Zed integration** | Run, Stop, Clear, and Inspect tasks, plus a native launch/output adapter. |
| **Build files preserved** | Uses temporary discovery/configuration files; no edits to your Gradle or Maven build files. |

There is **no automatic restart**. Structural changes are reported explicitly so you can decide when to restart.

## Quick start

### 1. Install the companion and extension

You need **Rust via rustup**, **JDK 17+**, and your project's Gradle/Maven wrapper or an installed build tool. Use a build-tool version compatible with your JDK; Gradle 9.1 supports running on Java 25. Existing Gradle toolchains are respected.

```sh
git clone https://github.com/xandertaylorx/java-hotswap.git
cd java-hotswap

cargo install --locked --path companion
rustup target add wasm32-wasip2
cargo build --locked --release --target wasm32-wasip2
```

The agent JAR is embedded in the companion, so there is no separate agent installation.

In Zed:

1. Ensure `~/.cargo/bin` is on the PATH visible to Zed, then restart Zed.
2. Run **zed: install dev extension** and select the cloned repository.
3. Keep the existing **Java** extension installed for language support and Java debugging.

### 2. Add global tasks once

```sh
java-hotswap tasks
```

Use **zed: open tasks** and add the entries from [zed-tasks.json](zed-tasks.json) to your global task array, preserving any existing tasks. They become available across your worktrees; no per-project task file is required.

### 3. Run, edit, save

1. Open a supported Java project at its build root.
2. Choose **task: spawn → Java HotSwap: Run**.
3. Edit an existing method and save.
4. Watch the compilation output and `RELOADED` messages in the task terminal.

| Task | Purpose |
|---|---|
| **Java HotSwap: Run** | Builds, starts the application with the agent, and watches Java sources. |
| **Java HotSwap: Stop** | Stops the session, including an active compilation. Ctrl-C in the Run terminal also stops it. |
| **Java HotSwap: Clear status** | Clears the active task terminal with ANSI terminal control. |
| **Java HotSwap: Inspect project** | Shows discovered directories, toolchains, modules, and entry-point candidates. |

Compilation errors retain the build tool's file/line diagnostics. Existing JDTLS provides editor diagnostics. Clear does not erase debugger output history; use Zed's output controls for that.

## Project support

| Project | Launch and compilation |
|---|---|
| **Gradle** | Java/application projects and multi-module builds. Reads main source sets, runtime classpaths, application settings, and the Java launcher. Uses `classes` tasks on saves. |
| **Maven** | Standard projects and reactors. Reads the effective POM and runtime dependencies. Performs an initial `package -DskipTests`, then uses `compile` on saves. |
| **Plain Java** | Conventional source layouts with a detected main method and optional extra classpath. Compiles all sources safely with `javac`. |
| **LWJGL** | Working Gradle example; inherits `-XstartOnFirstThread` on macOS. |

Java **17, 21, and 25** have been exercised on macOS Apple Silicon. An x86-64 companion was also tested under Rosetta. Linux has CI jobs configured; Windows remains experimental. Actual Intel hardware, Linux/Windows runtimes, and live Zed UI integration have not been verified locally. [Full validation record →](docs/validation.md)

> [!IMPORTANT]
> Standard JVM HotSwap rules apply. Changing method code does not rerun initializers or update values already stored in objects. Field/method additions, inheritance changes, and other structural edits may require a manual restart. Custom launchers, Android, module-path applications, and composite Gradle builds are outside automatic support in this release. [Compatibility details →](docs/compatibility.md)

## Examples

Each console example prints an explosion radius and its JVM PID, making reloads easy to observe.

| Example | What it demonstrates |
|---|---|
| [Plain Java](examples/plain) | A minimal project with no build tool. |
| [Gradle](examples/gradle) | Application detection and an external runtime dependency. |
| [Maven](examples/maven) | POM discovery and an external runtime dependency. |
| [Gradle multi-module](examples/gradle-multi) | Reloading a class in a dependency module. |
| [Maven multi-module](examples/maven-multi) | Reactor dependencies launched from class directories. |
| [LWJGL](examples/lwjgl) | A GLFW window whose title changes after a method reload. |
| [Nested Gradle](examples/nested-gradle) | Open the parent folder; the `java/` build uses its wrapper, LWJGL, Gson, and `build.nosync`. |

For a first try, open `examples/plain` in Zed and run **Java HotSwap: Run**. Change `return 0;` to `return 10;`, then save.

## Configuration and debugging

A single conventional `public static void main(String[] args)` is detected automatically. Gradle's `application.mainClass` takes precedence within its module. Multiple candidates produce a selection error with the discovered entry points.

### Workspaces with nested Java builds

Open your workspace normally. Discovery searches subdirectories for Gradle (Groovy/Kotlin) and Maven builds. A single build is selected automatically, so `minecraftclone/java/build.gradle` works with Zed opened at `minecraftclone`. The selected root and build executable appear in the output. Gradle's wrapper compiles the sources; Gradle supplies the resolved runtime classpath, actual output directories (including custom `build.nosync`), application main class, and JVM arguments. Build failures are reported without falling back to bare `javac`.

If the workspace contains several independent builds, discovery lists them and requires a selection before executing a build. Set `projectRoot` in each Zed launch scenario:

```json
[
  {
    "label": "Game HotSwap",
    "adapter": "Java HotSwap",
    "request": "launch",
    "cwd": "$ZED_WORKTREE_ROOT",
    "projectRoot": "java"
  }
]
```

The CLI equivalent is `java-hotswap run --project /path/to/workspace --project-root java`. You can also set `projectRoot` in the workspace's `.java-hotswap.json`. Gradle subprojects and Maven reactor modules belong to their enclosing build; use `module` to select an application within that build. To run separate sessions concurrently, set each scenario's `cwd` to its own build root so session locks and Stop/Clear controls are independent.

Discovery skips symlinks and generated/cache directories such as `.git`, `.gradle`, `.java-hotswap`, `build`, `build.nosync`, `target`, and `node_modules`. Use an explicit `projectRoot` for a build in an excluded directory. Plain Java compilation is used only when no build is found; an invalid explicit build selection produces an error.

<details>
<summary><strong>Choose an entry point or customize JVM arguments</strong></summary>

Create `.java-hotswap.json` in the workspace (`cwd`) only when needed:

```json
{
  "mainClass": "demo.Main",
  "module": ":app",
  "jvmArgs": [],
  "args": [],
  "debounceMs": 250
}
```

| Option | Use |
|---|---|
| `projectRoot` | Gradle/Maven build root, absolute or relative to the workspace. Required when several independent builds are discovered. |
| `mainClass` | Fully qualified entry point. |
| `module` | Gradle project path or absolute Maven module directory; Inspect shows the exact value. |
| `javaHome` | Select a JDK; relative paths resolve from the workspace. Otherwise uses Gradle's application launcher or `JAVA_HOME`/PATH. |
| `jvmArgs` | Additional JVM arguments, such as `-XstartOnFirstThread` for a macOS application that needs it. |
| `args` | Application arguments. |
| `buildArgs` | Build-tool options, such as Maven profiles. |
| `classpath` | Additional directories/JARs for plain Java. |
| `debounceMs` | Save debounce interval; defaults to 250 ms. |
| `jdwpPort` | Optional loopback debugger port for Java attach. |

For multi-module overrides, set both `module` and `mainClass`. Gradle application default JVM arguments and additional `run.jvmArgs` are inherited (duplicate identical arguments are removed); the LWJGL example already declares the macOS first-thread argument.

Unknown fields fail explicitly. Stop and start the session after changing build configuration so the project is rediscovered.

</details>

<details>
<summary><strong>Use Zed's native launch/output UI</strong></summary>

Add a scenario to your project's `.zed/debug.json` array:

```json
{
  "label": "Run with Java HotSwap",
  "adapter": "Java HotSwap",
  "request": "launch",
  "cwd": "$ZED_WORKTREE_ROOT"
}
```

Choose **debugger: start**. Closing the session stops the application.

This adapter manages launching, output, and stopping. It reports breakpoints as unverified; use the existing Java adapter for stepping and breakpoints. Automatic Java gutter scenario generation is not implemented, and live Zed UI testing remains outstanding.

</details>

<details>
<summary><strong>Attach the existing Java debugger</strong></summary>

Add `"jdwpPort": 5005` to `.java-hotswap.json`, start HotSwap, and launch an existing Java extension attach scenario:

```json
{
  "label": "Attach Java debugger",
  "adapter": "Java",
  "request": "attach",
  "hostName": "127.0.0.1",
  "port": 5005
}
```

JDWP binds to loopback. Live Zed Java attach has not been verified here. Avoid another tool simultaneously redefining the same classes.

</details>

## Development

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

The end-to-end tests start real JVMs, edit real source files, and assert changed output **with the same PID**. They also cover compilation failures, structural-change rejection, recovery, duplicate sessions, saves during compilation, cancellation, DAP shutdown, and snapshot-publication errors.

Gradle and Maven must be on PATH, or supplied through `JAVA_HOTSWAP_GRADLE` and `JAVA_HOTSWAP_MAVEN` as absolute paths. CI defines macOS/Linux × Java 17/21/25 jobs; see the workflow badge for current GitHub results and [validation.md](docs/validation.md) for executed local results.

Run the graphical test on an active desktop:

```sh
JAVA_HOTSWAP_TEST_LWJGL=1 python3 tests/e2e.py HotSwap.test_lwjgl -v
```

Headless CI skips that test. Add `.java-hotswap/` to your own project's ignore rules to hide generated session data; the tool never edits your ignore file.

## Documentation

| Guide | Contents |
|---|---|
| [Architecture](docs/architecture.md) | Project discovery, compilation, immutable reload snapshots, and process ownership. |
| [API research](docs/api-research.md) | Official Zed capabilities and the supported integration choices. |
| [Compatibility](docs/compatibility.md) | Build-system, JVM, and platform boundaries. |
| [Validation](docs/validation.md) | Executed checks and unverified environments. |
| [Publication](docs/publishing.md) | Marketplace prerequisites and release workflow. |
| [Contributing](CONTRIBUTING.md) | Development expectations and testing new integrations. |

---

**MIT licensed.** See [LICENSE](LICENSE). Contributions and reproducible bug reports are welcome through [GitHub issues](https://github.com/xandertaylorx/java-hotswap/issues).
