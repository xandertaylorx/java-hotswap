# Validation performed

Local verification on 2026-10-10, macOS Apple Silicon. These are executed results, distinct from configured CI jobs.

Toolchains: Rust 1.97.1; zed_extension_api 0.7.0; Temurin Java 17.0.20.1, 21.0.10, 25.0.2; Gradle 9.1.0; Maven 3.9.11. The embedded agent's classfile major version is 61 (Java 17).

## Nested-build detection fix

Verified with the rebuilt native arm64 release companion on Java 25 / Gradle 9.1:

- Full suite: 16 passed, one graphical test skipped (17 total). The GLFW graphical lifecycle/reload test passed separately.
- Five Rust tests passed, including four workspace-discovery regressions; rustfmt, clippy with warnings denied, and the WASM extension release build passed.
- A parent workspace with a nested `java/` build used its real Gradle 9.1 wrapper even when the global Gradle override pointed to a nonexistent executable. LWJGL 3.3.6, its native library, and Gson 2.13.2 loaded successfully. Compiled classes were under `java/build.nosync/classes/java/main`; no plain-javac output was created.
- Gradle's application main class won over a second source entry point. Native-access/default JVM arguments and additional run-task JVM arguments were verified at runtime. On macOS, `-XstartOnFirstThread` was verified in both metadata and the actual process command (the launcher consumes it before RuntimeMXBean records JVM arguments).
- Nested Gradle CLI and DAP sessions reloaded method changes with unchanged JVM PID. Tests also covered nested Maven, multiple independent Gradle/Maven roots, explicit configuration and DAP `projectRoot` selection, an unselected broken build that must not execute, and a selected broken Gradle build that must not fall back to javac.
- The installed `/Users/user/.cargo/bin/java-hotswap` was replaced with the rebuilt companion. Its `inspect --project examples/nested-gradle --project-root java` resolved the nested build, dependencies, custom outputs, application main, launcher and JVM flags correctly.

Original build files remained unchanged throughout lifecycle tests. The Minecraft clone was not read or modified. Native Zed UI testing remains unverified; the launch protocol used by Zed was exercised against real nested Gradle applications.

## Initial release verification

| Check | Result |
|---|---|
| Core suite on Java 17 | 9 passed; graphical test skipped; separate snapshot-failure recovery test passed |
| Core suite on Java 21 | 9 passed; graphical test skipped; separate snapshot-failure recovery test passed |
| Full final suite on Java 25 | 10 passed; graphical test skipped |
| LWJGL graphical test on Java 25 | Passed separately on the active macOS desktop; GLFW window creation succeeded |
| DAP framing/size-limit Rust test | Passed |
| rustfmt and clippy with warnings denied | Passed for extension and companion |
| WASM extension release build, wasm32-wasip2 | Passed |
| Native macOS arm64 release build | Passed |
| x86_64-apple-darwin release build | Passed |
| x86-64 companion under Rosetta | Plain Java, Gradle multi-module, Maven multi-module and DAP signal shutdown passed, using the installed arm64 JDK 25 |
| Relocated arm64 release executable | Plain Java, Gradle, Maven and DAP launch/disconnect passed from a directory containing spaces |
| Final arm64 and x86-64 release error recovery | Snapshot-publication failure/recovery passed on both release executables |

Each application fixture test verifies body/literal changes in a real running JVM, the same PID before/after, compiler-failure isolation, field-addition rejection, recovery after reverting the structural edit, duplicate-session refusal, Clear/Stop, child cleanup, and unchanged original build files. Single-module Gradle and Maven fixtures include an external dependency to exercise runtime dependency classpaths. Multi-module fixtures reload a class in the dependency module, not just the entry-point class.

Additional cases verify ambiguous entry points fail explicitly, saves during a slow build are not lost, a second build is cancelled by Stop, DAP SIGTERM shuts down its JVM, and an actual filesystem error during snapshot publication leaves the application running and permits recovery on a later source edit.

The final suite runs via `python3 tests/e2e.py -v`. Select a release binary with `JAVA_HOTSWAP_BINARY=/absolute/path/java-hotswap`. Select a JDK with `JAVA_HOME`; select build tools with `JAVA_HOTSWAP_GRADLE` and `JAVA_HOTSWAP_MAVEN`. Set `JAVA_HOTSWAP_TEST_LWJGL=1` only on an active graphical desktop.

Not verified: actual Intel hardware/JDK, Linux runtime, Windows runtime, live Zed UI/dev-extension installation, live Java debugger attach, registry acceptance/publication, or remote/container projects. Zed is installed locally, but native UI automation is unavailable to this task; the Rust/WASM adapter and its real DAP executable were tested separately. CI and release workflows are supplied but have not been run on GitHub.
