package com.accreta.examples;

import com.accreta.*;

import java.time.Duration;
import java.time.Instant;

/**
 * Java port of {@code accreta}'s {@code tdigest_quantiles.rs} example.
 *
 * <p>Registers {@code TDigest} alongside {@code Count} and {@code Sum} on one measure,
 * ingests a spread of latency samples (all under one dimension value, so no grouped-query
 * support is needed for full parity with the Rust original), rolls up to the hour, and reads
 * back quantile estimates — then demonstrates {@code TDigest}'s approximate-associativity
 * property directly via the standalone {@link TDigest} wrapper.
 */
public final class TDigestQuantilesExample {

    public static void main(String[] args) throws Exception {
        // 1. Register TDigest alongside Count and Sum on the same measure.
        Schema schema;
        try (SchemaBuilder builder = new SchemaBuilder()) {
            builder.dimension("route");
            builder.measureF64("request_latency_ms")
                    .withCount()
                    .withSum()
                    .withTDigest()
                    .done();
            schema = builder.build();
        }

        double[] latenciesMs = {
                12.0, 15.0, 11.0, 20.0, 14.0, 9.0, 100.0, 13.0, 16.0, 12.0,
                18.0, 250.0, 14.0, 15.0, 11.0, 13.0, 17.0, 12.0, 19.0, 500.0,
        };

        try (Engine engine = new Engine(schema)) {
            // 2. Ingest a spread of latency samples across a few minutes, all for the same route.
            Instant t0 = Instant.parse("2026-03-15T10:00:00Z");
            for (int i = 0; i < latenciesMs.length; i++) {
                Instant t = t0.plus(Duration.ofMinutes(i));
                engine.ingest(t, new double[]{latenciesMs[i]}, new String[]{"/api/search"});
            }

            // 3. Roll up to the hour.
            engine.rollup();

            Instant hourStart = t0.truncatedTo(java.time.temporal.ChronoUnit.HOURS);
            try (AggregateSet latencySet = engine.queryRange(
                    BucketLevel.HOUR, hourStart, hourStart.plus(Duration.ofHours(1)), /* measureId */ 0)) {

                long count = latencySet.getCount();
                double mean = count == 0 ? Double.NaN : latencySet.getSum() / count;

                System.out.println("samples ingested : " + count);
                System.out.printf("exact mean       : %.1f ms%n", mean);
                System.out.printf("p50 (median)     : %.1f ms%n", latencySet.getQuantile(0.50));
                System.out.printf("p95              : %.1f ms%n", latencySet.getQuantile(0.95));
                System.out.printf("p99              : %.1f ms%n", latencySet.getQuantile(0.99));

                if (!(latencySet.getQuantile(0.50) < mean)) {
                    throw new AssertionError("median should sit below the outlier-skewed mean");
                }
            }
        }

        // 4. Approximate associativity: merging the same samples in a different grouping
        //    produces a digest whose *quantile answers* agree closely with the original, even
        //    though the two digests are not structurally equal.
        TDigest byThirds = TDigest.identity();
        for (int start = 0; start < latenciesMs.length; start += 7) {
            TDigest piece = TDigest.identity();
            int end = Math.min(start + 7, latenciesMs.length);
            for (int i = start; i < end; i++) {
                piece.updateInPlace(latenciesMs[i]);
            }
            byThirds.mergeInPlace(piece);
            piece.close();
        }

        TDigest allAtOnce = TDigest.identity();
        for (double v : latenciesMs) {
            allAtOnce.updateInPlace(v);
        }

        double p50A = byThirds.quantile(0.50);
        double p50B = allAtOnce.quantile(0.50);
        System.out.printf("%np50 via chunked merges : %.2f ms%np50 via single digest  : %.2f ms%n", p50A, p50B);

        if (Math.abs(p50A - p50B) >= 1.0) {
            throw new AssertionError("merge order shouldn't meaningfully change the quantile estimate");
        }

        byThirds.close();
        allAtOnce.close();
        schema.close();
    }
}
