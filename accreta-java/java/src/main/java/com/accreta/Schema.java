package com.accreta;

/**
 * Mirrors {@code accreta::aggregate_set::Schema}.
 *
 * <p>Cheap to hold onto — the Rust side is {@code Arc}-backed, so {@link Engine#Engine(Schema)}
 * clones it rather than taking ownership, the same way you'd share one {@code Schema} across
 * multiple {@code Engine}s in Rust.
 */
public final class Schema implements AutoCloseable {
    static {
        NativeLibrary.load();
    }

    private long handle;

    /** Package-private: only {@link SchemaBuilder#build()} constructs a {@code Schema}. */
    Schema(long handle) {
        this.handle = handle;
    }

    public int dimensionCount() {
        checkOpen();
        return (int) nativeDimensionCount(handle);
    }

    public int measureCount() {
        checkOpen();
        return (int) nativeMeasureCount(handle);
    }

    long handle() {
        checkOpen();
        return handle;
    }

    @Override
    public void close() {
        if (handle != 0) {
            nativeDrop(handle);
            handle = 0;
        }
    }

    private void checkOpen() {
        if (handle == 0) {
            throw new IllegalStateException("Schema already closed");
        }
    }

    private static native long nativeDimensionCount(long handle);

    private static native long nativeMeasureCount(long handle);

    private static native void nativeDrop(long handle);
}
