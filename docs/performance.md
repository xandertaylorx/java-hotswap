# Large Gradle build investigation — 2026-10-11

The reported 64-second save was investigated on macOS Apple Silicon using an isolated copy of the current Minecraft 1.21.11 project, Gradle 8.10 and Java 21. Original Minecraft source, build files and project configuration were not edited or built. The copy contained 6,637 main Java source files; it retained normal dependencies, compiler settings and task prerequisites. Builds used the cached dependency graph in offline mode and Gradle info-level diagnostics.

| Experiment | Process wall time | Gradle decision |
|---|---:|---|
| First build after relocating copied state | 28.332 s | Full rebuild: relocated output/cache paths, no incremental change information |
| No source changes | 0.620 s | compileJava/processResources/classes up to date |
| Single AbstractArrow.getGravity method-body literal change | 24.906 s | Incremental compilation of 8,426 classes; compiler 23.022 s, dependency analysis 1.101 s |

The one-file edit did **not** cause a forced clean or non-incremental build. Gradle's incremental dependency closure was nevertheless large; class counts include inner/generated classes and need not equal source-file counts. The extension did not invoke clean, rerun-tasks or a custom javac file list. The quiet isolated copy did not reproduce exactly 64 seconds, so these measurements do not establish how much contention, daemon state or other project changes contributed to the original duration.

Gradle's maintainers track [class-level ABI avoidance within a source set](https://github.com/gradle/gradle/issues/26337); the existing incremental compiler can recompile unchanged consumers of a changed class even when its ABI stays compatible. This is consistent with the observed broad incremental compilation. Cross-project compile avoidance is a separate mechanism and was verified by the regression tests.

The new task diagnostics attributed 28.089 s of a 31.118 s instrumented build to compileJava (again 8,426 classes), while processResources was up to date in 10 ms. That run overlapped regression work and initial timing-script compilation; it is not a controlled before/after speed comparison. No-change builds with the timing service were 0.611 s; an explicitly enabled configuration-cache reuse build was 0.578 s and retained task events. These small differences are within normal measurement variation.

## Safe changes made

- Rebuild commands no longer force configuration caching off. They honour the project's settings; HotSwap does not enable caching for an incompatible project. Model discovery and JavaExec preparation retain their existing no-cache boundary.
- Compile requests exclude unrelated modules and keep the selected application plus project modules in its resolved runtime classpath. Gradle continues following their declared dependencies. Tests verify ABI-compatible upstream edits skip consumer compilation and ABI changes rebuild consumers.
- The completion listener follows Gradle's [supported shared build-service approach](https://docs.gradle.org/current/userguide/build_services.html), so collecting task timings does not defeat configuration-cache reuse.
- Compiler incremental/fork settings are reported and preserved, including an explicit incremental=false policy. No tasks, processors, generated sources or source files are bypassed. Maven/plain builds remain unchanged apart from timings.

These changes remove avoidable extension overhead and unrelated builds; they do not claim to turn the dense one-source-set compilation into a one-file compile. No concrete erroneous setting in the current Minecraft build was identified, so its build configuration was left alone. Runtime isolation and successful-build-only publication remain in force.

## Reading a slow save

Set `buildDiagnostics: true` in an existing HotSwap launch scenario or workspace config and restart. Capture a save's COMPILING line, task outcomes/timings, Gradle incremental/full rebuild reason, TIMING build, TIMING reload_publish and TIMING agent_reload. A high compileJava time with a large incremental class count is compiler work; slow preparation tasks, missing outputs or disabled incremental compilation point elsewhere. UP-TO-DATE tasks with high process wall time indicate startup/configuration/gaps/teardown rather than javac alone. Parallel task durations overlap, so do not subtract their sum from wall time.

Changed source counts and timings contain no source contents or launch credentials. Gradle's optional verbose build output is project-controlled. Resources and dependency JARs remain pinned in the runtime copy until restart.
