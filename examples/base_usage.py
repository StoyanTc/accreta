"""Mirrors basic_usage.rs (Rust): schema, ingest, rollup, query, retention/prune.

One structural difference from the Rust example: this wrapper doesn't expose raw Bucket
iteration (Engine.buckets(level)) — see README's "Known limitations". Where the Rust example
iterates `engine.buckets(BucketLevel::Hour)` and reads each bucket's groups directly, this
example gets the equivalent per-hour breakdown via query_range_grouped() called once per hour
window instead. It also can't print resolved dimension values ("Firefox") — only the raw
DimensionValueId — for the same reason.
"""

from datetime import datetime, timedelta, timezone

import accreta


def main() -> None:
    # 1. Describe which aggregates every bucket should track.
    builder = accreta.SchemaBuilder()
    builder.dimension("browser")
    builder.measure("visits", "f64", ["sum", "count", "min", "max", "average"])
    schema = builder.build()

    engine = accreta.Engine(schema)

    # 2. Ingest raw samples. Every sample only ever touches a minute-level bucket.
    start = datetime(2026, 6, 1, 9, 0, tzinfo=timezone.utc)
    readings = [
        (0, 21.5, "Firefox"),
        (1, 22.0, "Firefox"),
        (3, 19.8, "Firefox"),
        (15, 23.1, "Firefox"),
        (47, 20.4, "Firefox"),
        (61, 25.0, "Firefox"),   # rolls into the next hour
        (62, 24.7, "Firefox"),
        (125, 18.9, "Firefox"),  # rolls into a third hour
    ]
    for minute_offset, value, dim_val in readings:
        engine.ingest(start + timedelta(minutes=minute_offset), [value], [dim_val])

    print(
        f"ingested {len(readings)} raw samples into "
        f"{engine.bucket_count('minute')} minute buckets"
    )

    # 3. Roll everything up. This never re-reads a sample — only merges bucket states upward.
    engine.rollup()

    # 4. Read aggregates back at whatever granularity is useful. Grouped by "browser" (the only
    #    dimension here), one query per hour window (see module docstring for why).
    print("\nPer-hour breakdown:")
    group_by = accreta.DimensionMask().with_dimension(0)  # dimension 0 = "browser"
    for hour in range(3):
        hour_start = start.replace(minute=0) + timedelta(hours=hour)
        hour_end = hour_start + timedelta(hours=1)
        grouped = engine.query_range_grouped(
            "hour", hour_start, hour_end, measure_index=0, group_by=group_by
        )
        for key, agg_set in grouped.items():
            values = agg_set.values("f64")
            print(
                f"  [dim_ids={key.values} {hour_start:%H:%M} .. {hour_end:%H:%M}) "
                f"sum={values['sum']:>6.2f} count={values['count']:<2} "
                f"min={values['min']:>5.2f} max={values['max']:>5.2f} "
                f"avg={values['average']:>5.2f}"
            )

    # 5. Whole-day total (rolled all the way up) — one merged group across the full day.
    print("\nWhole-day total (rolled all the way up):")
    day_start = start.replace(hour=0, minute=0)
    day_end = day_start + timedelta(days=1)
    day_total = engine.query_range("day", day_start, day_end, measure_index=0)
    values = day_total.values("f64")
    print(f"  count={values['count']} sum={values['sum']:.2f} average={values['average']:.2f}")

    # 6. Ad-hoc range queries merge whichever buckets already exist at a given level, without
    #    storing anything new — handy for "give me the last N hours" style queries.
    print("\nAd-hoc query for the first two hours only:")
    first_two_hours = engine.query_range(
        "hour", start, start + timedelta(hours=2), measure_index=0
    )
    values = first_two_hours.values("f64")
    print(f"  count={values['count']} sum={values['sum']:.2f}")

    # 7. Retention: an Engine keeps every bucket forever by default, which is fine for a bounded
    #    batch job like this one but not for long-running ingestion. `Retention` bounds memory
    #    at whichever levels you configure; `prune()` is the explicit, opt-in step that actually
    #    discards old buckets (rollup itself never deletes anything).
    print("\nRetention: keeping only the last hour of minute-level detail")
    retention_builder = accreta.SchemaBuilder()
    retention_builder.dimension("browser")
    retention_builder.measure("visits", "f64", ["sum", "count"])
    retention_schema = retention_builder.build()

    policy = accreta.Retention().keep("minute", max_age_hours=1)
    bounded_engine = accreta.Engine(retention_schema, retention=policy)
    for minute_offset, value, dim_val in readings:
        bounded_engine.ingest(start + timedelta(minutes=minute_offset), [value], [dim_val])

    print(f"  before prune: {bounded_engine.bucket_count('minute')} minute buckets")
    bounded_engine.prune()
    print(
        f"  after prune:  {bounded_engine.bucket_count('minute')} minute buckets "
        "(older than 1h before the newest sample were dropped)"
    )


if __name__ == "__main__":
    main()