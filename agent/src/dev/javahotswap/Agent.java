package dev.javahotswap;

import java.lang.instrument.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.security.MessageDigest;
import java.util.*;

/** Only project-owned output directories are eligible; a successful-build commit gates reloads. */
public final class Agent {
    private final Instrumentation instrumentation;
    private final Path commit;
    private final List<Path> roots;
    private Map<Path, byte[]> previous;

    private Agent(String config, Instrumentation instrumentation) throws Exception {
        this.instrumentation = instrumentation;
        List<String> lines = Files.readAllLines(Path.of(config), StandardCharsets.UTF_8);
        commit = Path.of(lines.get(0));
        roots = new ArrayList<>();
        for (String line : lines.subList(1, lines.size())) roots.add(Path.of(line).toAbsolutePath().normalize());
        previous = hashes(roots);
    }
    public static void premain(String config, Instrumentation instrumentation) {
        try {
            if (!instrumentation.isRedefineClassesSupported()) {
                log("ERROR JVM does not support class redefinition"); return;
            }
            Agent agent = new Agent(config, instrumentation);
            Thread watcher = new Thread(agent::watch, "java-hotswap-agent");
            watcher.setDaemon(true); watcher.start();
            log("READY watching " + agent.roots.size() + " project output directories");
        } catch (Exception | LinkageError error) { log("ERROR agent startup: " + error); }
    }
    private void watch() {
        String generation = "";
        while (!Thread.currentThread().isInterrupted()) {
            try {
                String next = Files.exists(commit) ? Files.readString(commit) : "";
                if (!next.equals(generation)) {
                    generation = next;
                    if (!next.isEmpty()) {
                        Path snapshot = Path.of(next);
                        reload(snapshot);
                        Files.writeString(commit.resolveSibling("ack"), snapshot.getFileName().toString());
                    }
                }
                Thread.sleep(100);
            } catch (InterruptedException stop) { Thread.currentThread().interrupt(); }
              catch (Exception | LinkageError error) { log("ERROR reload watcher: " + error); }
        }
    }
    private Map<Path, byte[]> hashes(List<Path> inputs) throws Exception {
        Map<Path, byte[]> result = new HashMap<>();
        MessageDigest digest = MessageDigest.getInstance("SHA-256");
        for (int i = 0; i < inputs.size(); i++) {
            Path root = inputs.get(i);
            Path origin = roots.get(i);
            if (!Files.isDirectory(root)) continue;
            try (var files = Files.walk(root)) {
                for (Path file : files.filter(p -> p.toString().endsWith(".class") && !p.getFileName().toString().equals("module-info.class")).toList())
                    result.put(origin.resolve(root.relativize(file)), digest.digest(Files.readAllBytes(file)));
            }
        }
        return result;
    }
    private void reload(Path snapshot) throws Exception {
        List<Path> inputs = new ArrayList<>();
        for (int i = 0; i < roots.size(); i++) inputs.add(snapshot.resolve(Integer.toString(i)));
        Map<Path, byte[]> current = hashes(inputs);
        List<ClassDefinition> definitions = new ArrayList<>();
        for (Class<?> type : instrumentation.getAllLoadedClasses()) {
            if (type.isArray() || type.isPrimitive() || type.getName().startsWith("dev.javahotswap.")) continue;
            Path origin;
            try {
                var source = type.getProtectionDomain().getCodeSource();
                if (source == null || !source.getLocation().getProtocol().equals("file")) continue;
                origin = Path.of(source.getLocation().toURI()).toAbsolutePath().normalize();
            } catch (Exception denied) { continue; }
            if (!roots.contains(origin)) continue; // Never redefine JAR dependencies or a same-named class in another origin.
            Path file = origin.resolve(type.getName().replace('.', '/') + ".class");
            if (current.containsKey(file) && !Arrays.equals(current.get(file), previous.get(file))) {
                if (instrumentation.isModifiableClass(type)) definitions.add(new ClassDefinition(type, Files.readAllBytes(inputs.get(roots.indexOf(origin)).resolve(origin.relativize(file)))));
                else log("UNSUPPORTED unmodifiable class " + type.getName());
            } else if (previous.containsKey(file) && !current.containsKey(file)) log("UNSUPPORTED deleted loaded class " + type.getName());
        }
        previous = current; // One diagnostic per successful build, including rejected structural changes.
        if (definitions.isEmpty()) return;
        try {
            instrumentation.redefineClasses(definitions.toArray(ClassDefinition[]::new));
            for (ClassDefinition definition : definitions) log("RELOADED " + definition.getDefinitionClass().getName());
        } catch (UnsupportedOperationException | ClassNotFoundException | UnmodifiableClassException | LinkageError error) {
            log("UNSUPPORTED batch " + definitions.stream().map(d -> d.getDefinitionClass().getName()).toList()
                + ": " + error + "; application continues with previous definitions; restart manually if needed");
        }
    }
    private static void log(String message) { System.err.println("[Java HotSwap] " + message); }
}
