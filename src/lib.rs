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
//! sets of data at once. Rollups then become pure state merges:
//!
//! ```text
//! minute buckets --merge--> hour buckets --merge--> day buckets --merge--> ... --> year buckets
//! ```
//!
//! Raw [`Sample`]s are only ever folded into the finest ([`BucketLevel::Minute`])
//! buckets. Every coarser bucket is derived *exclusively* by merging finer buckets — the engine
//! never reprocesses raw data to compute a rollup.
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
