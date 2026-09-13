package com.accreta;

/**
 * Mirrors {@code accreta::aggregate_set::MeasureBuilder<'a, T>} for {@code T = f64}.
 *
 * <p>Obtained from {@link SchemaBuilder#measureF64}; call {@link #done()} to return to the
 * parent builder, same as the Rust side's {@code MeasureBuilder::done}. Holding a reference to
 * the parent keeps it reachable (and un-collectible) for exactly as long as this measure builder
 * is open, matching the borrow {@code MeasureBuilder<'a, T>} holds in Rust.
 */
public final class MeasureBuilder implements AutoCloseable {
    private long handle;
    private final SchemaBuilder parent;

    MeasureBuilder(long handle, SchemaBuilder parent) {
        this.handle = handle;
        this.parent = parent;
    }

    public MeasureBuilder withSum() {
        checkOpen();
        nativeWithSum(handle);
        return this;
    }

    public MeasureBuilder withCount() {
        checkOpen();
        nativeWithCount(handle);
        return this;
    }

    public MeasureBuilder withMin() {
        checkOpen();
        nativeWithMin(handle);
        return this;
    }

    public MeasureBuilder withMax() {
        checkOpen();
        nativeWithMax(handle);
        return this;
    }

    /** Only attachable to f64 measures — mirrors the compile-time restriction on the Rust side. */
    public MeasureBuilder withTDigest() {
        checkOpen();
        nativeWithTDigest(handle);
        return this;
    }

    /** Returns to the parent {@link SchemaBuilder}, finishing this measure's registration. */
    public SchemaBuilder done() {
        checkOpen();
        nativeDone(handle);
        handle = 0;
        return parent;
    }

    /** Equivalent to calling {@link #done()} and discarding the result. */
    @Override
    public void close() {
        if (handle != 0) {
            nativeDone(handle);
            handle = 0;
        }
    }

    private void checkOpen() {
        if (handle == 0) {
            throw new IllegalStateException("MeasureBuilder already done() or closed");
        }
    }

    private static native void nativeWithSum(long handle);

    private static native void nativeWithCount(long handle);

    private static native void nativeWithMin(long handle);

    private static native void nativeWithMax(long handle);

    private static native void nativeWithTDigest(long handle);

    private static native void nativeDone(long handle);
}
