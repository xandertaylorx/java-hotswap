# Architecture

The WASM extension is only the supported Zed adapter registration/lookup layer. The native companion owns all long-lived OS processes. The JAR is built with `javac --release 17` and embedded at Rust compile time, making installed companions relocatable.

- `src/lib.rs`: SDK binary discovery and launch request declaration.
- `companion/src/project.rs`: Gradle/Maven/plain discovery, classpaths, entry detection, compilation, source snapshots.
- `companion/src/gradle_runtime.rs`, `jdks.rs`, `properties.rs`: wrapper/version preflight, Gradle runtime compatibility, platform JDK discovery, Java properties parsing.
- `companion/src/runtime.rs`: independent launch classpath copies, origin mapping, cancellation and lifetime cleanup.
- `companion/src/session.rs`: exclusive project session lock, debounce, serial build scheduling, JVM lifecycle, build commits.

Workspace discovery selects build roots before source scanning. It walks subdirectories until a Gradle or Maven root is found, then treats that build's module tree as owned by the build tool. A single root is automatic; multiple independent roots require `projectRoot` (launch/config) or `--project-root` (CLI). Explicit invalid selections, conflicting build markers, unreadable traversal paths and failed builds are errors, never triggers for plain javac fallback. Session state and control files remain under `cwd/.java-hotswap`; the build tool runs from the selected build root. Gradle supplies actual SourceSet output/runtime paths and JavaExec task descriptors. The JVM uses the selected task’s working directory.
- `companion/src/process.rs`: stdout/stderr forwarding, cancellation, owned process groups.
- `companion/src/dap.rs`: framed JSON requests, output/events, terminate and disconnect.
- `agent/src/dev/javahotswap/Agent.java`: hashes, project-origin checks, class redefinition and error reporting.

## Gradle launch capture

After compilation, a temporary init script runs the selected JavaExec task’s dependencies and preceding preparation actions. A standard JavaExec probe identifies the built-in execution action through public Task.actions; capture replaces only that action. Unexpected action layouts and subclasses fail explicitly. Capture stops the remaining task actions and removes its finalizers rather than running an uninstrumented JVM or premature cleanup.

The capture includes late-bound application/JVM argument providers, structured fork options and system properties, the Java launcher, runtime classpath, working directory and environment. Project JARs map to class outputs for instrumentation. The companion appends explicit user overrides and the embedded instrumentation agent. An ephemeral launch.json is created with mode 0600 on Unix (inherited directory ACLs on Windows), parsed without logging its contents, then deleted on success or failure. Persistent gradle.json contains task descriptors, not arguments/environment/property values. Post-actions/finalizers are not replayed at shutdown.

## Build/reload transaction

Initial build and run-task capture complete before the JVM starts. Every resolved classpath directory and file (including dependency JARs and resources) is byte-copied into a private runtime directory. Copies preserve classpath order; canonical aliases share a destination. Symlinks are materialized rather than retaining references to build files, and cycles are rejected. Project class roots map to these runtime origins in the same index order used by reload generations. No rebuild writes to the runtime copy. Its owner is declared before the JVM process guard, so shutdown stops/waits for Java before deleting runtime files. Copy failure or cancellation cleans up without starting Java. Plain javac builds use a staging directory, so a failed plain compilation cannot replace existing output; successful builds also remove stale class files. The agent records SHA-256 hashes of project class outputs at startup. Saving source changes the companion's content snapshot; rapid changes settle for 250ms by default. One build runs at a time. The pre-build snapshot remains the baseline so saves during compilation schedule another compilation.

A successful build copies project `.class` outputs into an immutable generation directory. The companion atomically replaces the commit file only after copying finishes. The daemon agent sees that commit and compares the generation's hashes with its previous successful-build snapshot. Reload bytes come from that immutable generation, so later compiler writes cannot corrupt a reload. Generations acknowledged by the agent are pruned on subsequent successful builds; session startup clears previous generations.

Loaded classes must have a file CodeSource exactly matching a mapped private project runtime directory. Classes in dependency JARs, agents, test outputs, module descriptors, primitives, and arrays are excluded. Gradle/Maven project JAR dependencies are replaced with their classes directories in the launch classpath. An atomic `redefineClasses` batch updates all compatible changed loaded classes together. A structural rejection leaves the previous definitions in effect and prints the names and JVM reason. An unchanged build does not repeat that rejection. Source watch errors and reload-snapshot write errors are reported while leaving the application running. Reverting structural changes allows a subsequent compatible reload.

Classes first loaded later are served by the launch-time runtime copy. Newly added classes and updated resources/dependencies require a restart; only already loaded project classes receive method redefinitions. Existing instances retain their fields. Static initializers are not rerun. Existing active method frames complete with their old bytecode. Deleted loaded classes cannot be unloaded by this mechanism. Startup agent and watcher errors are caught and reported without terminating the Java application.

## Lifecycle

Only one session/discovery may own a project (OS file lock). Stop/Clear use local sentinel files rather than trusting a reusable PID. Stop works during build discovery/compilation too. Ctrl-C, SIGTERM and SIGHUP are handled in Run sessions and the DAP adapter. EOF/disconnect also shuts down the adapter session. On Unix, child processes have an owned process group, receive SIGTERM, then SIGKILL after a bounded grace period. Existing shared Gradle daemons are owned by Gradle and are not globally stopped. The Windows implementation kills the direct child; Windows Job Objects remain necessary for equivalent descendant cleanup.

No telemetry, network control server, or automatic restart. Build tools may download dependencies and execute normal project build code. DAP uses stdin/stdout; runtime log messages never contaminate protocol stdout. Frame limits reject oversized input. Filesystem paths are passed as process arguments rather than shell command interpolation.

## Build diagnostics and Gradle scope

The compile command uses a separate timing init script, with an injected BuildEventsListenerRegistry and an OperationCompletionListener shared build service. The service receives task completion timestamps/outcomes and remains active on configuration-cache hits. It does not attach legacy graph/build listeners or change compiler actions/options. Discovery and JavaExec capture retain their model-oriented init script and no-configuration-cache boundary; subsequent compilation honours project/user cache settings.

Initial task selection includes the selected module and mapped modules in its main runtime classpath. After JavaExec capture, selection uses the actual resolved launch classpath instead. All selected consumers remain requested, so Gradle can recompile them on dependency ABI changes, follow compile-only/generator/task prerequisites, or skip them on ABI-compatible edits. Source polling still observes discovered source roots; an unrelated edit can result in an up-to-date build but does not force that unrelated module to compile. Maven reactor and plain javac build selection are unchanged.

Rust monotonic clocks measure discovery, startup source scanning, launch preparation, runtime copying, build process wall time (including failure), reload publication and compilation-to-commit. The agent separately measures class hashing, loaded/changed class counts and redefinition; compile-to-commit excludes that asynchronous work. Gradle's task window spans first task start to last finish; summed task duration can exceed wall time with parallel tasks. The difference from process wall time includes startup, configuration, gaps and teardown, and is not labelled as pure configuration time. `buildDiagnostics` adds Gradle's own info-level incremental decisions without changing JavaCompile settings.
