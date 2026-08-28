//! The [`Monoid`] trait: the fundamental abstraction of this crate.
//!
//! Everything else — [`crate::aggregator::Aggregator`], [`crate::aggregate_set::AggregateSet`],
//! [`crate::bucket::Bucket`], and [`crate::engine::Engine`] — is built on top of the idea that
//! an aggregate's *state* can always be:
//!
//! 1. Created from nothing ([`Monoid::identity`]), and
//! 2. Combined with another state of the same type ([`Monoid::merge`]),
//!
//! such that the combination is **associative** and the identity is a true two-sided identity
//! element. Those two laws are what let the engine roll a year's worth of minute-level buckets
//! up into a single yearly bucket purely by merging states, without ever touching a raw sample
//! again.
//!
//! # The Monoid laws
//!
//! Implementers must uphold:
//!
//! - **Identity**: `x.merge(&T::identity()) == x` and `T::identity().merge(&x) == x`
//! - **Associativity**: `a.merge(&b).merge(&c) == a.merge(&b.merge(&c))`
//!
//! Commutativity (`a.merge(&b) == b.merge(&a)`) is *not* required by the trait, but every
//! aggregate shipped in this crate happens to be commutative, which is convenient for
//! time-series data where merge order (e.g. bucket iteration order) shouldn't matter.

use std::fmt::Debug;

/// A mergeable aggregate state.
///
/// A `Monoid` is a snapshot of "everything we know so far" for some aggregate (a running sum, a
/// running count, a min/max, a sketch, ...). Two states of the same aggregate — computed from
/// two disjoint sets of samples — can always be [`merge`](Monoid::merge)d into a state
/// equivalent to having seen the union of both sample sets, without re-reading either set.
///
/// This trait intentionally says nothing about *how* a state is produced from raw samples; that
/// is the job of [`crate::aggregator::Aggregator`]. `Monoid` only describes how two already-built
/// states combine.
pub trait Monoid: Clone + Debug + Send + Sync + 'static {
    /// The identity element: `x.merge(&identity()) == x` for all `x`.
    ///
    /// Represents "no data seen yet" (an empty bucket).
    fn identity() -> Self
    where
        Self: Sized;

    /// Combine two states into one representing the union of the data each summarizes.
    ///
    /// Must be associative. Implementers should generally also make this commutative, since
    /// bucket rollups do not guarantee merge order.
    fn merge(&self, other: &Self) -> Self;

    /// Merge `other` into `self` in place. The default implementation just calls
    /// [`merge`](Monoid::merge); override it if an in-place merge can avoid an allocation.
    fn merge_in_place(&mut self, other: &Self) {
        *self = self.merge(other);
    }
}
