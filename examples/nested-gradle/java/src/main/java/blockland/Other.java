package blockland;

// Discovery must use application.mainClass despite another conventional entry point.
public class Other {
    public static void main(String[] args) { throw new AssertionError("Wrong entry point"); }
}
