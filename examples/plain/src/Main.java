public class Main {
    static int explosionRadius() { return 0; }
    public static void main(String[] args) throws Exception {
        while (true) {
            System.out.println("radius=" + explosionRadius() + " pid=" + ProcessHandle.current().pid());
            Thread.sleep(250);
        }
    }
}
