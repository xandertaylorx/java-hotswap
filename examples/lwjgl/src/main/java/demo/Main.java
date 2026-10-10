package demo;
import static org.lwjgl.glfw.GLFW.*;
public class Main {
    static int explosionRadius() { return 0; }
    public static void main(String[] args) throws Exception {
        if (!glfwInit()) throw new IllegalStateException("GLFW needs an active desktop session");
        glfwWindowHint(GLFW_CLIENT_API, GLFW_NO_API);
        long window = glfwCreateWindow(640, 400, "Java HotSwap", 0, 0);
        if (window == 0) throw new IllegalStateException("Cannot create window");
        try {
            while (!glfwWindowShouldClose(window)) {
                glfwSetWindowTitle(window, "Explosion radius: " + explosionRadius());
                System.out.println("radius=" + explosionRadius() + " pid=" + ProcessHandle.current().pid());
                glfwPollEvents(); Thread.sleep(250);
            }
        } finally { glfwDestroyWindow(window); glfwTerminate(); }
    }
}
