package sample.declarations;

@Deprecated(since = "26", forRemoval = true)
sealed interface Shape permits Point, Branch {
    default int area() {
        return 0;
    }
}

record Point(int x, int y) implements Shape {
    Point {
        if (x < 0) {
            throw new IllegalArgumentException();
        }
    }
}

non-sealed interface Branch extends Shape {}

enum Mode {
    FAST(1) {
        @Override
        int code() {
            return 10;
        }
    },
    SAFE(2);

    final int value;

    Mode(int value) {
        this.value = value;
    }

    abstract int code();
}

strictfp abstract class ModifierSurface<T> {
    private static final long SERIAL = 1L;
    protected volatile int state;
    public transient Object cached;

    ModifierSurface(T value) {}

    public static native synchronized void reset();

    protected abstract T convert(final T value) throws Exception;
}
