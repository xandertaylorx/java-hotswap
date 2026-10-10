# Architecture

The WASM extension is only the supported Zed adapter registration/lookup layer. The native companion owns all long-lived OS processes. The JAR is built with `javac --release 17` and embedded at Rust compile time, making installed companions relocatable.

- `src/lib.rs`: SDK binary discovery and launch request declaration.
- `companion/src/project.rs`: Gradle/Maven/plain discovery, classpaths, entry detection, compilation, source snapshots.
- `companion/src/gradle_runtime.rs`, `jdks.rs`, `properties.rs`: wrapper/version preflight, Gradle runtime compatibility, platform JDK discovery, Java properties parsing.
- `companion/src/session.rs`: exclusive project session lock, debounce, serial build scheduling, JVM lifecycle, build commits.

Workspace discovery selects build roots before source scanning. It walks subdirectories until a Gradle or Maven root is found, then treats that build's module tree as owned by the build tool. A single root is automatic; multiple independent roots require `projectRoot` (launch/config) or `--project-root` (CLI). Explicit invalid selections, conflicting build markers, unreadable traversal paths and failed builds are errors, never triggers for plain javac fallback. Session state and control files remain under `cwd/.java-hotswap`; the build tool and JVM run from the selected build root. Gradle supplies actual SourceSet output/runtime paths, application entry point, application/default and run-task JVM arguments, and the run-task Java launcher.
- `companion/src/process.rs`: stdout/stderr forwarding, cancellation, owned process groups.
- `companion/src/dap.rs`: framed JSON requests, output/events, terminate and disconnect.
- `agent/src/dev/javahotswap/Agent.java`: hashes, project-origin checks, class redefinition and error reporting.

## Build/reload transaction

Initial build completes before the JVM starts. Plain javac builds use a staging directory, so a failed plain compilation cannot replace existing output; successful builds also remove stale class files. The agent records SHA-256 hashes of project class outputs at startup. Saving source changes the companion's content snapshot; rapid changes settle for 250ms by default. One build runs at a time. The pre-build snapshot remains the baseline so saves during compilation schedule another compilation.

A successful build copies project `.class` outputs into an immutable generation directory. The companion atomically replaces the commit file only after copying finishes. The daemon agent sees that commit and compares the generation's hashes with its previous successful-build snapshot. Reload bytes come from that immutable generation, so later compiler writes cannot corrupt a reload. Generations acknowledged by the agent are pruned on subsequent successful builds; session startup clears previous generations.

Loaded classes must have a file CodeSource exactly matching a discovered project classes directory. Classes in dependency JARs, agents, test outputs, module descriptors, primitives, and arrays are excluded. Gradle/Maven project JAR dependencies are replaced with their classes directories in the launch classpath. An atomic `redefineClasses` batch updates all compatible changed loaded classes together. A structural rejection leaves the previous definitions in effect and prints the names and JVM reason. An unchanged build does not repeat that rejection. Source watch errors and reload-snapshot write errors are reported while leaving the application running. Reverting structural changes allows a subsequent compatible reload.

New unloaded classes are left to ordinary class loading. Existing instances retain their fields. Static initializers are not rerun. Existing active method frames complete with their old bytecode. Deleted loaded classes cannot be unloaded by this mechanism. Startup agent and watcher errors are caught and reported without terminating the Java application.

## Lifecycle

Only one session/discovery may own a project (OS file lock). Stop/Clear use local sentinel files rather than trusting a reusable PID. Stop works during build discovery/compilation too. Ctrl-C, SIGTERM and SIGHUP are handled in Run sessions and the DAP adapter. EOF/disconnect also shuts down the adapter session. On Unix, child processes have an owned process group, receive SIGTERM, then SIGKILL after a bounded grace period. Existing shared Gradle daemons are owned by Gradle and are not globally stopped. The Windows implementation kills the direct child; Windows Job Objects remain necessary for equivalent descendant cleanup.

No telemetry, network control server, or automatic restart. Build tools may download dependencies and execute normal project build code. DAP uses stdin/stdout; runtime log messages never contaminate protocol stdout. Frame limits reject oversized input. Filesystem paths are passed as process arguments rather than shell command interpolation.
