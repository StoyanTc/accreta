package com.accreta;

/**
 * Mirrors {@code accreta::aggregates::TDigest} as a free-standing value — distinct from
 * {@link AggregateSet#getQuantile}, which only reads a digest already living inside a set.
 *
 * <p>Approximate, compressing sketch: merges are only <em>approximately</em> associative, so
 * digests built from the same samples via different merge orders won't be structurally equal,
 * even though their {@link #quantile} answers agree within its error bound. See the Rust
 * {@code tdigest.rs} module docs for the full explanation.
 */
public final class TDigest implements AutoCloseable {
    static {
        NativeLibrary.load();
    }

    private long handle;

    private TDigest(long handle) {
        this.handle = handle;
    }

    /** The empty digest — {@code Monoid::identity()}. */
    public static TDigest identity() {
        return new TDigest(nativeIdentity());
    }

    /** Folds one raw sample in, in place. */
    public void updateInPlace(double value) {
        checkOpen();
        nativeUpdateInPlace(handle, value);
    }

    /** Merges {@code other} into this digest, in place. {@code other} is left untouched. */
    public void mergeInPlace(TDigest other) {
        checkOpen();
        other.checkOpen();
        nativeMergeInPlace(handle, other.handle);
    }

    /** Estimates the value at quantile {@code q} ({@code 0.0..=1.0}). {@code NaN} if empty. */
    public double quantile(double q) {
        checkOpen();
        return nativeQuantile(handle, q);
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
            throw new IllegalStateException("TDigest already closed");
        }
    }

    private static native long nativeIdentity();

    private static native void nativeUpdateInPlace(long handle, double value);

    private static native void nativeMergeInPlace(long handle, long otherHandle);

    private static native double nativeQuantile(long handle, double q);

    private static native void nativeDrop(long handle);
}
