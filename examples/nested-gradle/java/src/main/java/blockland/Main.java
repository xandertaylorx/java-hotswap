package blockland;

import com.google.gson.Gson;
import java.lang.management.ManagementFactory;
import org.lwjgl.Version;
import org.lwjgl.system.MemoryUtil;

public class Main {
    static int explosionRadius() { return 0; }
    public static void main(String[] args) throws Exception {
        var flags = ManagementFactory.getRuntimeMXBean().getInputArguments();
        if (!flags.contains("--enable-native-access=ALL-UNNAMED") || !Boolean.getBoolean("hotswap.fixture") || !Boolean.getBoolean("hotswap.run"))
            throw new IllegalStateException("Gradle JVM arguments were lost");
        var buffer = MemoryUtil.memAlloc(16);
        MemoryUtil.memFree(buffer);
        System.out.println("deps=" + Version.getVersion() + " gson=" + new Gson().toJson("resolved") + " flags=true cwd=" + System.getProperty("user.dir"));
        while (true) {
            System.out.println("radius=" + explosionRadius() + " pid=" + ProcessHandle.current().pid());
            Thread.sleep(250);
        }
    }
}
