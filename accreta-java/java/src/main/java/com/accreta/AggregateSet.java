package com.accreta;

import java.util.OptionalDouble;

/**
 * Mirrors {@code accreta::aggregate_set::AggregateSet}.
 *
 * <p>Rust's {@code set.get::<Sum<f64>>()} is a compile-time-typed lookup; JNI has no generics, so
 * this exposes one typed getter per built-in aggregate instead. Each getter's fallback value
 * matches what the corresponding Rust aggregate would report if it isn't registered on this
 * measure at all — check {@link Schema} up front if you need to tell "not registered" apart from
 * a genuine zero/empty result.
 */
public final class AggregateSet implements AutoCloseable {
    static {
        NativeLibrary.load();
    }

    private long handle;

    /** Package-private: only {@link Engine#queryRange} constructs one. */
    AggregateSet(long handle) {
        this.handle = handle;
    }

    /** {@code Sum<f64>.value()}. */
    public double getSum() {
        checkOpen();
        return nativeGetSum(handle);
    }

    /** {@code Count.value()}. */
    public long getCount() {
        checkOpen();
        return nativeGetCount(handle);
    }

    /** {@code Average<f64>.sum() / Average<f64>.count()} — matches the division done inline in the Rust examples. */
    public double getAverage() {
        checkOpen();
        long count = nativeGetAverageCount(handle);
        if (count == 0) {
            return Double.NaN;
        }
        return nativeGetAverageSum(handle) / (double) count;
    }

    /** {@code Min<f64>.value()} — empty until the first sample, same as the Rust {@code Option}. */
    public OptionalDouble getMin() {
        checkOpen();
        double[] out = new double[1];
        return nativeGetMin(handle, out) ? OptionalDouble.of(out[0]) : OptionalDouble.empty();
    }

    /** {@code Max<f64>.value()}. */
    public OptionalDouble getMax() {
        checkOpen();
        double[] out = new double[1];
        return nativeGetMax(handle, out) ? OptionalDouble.of(out[0]) : OptionalDouble.empty();
    }

    /** {@code TDigest.quantile(q)}. Returns {@code NaN} if TDigest wasn't registered on this measure. */
    public double getQuantile(double q) {
        checkOpen();
        return nativeGetQuantile(handle, q);
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
            throw new IllegalStateException("AggregateSet already closed");
        }
    }

    private static native double nativeGetSum(long handle);

    private static native long nativeGetCount(long handle);

    private static native double nativeGetAverageSum(long handle);

    private static native long nativeGetAverageCount(long handle);

    private static native boolean nativeGetMin(long handle, double[] out);

    private static native boolean nativeGetMax(long handle, double[] out);

    private static native double nativeGetQuantile(long handle, double q);

    private static native void nativeDrop(long handle);
}
