package com.accreta;

/**
 * Mirrors {@code accreta::bucket::BucketLevel}.
 *
 * <p><b>Ordinal order matters</b> — the native side ({@code engine.rs}'s
 * {@code bucket_level_from_ordinal}) maps this enum's {@link #ordinal()} straight onto the Rust
 * enum variant. If you ever reorder this enum, update that match arm-for-arm in the same
 * change.
 */
public enum BucketLevel {
    SECOND,
    MINUTE,
    HOUR,
    DAY,
    WEEK,
    MONTH,
    YEAR,
}
