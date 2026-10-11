package demo;
import java.nio.file.Files;
import java.nio.file.Path;
import com.google.gson.Gson;
public class Main {
    static int explosionRadius() { return 0; }
    public static void main(String[] args) throws Exception {
        boolean assertions = false;
        assert assertions = true;
        if (args.length != 3 || !args[0].equals("required-argument") || !args[1].equals(System.getenv("FIXTURE_SECRET")) || !args[2].equals("provider-argument")) throw new IllegalStateException("Required task arguments missing");
        if (!"inherited".equals(System.getenv("FIXTURE_ENV")) || !"inherited".equals(System.getProperty("fixture.system")) || !"inherited".equals(System.getProperty("fixture.jvm")) || !"inherited".equals(System.getProperty("fixture.provider"))) throw new IllegalStateException("Task environment or JVM settings missing");
        if (!Files.readString(Path.of("prepared.txt")).equals("ready")) throw new IllegalStateException("Task working directory missing");
        if (!assertions || Runtime.getRuntime().maxMemory() > 110 * 1024 * 1024L) throw new IllegalStateException("Structured JVM options missing");
        try (var resource = Main.class.getClassLoader().getResourceAsStream("task-resource.txt")) {
            if (resource == null || !new String(resource.readAllBytes()).equals("custom-classpath")) throw new IllegalStateException("Task runtime classpath missing");
        }
        new Gson().toJson("dependency-resolved");
        System.out.println("launch-settings-ok");
        while (true) {
            System.out.println("radius=" + explosionRadius() + " pid=" + ProcessHandle.current().pid());
            Thread.sleep(250);
        }
    }
}
