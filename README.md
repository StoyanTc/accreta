# accreta

*From the Latin* accrescere*, "to grow."*

A Rust **engine for mergeable-state aggregation** — pluggable aggregates via a small
trait-based framework — with hierarchical time-series rollups as its first application.

> **Note:** this README assumes the package is published as `accreta` (matching the imports used
> in `examples/`) and defaults to a dual MIT/Apache-2.0 license, the Rust ecosystem convention —
> adjust the [License](#license) section and the version number below if that's not accurate.

## Why accreta

Instead of re-scanning raw data every time you want a coarser summary (hourly from minutes,
daily from hours, ...), accreta keeps every summary in a form that can be combined with another
summary of the same kind to get the answer you'd have gotten by seeing both at once. Rollups
become cheap merges of existing state.

The hierarchy is **not** a straight chain, though — `day` buckets roll up directly into *both*
`week` and `month`, and `week` is a dead end that never rolls up any further (only `month` feeds
`year`):

```text
                                                         +--merge--> week buckets   (dead end)
                                                         |
minute --merge--> hour --merge--> day buckets -----------+
 buckets           buckets                               |
                                                         +--merge--> month buckets --merge--> year buckets
```

See `BucketLevel::rollup_targets` for the exact, authoritative fan-out at each level.

Raw samples are only ever folded into the finest (`Minute`) buckets. Every coarser bucket is
derived *exclusively* by merging finer buckets — the engine never reprocesses raw data to
compute a rollup, and `rollup()` is idempotent to call as often as you like.

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

// 1. Describe what you're tracking: dimensions to group by, and which
//    built-in aggregates each measure should keep.
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

// 2. Feed in raw samples as they arrive.
let t0 = Utc.with_ymd_and_hms(2026, 3, 15, 10, 5, 0).unwrap();
engine.ingest(t0, vec![12.0], vec!["Firefox"]).unwrap();
engine
    .ingest(t0 + Duration::minutes(1), vec![8.0], vec!["Firefox"])
    .unwrap();

// 3. Roll up whenever you want coarser buckets. Cheap — it only merges
//    what's already there, it never re-reads your samples.
engine.rollup();

// 4. Read back at whatever granularity you need.
let hour_start = BucketLevel::Hour.truncate(t0);
let hour = engine.bucket(BucketLevel::Hour, hour_start).unwrap();
let (_, sets) = hour.groups().next().unwrap();
let visits = &sets[0];

assert_eq!(visits.get::<Sum<f64>>().unwrap().value(), 20.0);
assert_eq!(visits.get::<Count>().unwrap().value(), 2);
```

That's the whole workflow: describe your measures, ingest, roll up, read. You don't need to know
anything about how the merging works under the hood to use the built-in aggregates
(`Sum`, `Count`, `Min`, `Max`, `Average`).

You can also query a time range directly, merging whichever buckets already exist at a level
without storing anything new:

```rust,ignore
let totals = engine
    .query_range(BucketLevel::Hour, range_start, range_end, measure_id)?;

let grouped = engine
    .query_range_grouped(BucketLevel::Hour, range_start, range_end, measure_id, group_by)?;
```

See [`examples/basic_usage.rs`](examples/basic_usage.rs) for the complete walkthrough, including
ad-hoc queries and retention.

## Features

- **Hierarchical rollups** — a fixed `Minute -> Hour -> Day -> Week -> Month -> Year` set of
  levels, but the rollup path between them fans out rather than chaining straight through: `day`
  feeds both `week` and `month` directly, `week` never rolls up any further, and `month` feeds
  `year`. See `BucketLevel::rollup_targets` for the exact fan-out at each level.
- **Pluggable aggregates** — `Sum`, `Count`, `Min`, `Max`, and `Average` ship built in; you can
  add your own (e.g. running variance) without changing anything in the engine — see
  [Adding a custom aggregate](#adding-a-custom-aggregate) below.
- **Dimension-based grouping** — up to 64 dimensions per schema. Every bucket stores the *full*
  dimension key, so you can query any `GROUP BY` combination later without precomputing every
  possible projection up front.
- **Type-safe measures** — `i64`, `u64`, and `f64` measure values are checked against the schema
  when you ingest them.
- **Bounded memory** — an optional `Retention` policy plus an explicit `prune()` step for
  long-running ingestion; rollups themselves never delete anything.
- **No hot-path allocation** — folding a sample into an existing bucket allocates nothing.

## Adding a custom aggregate

If the built-ins don't cover what you need, you can add your own. Under the hood, every
aggregate is a **monoid**: a state with an identity value and a way to merge two states of the
same kind together. Implement that (`Monoid`), implement how a single sample updates the state
(`Aggregator`), and register it on a measure exactly like a built-in — nothing in `engine`,
`bucket`, or `aggregate_set` needs to change:

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
