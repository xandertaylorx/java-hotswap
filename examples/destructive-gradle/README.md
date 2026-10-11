# Destructive Gradle output regression

This fixture uses a custom `runConfigured` JavaExec task, Gson, a generated dependency JAR, custom `build.nosync` outputs, and lazy application/anonymous/dependency classes. `prepareSource` and `applyChanges` represent a build pipeline that regenerates outputs.

The automated regression runs on Gradle 9.1 and the real Gradle 8.10 wrapper with Java 21:

```sh
python3 tests/e2e.py HotSwap.test_destructive_gradle_failed_build_preserves_runtime HotSwap.test_destructive_gradle810_failed_build_preserves_runtime -v
```

When the test creates `destructive`, `applyChanges` deletes the application class directory, resource output and generated dependency JAR, then waits three seconds before `compileJava`. The test inserts a duplicate method, requests previously unloaded classes during the deletion window and after failure, verifies the same running PID and last successful method behavior, then repairs the source and verifies HotSwap recovery. It also compares the private runtime bytes before/after and verifies that shutdown removes the snapshot.

To run the example normally, use `java-hotswap run --project .`; its single JavaExec task is selected automatically. Requires JDK 17+ and compatible Gradle on PATH. The tests use temporary copies and do not edit the example or any Minecraft project.
