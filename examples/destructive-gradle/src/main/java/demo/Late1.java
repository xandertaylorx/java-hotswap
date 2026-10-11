package demo;
public class Late1 {
    public static String touch() {
        return new Object() { public String toString() { return "class-1"; } }.toString();
    }
}
