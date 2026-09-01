"""Approximate quantiles with `TDigest`, via accreta-py.

Python counterpart to the core crate's `tdigest_quantiles.rs` example: register `TDigest`
alongside the exact built-ins on a "request_latency_ms" measure, ingest a spread of samples
across several minutes, roll up to an hour bucket, and read back quantile estimates.

accreta-py doesn't expose `TDigest`/`Monoid` directly the way the Rust API does — an
`AggregateSet` is the only handle you get, read through `.values()` / `.quantile()`. So
instead of merging two standalone `TDigest`s to show the "approximate associativity"
property, this demonstrates the same idea using only the public Python surface: reading p50
off the pre-rolled-up hour bucket vs. reading p50 by merging the raw minute buckets directly
(`query_range` at "minute" level spanning the whole hour). Two different merge paths over the
same 20 samples; the *quantile answers* should agree within a small tolerance even though nothing
here says the two underlying digests are structurally identical.

Run with (after `maturin develop` in this crate):

    python examples/tdigest_quantiles.py
"""

from datetime import datetime, timedelta, timezone

import accreta


def main() -> None:
    # 1. Register TDigest alongside Count/Average on the same measure. TDigest is
    #    deliberately heavier than the exact aggregates, so it's registered only on the one
    #    measure that actually needs quantiles ("request_latency_ms") — Average stays the
    #    cheap, exact general-purpose mean for the same measure, and other measures in a real
    #    schema wouldn't get TDigest at all unless they too needed quantile queries.
    builder = accreta.SchemaBuilder()
    builder.dimension("route")
    builder.measure("request_latency_ms", "f64", ["count", "average", "tdigest"])
    schema = builder.build()

    engine = accreta.Engine(schema)

    # 2. Ingest a spread of latency samples across a few minutes, all for the same route.
    #    Nothing here is TDigest-specific — ingestion looks identical to any other measure.
    t0 = datetime(2026, 3, 15, 10, 0, 0, tzinfo=timezone.utc)
    latencies_ms = [
        12.0, 15.0, 11.0, 20.0, 14.0, 9.0, 100.0, 13.0, 16.0, 12.0,
        18.0, 250.0, 14.0, 15.0, 11.0, 13.0, 17.0, 12.0, 19.0, 500.0,
    ]
    for i, latency in enumerate(latencies_ms):
        engine.ingest(t0 + timedelta(minutes=i), [latency], ["/api/search"])

    # 3. Roll up to the hour, same as any other measure — TDigest merges through the rollup
    #    hierarchy exactly like the exact aggregates do, it just doesn't guarantee an exact
    #    answer at the end.
    engine.rollup()

    hour_start = t0.replace(minute=0, second=0, microsecond=0)
    hour_end = hour_start + timedelta(hours=1)
    latency_set = engine.query_range("hour", hour_start, hour_end, 0)

    values = latency_set.values("f64")
    count = values["count"]
    mean = values["average"]
    p50 = latency_set.quantile("tdigest", 0.50)
    p95 = latency_set.quantile("tdigest", 0.95)
    p99 = latency_set.quantile("tdigest", 0.99)

    print(f"samples ingested : {count}")
    print(f"exact mean       : {mean:.1f} ms")
    print(f"p50 (median)     : {p50:.1f} ms")
    print(f"p95              : {p95:.1f} ms")
    print(f"p99              : {p99:.1f} ms")

    # The mean is dragged upward by the outliers (100, 250, 500 ms) far more than the median
    # is — a good illustration of why you'd want both an exact mean *and* quantiles on the
    # same measure rather than relying on the mean alone to characterize latency.
    assert p50 < mean, "median should sit below the outlier-skewed mean"

    # 4. Approximate associativity, reached through the public API: reading p50 off the
    #    pre-computed hour bucket merges centroids in one shape (20 minute-buckets folded
    #    together during rollup()); reading p50 by querying "minute" level across the whole
    #    hour merges the same 20 buckets in a different shape (query-time merge, no rollup
    #    involved). Different merge path, same underlying samples — the module docs' point is
    #    that `quantile()` answers should agree closely even though the two merges aren't
    #    guaranteed to land on structurally identical digests.
    minute_set = engine.query_range("minute", hour_start, hour_end, 0)
    p50_via_minutes = minute_set.quantile("tdigest", 0.50)

    print(
        f"\np50 via hour rollup    : {p50:.2f} ms"
        f"\np50 via minute merge   : {p50_via_minutes:.2f} ms"
    )
    assert abs(p50 - p50_via_minutes) < 1.0, (
        "merge path shouldn't meaningfully change the quantile estimate"
    )


if __name__ == "__main__":
    main()
