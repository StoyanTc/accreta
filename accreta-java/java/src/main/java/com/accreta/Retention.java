package com.accreta;

import java.time.Duration;

/**
 * Mirrors {@code accreta::retention::Retention}.
 *
 * <pre>{@code
 * Retention policy = new Retention().keep(BucketLevel.MINUTE, Duration.ofHours(1));
 * Engine engine = new Engine(schema, policy);
 * }</pre>
 */
public final class Retention implements AutoCloseable {
    static {
        NativeLibrary.load();
    }

    private long handle;

    public Retention() {
        this.handle = nativeNew();
    }

    /** Keep buckets at {@code level} for {@code maxAge} (measured back from that level's newest bucket). */
    public Retention keep(BucketLevel level, Duration maxAge) {
        checkOpen();
        nativeKeep(handle, level.ordinal(), maxAge.getSeconds());
        return this;
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
            throw new IllegalStateException("Retention already closed");
        }
    }

    private static native long nativeNew();

    private static native void nativeKeep(long handle, int levelOrdinal, long maxAgeSeconds);

    private static native void nativeDrop(long handle);
}
