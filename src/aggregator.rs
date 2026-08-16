//! The [`Aggregator`] trait: turning raw samples into monoid states.

use crate::{measures::FromValue, monoid::Monoid};

/// A [`Monoid`] state that additionally knows how to incorporate one raw
/// measure value.
///
/// `Aggregator` is generic over the numeric type of the measure it consumes.
/// This keeps integer measures as integers and floating-point measures as
/// floating-point values.
pub trait Aggregator: Monoid
where
    Self::Input: FromValue,
{
    type Input;

    /// The name this aggregate is registered and looked up under.
    const NAME: &'static str;

    /// Fold one raw measure value into this state.
    fn update(&self, value: Self::Input) -> Self
    where
        Self: Sized;

    /// Fold one raw measure value into this state in place.
    fn update_in_place(&mut self, value: Self::Input) {
        *self = self.update(value);
    }
}
