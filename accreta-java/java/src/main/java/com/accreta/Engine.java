package com.accreta;

import java.time.Instant;

/**
 * Mirrors {@code accreta::engine::Engine}.
 *
 * <pre>{@code
 * try (Schema schema = builder.build();
 *      Engine engine = new Engine(schema)) {
 *     engine.ingest(Instant.now(), new double[]{12.0}, new String[]{"server-a"});
 *     engine.rollup();
 *     try (AggregateSet result = engine.queryRange(
 *             BucketLevel.HOUR, start, end, /* measureId = *\/ 0)) {
 *         double total = result.getSum();
 *     }
 * }
 * }</pre>
 */
public final class Engine implements AutoCloseable {
    static {
        NativeLibrary.load();
    }

    private long handle;

    public Engine(Schema schema) {
        this.handle = nativeNew(schema.handle());
    }

    /** Mirrors {@code Engine::with_retention} — bounds memory use per {@link BucketLevel}. */
    public Engine(Schema schema, Retention retention) {
        this.handle = nativeNewWithRetention(schema.handle(), retention.handle());
    }

    /**
     * Folds one sample into the second bucket for {@code timestamp} (sub-second precision
     * is truncated).
     *
     * @param measures   values for every measure in the schema, in registration order
     * @param dimensions values for every dimension in the schema, in registration order
     */
    public void ingest(Instant timestamp, double[] measures, String[] dimensions) throws IngestException {
        checkOpen();
        nativeIngest(handle, timestamp.toEpochMilli(), measures, dimensions);
    }

    /**
     * Recomputes every level above {@link BucketLevel#SECOND} by merging bucket states upward.
     * Idempotent. Coarser levels are rebuilt from the level below on every call, so call
     * {@link #prune()} <em>after</em> this rather than before a later rollup: pruning SECOND
     * buckets and rolling up again recomputes MINUTE and above from only the surviving seconds.
     */
    public void rollup() {
        checkOpen();
        nativeRollup(handle);
    }

    /** Discards buckets older than the configured retention window. No-op with the default (unbounded) retention. */
    public void prune() {
        checkOpen();
        nativePrune(handle);
    }

    public long bucketCount(BucketLevel level) {
        checkOpen();
        return nativeBucketCount(handle, level.ordinal());
    }

    /**
     * Merges every bucket in {@code [rangeStart, rangeEnd)} at {@code level} into one total
     * {@link AggregateSet}. Caller owns the returned set and must close it.
     */
    public AggregateSet queryRange(BucketLevel level, Instant rangeStart, Instant rangeEnd, int measureId)
            throws SchemaException {
        checkOpen();
        long setHandle = nativeQueryRange(
                handle, level.ordinal(), rangeStart.toEpochMilli(), rangeEnd.toEpochMilli(), measureId);
        return new AggregateSet(setHandle);
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
            throw new IllegalStateException("Engine already closed");
        }
    }

    private static native long nativeNew(long schemaHandle);

    private static native long nativeNewWithRetention(long schemaHandle, long retentionHandle);

    private static native void nativeIngest(long handle, long epochMillis, double[] measures, String[] dimensions)
            throws IngestException;

    private static native void nativeRollup(long handle);

    private static native void nativePrune(long handle);

    private static native long nativeBucketCount(long handle, int levelOrdinal);

    private static native long nativeQueryRange(
            long handle, int levelOrdinal, long rangeStartMillis, long rangeEndMillis, int measureId)
            throws SchemaException;

    private static native void nativeDrop(long handle);
}
