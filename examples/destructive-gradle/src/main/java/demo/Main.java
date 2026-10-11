package demo;
import java.nio.file.*;
import java.nio.charset.StandardCharsets;
import com.google.gson.Gson;
public class Main {
    static int radius() { return 0; }
    public static void main(String[] args) throws Exception {
        if (args.length != 1 || !args[0].equals("required-argument") || !"inherited".equals(System.getProperty("fixture.launch")))
            throw new IllegalStateException("Run configuration missing");
        System.out.println("dependency=" + new Gson().toJson("resolved"));
        for (;;) {
            try (var stream = Main.class.getClassLoader().getResourceAsStream("stable.txt")) {
                if (stream == null || !new String(stream.readAllBytes(), StandardCharsets.UTF_8).trim().equals("stable-resource"))
                    throw new IllegalStateException("Runtime resource disappeared");
            }
            for (int phase = 1; phase <= 2; phase++) {
                Path request = Path.of("load-" + phase);
                if (Files.exists(request)) {
                    Object value = Class.forName("demo.Late" + phase).getMethod("touch").invoke(null);
                    Object dependency = Class.forName("dependency.Late" + phase).getMethod("touch").invoke(null);
                    System.out.println("lazy-" + phase + "=" + value + "/" + dependency);
                    Files.delete(request);
                }
            }
            System.out.println("radius=" + radius() + " pid=" + ProcessHandle.current().pid());
            Thread.sleep(100);
        }
    }
}
