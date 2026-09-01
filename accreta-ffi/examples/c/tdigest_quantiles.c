// examples/tdigest_quantiles/tdigest_quantiles.c
//
// Approximate quantiles with TDigest, through the accreta-ffi C ABI — mirrors the Rust
// `tdigest_quantiles` example: register TDigest alongside Count and Average on a "latency"
// measure, ingest a spread of samples across several minutes, roll up to an hour bucket, and
// read back quantile estimates with accreta_aggregate_set_get_quantile.
//
// NOT covered here (unlike the Rust example): the "merges are only approximately associative"
// demonstration. That works directly with a standalone TDigest via Monoid::merge_in_place,
// which this crate deliberately never exposes across the C boundary (see lib.rs's crate docs) —
// the only way to fold samples into a TDigest from C is through accreta_engine_ingest +
// accreta_engine_rollup, and the merge order there is decided internally by the rollup
// hierarchy, not something a caller can vary to produce two digests to compare. If you need to
// see that property directly, run the Rust example instead.
//
// NOTE: enum variant spelling below (e.g. AccretaAggregateKind_TDigest) is what this project's
// cbindgen.toml is expected to produce. Check your generated accreta_ffi.h and adjust the
// spelling here if it differs.
//
// Build: see Makefile in this directory.

#include <stdio.h>
#include <stdlib.h>

#include "accreta_ffi.h"

static void check(AccretaStatus status, const char *what) {
    if (status != ACCRETA_STATUS_OK) {
        fprintf(stderr, "%s failed (status %d): %s\n", what, (int)status,
                accreta_last_error_message());
        exit(1);
    }
}

int main(void) {
    // 1. Register TDigest alongside Count and Average on the same measure. TDigest is
    //    deliberately heavier than the exact aggregates, so in a real schema you'd register it
    //    only on the measures that actually need quantiles — Average stays the cheap, exact
    //    general-purpose mean for the same measure.
    AccretaSchemaBuilder *builder = accreta_schema_builder_new();
    check(accreta_schema_builder_add_dimension(builder, "route"), "add_dimension");

    AccretaAggregateKind kinds[] = {
        ACCRETA_AGGREGATE_KIND_COUNT,
        ACCRETA_AGGREGATE_KIND_AVERAGE,
        ACCRETA_AGGREGATE_KIND_T_DIGEST,
    };
    check(
        accreta_schema_builder_add_measure(
            builder, "request_latency_ms", ACCRETA_MEASURE_TYPE_F64, kinds, 3),
        "add_measure");

    AccretaSchema *schema = NULL;
    check(accreta_schema_builder_build(builder, &schema), "build");

    AccretaEngine *engine = accreta_engine_new(schema);
    if (engine == NULL) {
        fprintf(stderr, "accreta_engine_new returned null\n");
        return 1;
    }

    // 2. Ingest a spread of latency samples across a few minutes, all for the same route.
    //    Nothing here is TDigest-specific — ingestion looks identical to any other measure.
    double latencies_ms[] = {
        12.0, 15.0, 11.0, 20.0, 14.0, 9.0, 100.0, 13.0, 16.0, 12.0,
        18.0, 250.0, 14.0, 15.0, 11.0, 13.0, 17.0, 12.0, 19.0, 500.0,
    };
    size_t n = sizeof(latencies_ms) / sizeof(latencies_ms[0]);

    // 2026-03-15T10:00:00Z, in ms since the Unix epoch.
    long long t0_ms = 1773568800000LL;
    const char *route = "/api/search";
    const char *dims[] = { route };

    for (size_t i = 0; i < n; i++) {
        AccretaMeasureValue value;
        value.tag = ACCRETA_MEASURE_TYPE_F64;
        value.value.f64 = latencies_ms[i];

        long long t_ms = t0_ms + (long long)i * 60 * 1000;
        int status = accreta_engine_ingest(engine, t_ms, &value, 1, dims, 1);
        if (status != (int)ACCRETA_STATUS_OK) {
            fprintf(stderr, "ingest failed at sample %zu (status %d): %s\n", i, status,
                    accreta_last_error_message());
            return 1;
        }
    }

    // 3. Roll up to the hour, same as any other measure — TDigest merges through the rollup
    //    hierarchy exactly like the exact aggregates do, it just doesn't guarantee an exact
    //    answer at the end.
    accreta_engine_rollup(engine);

    // Hour bucket start = t0 truncated to the hour.
    long long hour_start_ms = (t0_ms / (3600LL * 1000)) * (3600LL * 1000);

    AccretaBucket *hour = NULL;
    check(
        accreta_engine_bucket(engine, ACCRETA_BUCKET_LEVEL_HOUR, hour_start_ms, &hour),
        "engine_bucket");

    AccretaGroupCursor *groups = accreta_bucket_groups_cursor(hour);
    AccretaDimensionKey *key = NULL;
    AccretaAggregateSetList *sets = NULL;
    if (!accreta_group_cursor_next(groups, &key, &sets)) {
        fprintf(stderr, "expected at least one dimension group\n");
        return 1;
    }

    AccretaAggregateSet *latency_set = accreta_aggregate_set_list_get(sets, 0);

    AccretaMeasureValue count_value;
    check(
        accreta_aggregate_set_get_value(
            latency_set, ACCRETA_AGGREGATE_KIND_COUNT, ACCRETA_MEASURE_TYPE_F64, &count_value),
        "get_value(Count)");

    AccretaMeasureValue mean_value;
    check(
        accreta_aggregate_set_get_value(
            latency_set, ACCRETA_AGGREGATE_KIND_AVERAGE, ACCRETA_MEASURE_TYPE_F64, &mean_value),
        "get_value(Average)");

    double p50, p95, p99;
    check(accreta_aggregate_set_get_quantile(latency_set, 0.50, &p50), "get_quantile(p50)");
    check(accreta_aggregate_set_get_quantile(latency_set, 0.95, &p95), "get_quantile(p95)");
    check(accreta_aggregate_set_get_quantile(latency_set, 0.99, &p99), "get_quantile(p99)");

    printf("samples ingested : %llu\n", (unsigned long long)count_value.value.u64);
    printf("exact mean       : %.1f ms\n", mean_value.value.f64);
    printf("p50 (median)     : %.1f ms\n", p50);
    printf("p95              : %.1f ms\n", p95);
    printf("p99              : %.1f ms\n", p99);

    // The mean is dragged upward by the outliers (100, 250, 500 ms) far more than the median
    // is — a good illustration of why you'd want both an exact mean *and* quantiles on the same
    // measure rather than relying on the mean alone to characterize latency.
    if (!(p50 < mean_value.value.f64)) {
        fprintf(stderr, "expected median to sit below the outlier-skewed mean\n");
        return 1;
    }

    // Also confirm TDigest is properly rejected by accreta_aggregate_set_get_value, rather than
    // silently returning a garbage value — a quick smoke test for that guard.
    AccretaMeasureValue bogus;
    AccretaStatus tdigest_via_get_value = accreta_aggregate_set_get_value(
        latency_set, ACCRETA_AGGREGATE_KIND_T_DIGEST, ACCRETA_MEASURE_TYPE_F64, &bogus);
    if (tdigest_via_get_value != ACCRETA_STATUS_TYPE_MISMATCH) {
        fprintf(stderr,
                "expected accreta_aggregate_set_get_value(TDigest) to return TypeMismatch, got "
                "%d\n",
                (int)tdigest_via_get_value);
        return 1;
    }

    // Cleanup.
    accreta_aggregate_set_free(latency_set);
    accreta_dimension_key_free(key);
    accreta_aggregate_set_list_free(sets);
    accreta_group_cursor_free(groups);
    accreta_bucket_free(hour);
    accreta_engine_free(engine);
    accreta_schema_free(schema);

    return 0;
}
