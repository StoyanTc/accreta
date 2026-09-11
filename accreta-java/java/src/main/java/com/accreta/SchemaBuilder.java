package com.accreta;

/**
 * Mirrors {@code accreta::aggregate_set::SchemaBuilder}.
 *
 * <pre>{@code
 * Schema schema;
 * try (SchemaBuilder builder = new SchemaBuilder()) {
 *     builder.dimension("host");
 *     builder.measureF64("value").withSum().withCount().done();
 *     schema = builder.build(); // consumes the builder
 * }
 * }</pre>
 *
 * Only f64 measures are supported so far — see the crate root doc comment on the Rust side for
 * the i64/u64 follow-up plan.
 */
public final class SchemaBuilder implements AutoCloseable {
    static {
        NativeLibrary.load();
    }

    private long handle;

    public SchemaBuilder() {
        this.handle = nativeNew();
    }

    /**
     * Registers a dimension, assigning it the next available id. Registering the same name
     * twice is a no-op, same as the Rust side.
     */
    public SchemaBuilder dimension(String name) {
        checkOpen();
        nativeDimension(handle, name);
        return this;
    }

    /** Registers an f64 measure and returns a builder for attaching aggregates to it. */
    public MeasureBuilder measureF64(String name) {
        checkOpen();
        long measureHandle = nativeMeasureF64(handle, name);
        return new MeasureBuilder(measureHandle, this);
    }

    /**
     * Finalizes the builder into a {@link Schema}. Consumes this builder — it must not be used
     * again afterward (mirrors {@code SchemaBuilder::build(self)} taking ownership in Rust).
     */
    public Schema build() throws SchemaException {
        checkOpen();
        long schemaHandle = nativeBuild(handle);
        handle = 0; // consumed, whether build succeeded or threw
        return new Schema(schemaHandle);
    }

    @Override
    public void close() {
        if (handle != 0) {
            nativeDrop(handle);
            handle = 0;
        }
    }

    void checkOpen() {
        if (handle == 0) {
            throw new IllegalStateException("SchemaBuilder already built or closed");
        }
    }

    long handle() {
        return handle;
    }

    private static native long nativeNew();

    private static native void nativeDimension(long handle, String name);

    private static native long nativeMeasureF64(long handle, String name);

    private static native long nativeBuild(long handle) throws SchemaException;

    private static native void nativeDrop(long handle);
}
