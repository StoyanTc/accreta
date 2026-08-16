//! Type erasure that lets heterogeneous [`Monoid`]/[`Aggregator`] implementations live side by
//! side in one [`crate::aggregate_set::AggregateSet`].
//!
//! [`Monoid::identity`] returns `Self`, which makes `Monoid` (and `Aggregator`) object-safe only
//! up to a point: we can't call `identity()` on a `dyn Monoid`. To support a *registry* of
//! aggregates that's decided at runtime (so the engine architecture never has to change when a
//! new aggregate type is added), this module defines an object-safe [`ErasedState`] trait with a
//! blanket implementation for every concrete `T: Aggregator`, plus a companion
//! [`AggregateFactory`] that knows how to produce fresh identity states.

use std::any::Any;
use std::fmt::Debug;

use crate::aggregator::Aggregator;
use crate::measures::{FromValue, MeasureValue};
use crate::monoid::Monoid;

/// Object-safe, type-erased view of an [`Aggregator`] state.
///
/// You will not normally implement this yourself — it has a blanket implementation for every
/// `T: Aggregator`. It exists so an [`crate::aggregate_set::AggregateSet`] can hold a
/// `Box<dyn ErasedState>` per named aggregate without the engine knowing (or needing to know)
/// the concrete type.
///
/// The mutating methods here (`update_erased_in_place`, `merge_erased_in_place`) update the
/// state behind `&mut self` rather than returning a freshly boxed replacement. That distinction
/// matters on the hot path: [`AggregateSet::update`](crate::aggregate_set::AggregateSet::update)
/// is called once per aggregate per incoming sample, so an allocating `update` would mean one
/// heap allocation per aggregate per sample. Mutating in place turns that into zero allocations
/// per sample after the state's `Box` first exists.
pub trait ErasedState: Debug + Send + Sync {
    /// Fold a raw sample into this state in place.
    fn update_erased_in_place(&mut self, value: MeasureValue);

    /// Merge another erased state of the same underlying type into this one, in place.
    ///
    /// # Panics
    ///
    /// Panics if `other` does not wrap the same concrete type as `self`. Because states are
    /// always produced from a single [`AggregateFactory`] per slot, and every bucket for a given
    /// engine is built from the same set of factories, this should never happen in practice; see
    /// [`crate::aggregate_set::AggregateSet::merge`] for the invariant that guarantees it.
    fn merge_erased_in_place(&mut self, other: &dyn ErasedState);

    /// Access the concrete type for downcasting, e.g. via
    /// [`AggregateSet::get`](crate::aggregate_set::AggregateSet::get).
    fn as_any(&self) -> &dyn Any;

    /// Clone this state into a new box.
    fn clone_erased(&self) -> Box<dyn ErasedState>;
}

impl<T> ErasedState for T
where
    T: Aggregator + Clone + 'static,
{
    fn update_erased_in_place(&mut self, value: MeasureValue) {
        let value = T::Input::from_value(value).unwrap_or_else(|| {
            panic!("aggregate '{}' received incompatible measure type", T::NAME)
        });

        Aggregator::update_in_place(self, value);
    }

    fn merge_erased_in_place(&mut self, other: &dyn ErasedState) {
        let other = other
            .as_any()
            .downcast_ref::<T>()
            .expect("merge_erased_in_place called with mismatched aggregate types");
        // `Monoid::merge` itself returns a new value rather than mutating in place (that's the
        // right shape for the public `Monoid` trait — merging two `&self` states has no obvious
        // owner to mutate). The erased wrapper is where we can safely take that one unavoidable
        // stack-local `T` and write it over the existing `Box`'s storage instead of allocating a
        // new one: `*self = ...` reuses this box's already-allocated memory.
        *self = Monoid::merge(self, other);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn clone_erased(&self) -> Box<dyn ErasedState> {
        Box::new(self.clone())
    }
}

impl Clone for Box<dyn ErasedState> {
    fn clone(&self) -> Self {
        self.clone_erased()
    }
}

/// A factory that produces fresh identity states for one named aggregate.
///
/// An [`AggregateSet`](crate::aggregate_set::AggregateSet) is configured with a slice of these
/// (its "schema"). Every bucket the engine creates — at every hierarchy level — builds its
/// `AggregateSet` from the same factories, which is what lets [`ErasedState::merge_erased_in_place`]
/// safely assume matching concrete types.
#[derive(Clone)]
pub struct AggregateFactory {
    name: &'static str,
    make: fn() -> Box<dyn ErasedState>,
}

impl AggregateFactory {
    /// Build a factory for aggregate type `T`, registered under its own [`Aggregator::NAME`].
    ///
    /// There is deliberately no `name` parameter here: accepting one would reopen exactly the
    /// hole this type exists to close (a caller registering, say, a `Count` under the string
    /// `"sum"`). The name always comes from the type itself.
    pub fn new<A>() -> Self
    where
        A: Aggregator + Clone + 'static,
    {
        Self {
            name: A::NAME,
            make: || Box::new(A::identity()),
        }
    }

    /// The registered name of this aggregate.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Produce a fresh identity state.
    pub fn identity(&self) -> Box<dyn ErasedState> {
        (self.make)()
    }
}

impl Debug for AggregateFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AggregateFactory")
            .field("name", &self.name)
            .finish()
    }
}
