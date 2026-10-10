# Nested Gradle workspace

Open this directory in Zed, keeping `java/` nested. Launch the included Java HotSwap debug scenario or run:

```sh
java-hotswap run --project .
```

The companion discovers `java/`, invokes its Gradle 9.1 wrapper, resolves LWJGL 3.3.6 and Gson, and uses `build.nosync` outputs. Gradle selects `blockland.Main` despite the second entry point in `Other.java`. Application default JVM arguments and additional `run.jvmArgs` are retained, including native access and the first-thread flag on macOS. The example allocates/frees native LWJGL memory without opening a window.

Change `return 0;` to `return 10;` in `java/src/main/java/blockland/Main.java` and save. The printed radius changes while the PID stays the same. The parent directory holds session state; build files are left untouched.
