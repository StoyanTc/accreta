package com.accreta.examples;

import com.accreta.*;

import java.time.Duration;
import java.time.Instant;
import java.time.temporal.ChronoUnit;

/**
 * Java port of {@code accreta}'s {@code basic_usage.rs} example, adapted to what's currently
 * wrapped.
 *
 * <p><b>One deliberate simplification vs. the Rust original:</b> {@code basic_usage.rs} ingests
 * several distinct dimension values (browsers) and prints a per-browser breakdown by iterating
 * each {@code Bucket}'s {@code groups()} directly. Neither {@code Bucket} nor
 * {@code query_range_grouped}/{@code DimensionKey} are wrapped yet (see the crate README), so
 * this port uses a single dimension value throughout and reads results back with
 * {@link Engine#queryRange}, which merges every group into one total — the right substitute
 * *only because* there's a single group here. Once grouped queries are wrapped, the per-browser
 * breakdown can be restored faithfully.
 */
public final class BasicUsageExample {

    public static void main(String[] args) throws Exception {
        Schema schema;
        try (SchemaBuilder builder = new SchemaBuilder()) {
            builder.dimension("browser");
            builder.measureF64("visits")
                    .withSum()
                    .withCount()
                    .withMin()
                    .withMax()
                    .withAverage()
                    .done();
            schema = builder.build();
        }

        Instant start = Instant.parse("2026-03-15T09:00:00Z");
        // (minuteOffset, value) pairs spread across two hours.
        double[] values = new double[120];
        for (int i = 0; i < values.length; i++) {
            values[i] = 5.0 + (i % 7); // some arbitrary spread, mirrors "a day of readings"
        }

        try (Engine engine = new Engine(schema)) {
            for (int i = 0; i < values.length; i++) {
                engine.ingest(start.plus(Duration.ofMinutes(i)), new double[]{values[i]}, new String[]{"Firefox"});
            }

            engine.rollup();

            System.out.println("Per-hour totals:");
            for (int hour = 0; hour < 2; hour++) {
                Instant hourStart = start.plus(Duration.ofHours(hour));
                Instant hourEnd = hourStart.plus(Duration.ofHours(1));
                try (AggregateSet hourSet = engine.queryRange(BucketLevel.HOUR, hourStart, hourEnd, 0)) {
                    System.out.printf(
                            "  [%s .. %s) sum=%6.2f count=%-3d min=%5.2f max=%5.2f avg=%5.2f%n",
                            hourStart, hourEnd,
                            hourSet.getSum(), hourSet.getCount(),
                            hourSet.getMin().orElse(Double.NaN), hourSet.getMax().orElse(Double.NaN),
                            hourSet.getAverage());
                }
            }

            System.out.println("\nWhole range total (rolled all the way up):");
            Instant dayStart = start.truncatedTo(ChronoUnit.DAYS);
            try (AggregateSet dayTotal = engine.queryRange(BucketLevel.DAY, dayStart, dayStart.plus(Duration.ofDays(1)), 0)) {
                System.out.printf("  count=%d sum=%.2f average=%.2f%n",
                        dayTotal.getCount(), dayTotal.getSum(), dayTotal.getAverage());
            }

            // 5. Ad-hoc range query, same as the Rust original — merges whichever bucket states
            //    already exist without storing anything new.
            System.out.println("\nAd-hoc query for the first two hours only:");
            try (AggregateSet range = engine.queryRange(BucketLevel.HOUR, start, start.plus(Duration.ofHours(2)), 0)) {
                System.out.printf("  count=%d sum=%.2f%n", range.getCount(), range.getSum());
            }
        }

        // 6. Retention: keep only the last hour of minute-level detail.
        System.out.println("\nRetention: keeping only the last hour of minute-level detail");
        Schema retentionSchema;
        try (SchemaBuilder builder = new SchemaBuilder()) {
            builder.dimension("browser");
            builder.measureF64("visits").withSum().withCount().done();
            retentionSchema = builder.build();
        }

        try (Retention policy = new Retention().keep(BucketLevel.MINUTE, Duration.ofHours(1));
             Engine boundedEngine = new Engine(retentionSchema, policy)) {

            for (int i = 0; i < values.length; i++) {
                try {
                    boundedEngine.ingest(start.plus(Duration.ofMinutes(i)), new double[]{values[i]}, new String[]{"Firefox"});
                } catch (IngestException ignored) {
                    // basic_usage.rs discards ingest errors here too (`_ = ...`).
                }
            }

            System.out.println("  before prune: " + boundedEngine.bucketCount(BucketLevel.MINUTE) + " minute buckets");
            boundedEngine.prune();
            System.out.println("  after prune:  " + boundedEngine.bucketCount(BucketLevel.MINUTE)
                    + " minute buckets (older than 1h before the newest sample were dropped)");
        }

        retentionSchema.close();
        schema.close();
    }
}
