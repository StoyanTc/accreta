# accreta

A **mergeable-state aggregation framework** for Rust, with hierarchical time-series rollups as
its first application.

> **Note:** this README assumes the package is published as `accreta` (matching the imports used
> in `examples/`) and defaults to a dual MIT/Apache-2.0 license, the Rust ecosystem convention —
> adjust the [License](#license) section and the version number below if that's not accurate.

## The core idea

Instead of re-scanning raw data every time you want a coarser summary (hourly from minutes,
daily from hours, ...), every aggregate is represented as a `Monoid`: a state that can be built
incrementally from samples (`Aggregator`) and combined with another state of the same kind
(`Monoid::merge`) to get the state you'd have gotten by seeing both sets of data at once.
Rollups then become pure state merges:

```text
minute buckets --merge--> hour buckets --merge--> day buckets --merge--> ... --> year buckets
```

Raw samples are only ever folded into the finest (`Minute`) buckets. Every coarser bucket is
derived *exclusively* by merging finer buckets — the engine never reprocesses raw data to
compute a rollup, and `rollup()` is idempotent to call as often as you like.

## Features

- **Hierarchical rollups** — a fixed `Minute -> Hour -> Day -> Week -> Month -> Year` bucket
  hierarchy, each level derived by merging the level below.
- **Pluggable aggregates** — `Sum`, `Count`, `Min`, `Max`, and `Average` ship built in; adding a
  new one (e.g. running variance) is a matter of implementing `Monoid` + `Aggregator` for your
  own type and registering it on a `Schema` — nothing in `engine`, `bucket`, or `aggregate_set`
  needs to change. See [`examples/custom_aggregate.rs`](examples/custom_aggregate.rs).
- **Dimension-based grouping** — up to 64 dimensions per schema. Every bucket stores the *full*
  dimension key, so a query can project onto any `GROUP BY` combination later without
  materializing every possible projection at ingestion time.
- **Type-safe measures** — `i64`, `u64`, and `f64` measure values are validated against the
  schema at ingestion time.
- **Bounded memory** — an optional `Retention` policy plus an explicit `prune()` step for
  long-running ingestion; rollups themselves never delete anything.
- **No hot-path allocation** — aggregate states are updated and merged in place behind a
  `Box<dyn ErasedState>`, so folding a sample into an existing bucket allocates nothing.

## Installation

```toml
[dependencies]
accreta = "0.1"
```

## Quick start

```rust
use accreta::aggregate_set::Schema;
use accreta::aggregates::{Average, Count, Max, Min, Sum};
use accreta::bucket::BucketLevel;
use accreta::engine::Engine;
use chrono::{Duration, TimeZone, Utc};

// 1. Describe the schema: dimensions to group by, and the aggregates each measure tracks.
let mut builder = Schema::builder();
builder
    .dimension("browser")
    .measure("visits")
    .with::<Sum<f64>>()
    .with_any::<Count>()
    .with::<Min<f64>>()
    .with::<Max<f64>>()
    .with::<Average<f64>>();
let schema = builder.build().unwrap();

let mut engine = Engine::new(schema);

// 2. Ingest raw samples — each one only ever touches a minute-level bucket.
let t0 = Utc.with_ymd_and_hms(2026, 3, 15, 10, 5, 0).unwrap();
engine.ingest(t0, vec![12.0], vec!["Firefox"]).unwrap();
engine
    .ingest(t0 + Duration::minutes(1), vec![8.0], vec!["Firefox"])
    .unwrap();

// 3. Roll up. This only ever merges bucket states — it never re-reads a sample.
engine.rollup();

// 4. Read aggregates back at whatever granularity you need.
let hour_start = BucketLevel::Hour.truncate(t0);
let hour = engine.bucket(BucketLevel::Hour, hour_start).unwrap();
let (_, sets) = hour.groups().next().unwrap();
let visits = &sets[0];

assert_eq!(visits.get::<Sum<f64>>().unwrap().value(), 20.0);
assert_eq!(visits.get::<Count>().unwrap().value(), 2);
```

Ad-hoc range queries merge whichever buckets already exist at a given level without storing
anything new:

```rust,ignore
let totals = engine
    .query_range(BucketLevel::Hour, range_start, range_end, measure_id)?;

let grouped = engine
    .query_range_grouped(BucketLevel::Hour, range_start, range_end, measure_id, group_by)?;
```

See [`examples/basic_usage.rs`](examples/basic_usage.rs) for the complete walkthrough, including
ad-hoc queries and retention.

## Adding a custom aggregate

Nothing in `engine`, `bucket`, or `aggregate_set` needs to change to add a new aggregate.
Implement `Monoid` (an identity value plus a merge function) and `Aggregator` (how one sample
folds into that state), then register it on a measure exactly like a built-in:

```rust,ignore
struct Variance<T> { /* ... */ }

impl<T> Monoid for Variance<T> { /* identity() + merge() */ }
impl<T> Aggregator for Variance<T> {
    const NAME: &'static str = "variance";
    type Input = T;
    fn update(&self, sample: T) -> Self { /* ... */ }
}

builder.measure("visits").with_any::<Count>().with::<Variance<f64>>();
```

`examples/custom_aggregate.rs` works through a complete, numerically stable running-variance
implementation using Welford's algorithm and its parallel-merge variant, including a worked
example that verifies the merge path and the single-update path agree.

## Retention and pruning

By default an `Engine` keeps every bucket it ever creates, at every level, forever — fine for a
bounded batch job, but not for long-running ingestion. Configure a `Retention` policy per level
and call `prune()` explicitly (typically right after `rollup()`) to bound memory:

```rust,ignore
let policy = accreta::Retention::new().keep(BucketLevel::Minute, Duration::hours(1));
let mut engine = Engine::with_retention(schema, policy);
// ... ingest, rollup ...
engine.prune();
```

"Older" is measured from the newest bucket currently stored *at that level*, not wall-clock
time, so this behaves the same way for live ingestion and for replaying historical data. Levels
with no configured limit are left untouched. `rollup()` never deletes anything — `prune()` is
the only place data leaves the engine.

## Module map

| Module | Responsibility |
|---|---|
| `sample` | The raw measurements that come in |
| `monoid` | The `Monoid` trait: how two states combine |
| `aggregator` | The `Aggregator` trait: how one sample folds into a state |
| `erased` | Type-erasure so heterogeneous aggregates can share a collection |
| `aggregates` | Built-in aggregates: `Sum`, `Count`, `Min`, `Max`, `Average` |
| `aggregate_set` | `Schema` + `AggregateSet`: a named collection of states |
| `dimensions` | `DimensionId`, `DimensionMask`, `DimensionKey`, and their dictionaries |
| `measures` | `MeasureId`, `MeasureType`, `MeasureValue`, and the `MeasureNumber`/`FromValue` traits |
| `bucket` | `BucketLevel` + `Bucket`: a time window holding an `AggregateSet` per dimension group |
| `retention` | `Retention`: how long buckets are kept at each level |
| `engine` | `Engine`: owns the bucket hierarchy, drives ingestion, rollup, and pruning |

## Examples

```sh
cargo run --example basic_usage
cargo run --example custom_aggregate
```

## Testing

```sh
cargo test
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
