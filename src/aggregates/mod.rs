//! Built-in aggregate implementations.
//!
//! Each aggregate here implements just [`crate::monoid::Monoid`] and
//! [`crate::aggregator::Aggregator`] for a small state struct. That's the entire contract:
//! nothing in [`crate::engine`], [`crate::bucket`], or [`crate::aggregate_set`] needs to change
//! to support a new one. See the crate-level docs and the `custom_aggregate` example for how to
//! add your own (e.g. Variance, t-Digest, HyperLogLog, Bloom filter, Top-K).

mod average;
mod count;
mod max;
mod min;
mod sum;

pub use average::Average;
pub use count::Count;
pub use max::Max;
pub use min::Min;
pub use sum::Sum;
