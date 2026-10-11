# Custom JavaExec launch

Open this directory in Zed and select **Configured JavaExec HotSwap**, or run:

```sh
java-hotswap run --project . --gradle-run-task :runConfigured
```

Requires JDK 17+ and compatible Gradle on PATH. This fixture resolves Gson and verifies required application arguments, argument providers, preparation dependencies, a working directory containing spaces, environment variables, system properties, assertions, heap settings and an extra runtime resource. It prints `launch-settings-ok`, followed by the JVM PID and changing values. Edit `Main.java` method bodies to test reloads without restarting.

The fixture’s credential-shaped value is disposable test data. Its deliberately failing post-action/finalizer verifies that capture leaves application ownership to the companion.
