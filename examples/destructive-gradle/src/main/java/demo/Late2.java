package demo;
public class Late2 {
    public static String touch() {
        return new Object() { public String toString() { return "class-2"; } }.toString();
    }
}
