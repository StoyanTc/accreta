//! # accreta
//!
//! A reusable **mergeable-state aggregation framework**, with hierarchical time-series rollups
//! as its first application.
//!
//! ## The core idea
//!
//! Instead of re-scanning raw data every time you want a coarser summary (hourly from minutes,
//! daily from hours, ...), every aggregate is represented as a [`Monoid`]: a
//! state that can be built incrementally from samples ([`Aggregator`])
//! and combined with another state of the same kind
//! ([`Monoid::merge`]) to get the state you'd have gotten by seeing both
//! sets of data at once. Rollups then become pure state merges.
//!
//! The hierarchy is **not** a straight chain — `day` buckets roll up directly into *both*
//! `week` and `month`, and `week` is a dead end that never rolls up any further (only `month`
//! feeds `year`):
//!
//! ```text
//!                                                          +--merge--> week buckets   (dead end)
//!                                                          |
//! minute --merge--> hour --merge--> day buckets -----------+
//!  buckets           buckets                                |
//!                                                            +--merge--> month buckets --merge--> year buckets
//! ```
//!
//! See [`BucketLevel::rollup_targets`] for the exact, authoritative fan-out at each level, and
//! [`BucketLevel::parent`] for the (different!) notion of which level a bucket's *default*
//! parent is — the two aren't the same thing for `day`, so don't assume `parent()` tells you
//! everywhere a level's data ends up.
//!
//! Raw [`Sample`]s are only ever folded into the finest ([`BucketLevel::Minute`])
//! buckets. Every coarser bucket is derived *exclusively* by merging finer buckets — the engine
//! never reprocesses raw data to compute a rollup.
//!
//! ## Dimension keys and GROUP BY
//!
//! A [`Bucket`] stores the **full** dimension key for every group it holds, not just the subset
//! a particular query cares about. This means a query can later group by an arbitrary subset of
//! the schema's dimensions — via [`crate::dimensions::DimensionKey::project`] — without
//! re-aggregating raw data or materializing every possible projection up front. The cost of this
//! flexibility is paid once, at ingest time, by however many distinct full-dimension
//! combinations actually occur in your data.
//!
//! ## Module map
//!
//! | Module | Responsibility |
//! |---|---|
//! | [`sample`] | The raw measurements that come in |
//! | [`monoid`] | The `Monoid` trait: how two states combine |
//! | [`aggregator`] | The `Aggregator` trait: how one sample folds into a state |
//! | [`erased`] | Type-erasure so heterogeneous aggregates can share a collection |
//! | [`aggregates`] | Built-in aggregates: `Sum`, `Count`, `Min`, `Max`, `Average` |
//! | [`aggregate_set`] | `Schema` + `AggregateSet`: a named collection of states |
//! | [`bucket`] | `BucketLevel` + `Bucket`: a time window holding an `AggregateSet` per dimension group |
//! | [`retention`] | `Retention`: how long buckets are kept at each level |
//! | [`engine`] | `Engine`: owns the bucket hierarchy, drives ingestion, rollup, and pruning |
//!
//! ## Adding a new aggregate
//!
//! Nothing above [`aggregates`] needs to change. Implement [`Monoid`] (identity
//! and merge) and [`Aggregator`] (fold in one sample) for a new state type,
//! then register it on a measure in a [`Schema`] alongside the built-ins:
//!
//! ```
//! use accreta::aggregate_set::Schema;
//! use accreta::aggregates::{Count, Sum};
//!
//! let mut builder = Schema::builder();
//! builder
//!     .dimension("host")
//!     .measure("value")
//!     .with::<Sum<f64>>()
//!     .with_any::<Count>();
//! let schema = builder.build().unwrap();
//! ```
//!
//! See the `custom_aggregate` example for a complete worked implementation (a running Variance
//! computed via Welford's algorithm), and [`aggregates::Average`] for an example of
//! building a derived aggregate out of two existing ones.
//!
//! ## Quick start
//!
//! `Schema::builder().build()` and `Engine::ingest` return `Result`s rather than panicking — a
//! schema with no registered dimension or measure fails to build with
//! [`crate::errors::SchemaError::NoDimensions`] / [`crate::errors::SchemaError::NoMeasures`]
//! respectively. The example below `.unwrap()`s throughout for brevity; production code should
//! handle these explicitly.
//!
//! ```
//! use chrono::{TimeZone, Utc};
//! use accreta::aggregate_set::Schema;
//! use accreta::aggregates::{Count, Sum};
//! use accreta::bucket::BucketLevel;
//! use accreta::engine::Engine;
//! use accreta::measures::MeasureId;
//!
//! let mut builder = Schema::builder();
//! builder
//!     .dimension("host")
//!     .measure("value")
//!     .with::<Sum<f64>>()
//!     .with_any::<Count>();
//! let schema = builder.build().unwrap();
//!
//! let mut engine = Engine::new(schema);
//!
//! let t0 = Utc.with_ymd_and_hms(2026, 3, 15, 10, 5, 0).unwrap();
//! engine.ingest(t0, [12.0], ["server-a"]).unwrap();
//! engine
//!     .ingest(t0 + chrono::Duration::minutes(1), [8.0], ["server-a"])
//!     .unwrap();
//!
//! // Rollups happen purely by merging bucket states upward.
//! engine.rollup();
//!
//! let hour_start = BucketLevel::Hour.truncate(t0);
//! let hour = engine.bucket(BucketLevel::Hour, hour_start).unwrap();
//! let (_, sets) = hour.groups().next().unwrap();
//! let value = &sets[MeasureId(0).index()];
//!
//! assert_eq!(value.get::<Sum<f64>>().unwrap().value(), 20.0);
//! assert_eq!(value.get::<Count>().unwrap().value(), 2);
//! ```

pub mod aggregate_set;
pub mod aggregates;
pub mod aggregator;
pub mod bucket;
pub mod dimensions;
pub mod engine;
pub mod erased;
pub mod errors;
pub mod measures;
pub mod monoid;
pub mod retention;
pub mod sample;

pub use aggregate_set::{AggregateSet, Schema, SchemaBuilder};
pub use aggregator::Aggregator;
pub use bucket::{Bucket, BucketLevel};
pub use dimensions::{
    DimensionDictionary, DimensionId, DimensionKey, DimensionMask, DimensionValueId,
    DimensionValues,
};
pub use engine::Engine;
pub use monoid::Monoid;
pub use retention::Retention;
pub use sample::Sample;
