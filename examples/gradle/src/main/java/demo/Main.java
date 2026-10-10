package demo;
import org.apache.commons.lang3.StringUtils;
public class Main {
    static int explosionRadius() { return 0; }
    public static void main(String[] args) throws Exception {
        while (true) {
            System.out.println("radius=" + (StringUtils.isEmpty("") ? explosionRadius() : -1) + " pid=" + ProcessHandle.current().pid());
            Thread.sleep(250);
        }
    }
}
