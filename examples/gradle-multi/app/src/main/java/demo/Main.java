package demo;
public class Main {
    public static void main(String[] args) throws Exception {
        while (true) {
            System.out.println("radius=" + Radius.explosionRadius() + " pid=" + ProcessHandle.current().pid());
            Thread.sleep(250);
        }
    }
}
