//! Running minimum.

use crate::aggregator::Aggregator;
use crate::measures::{FromValue, MeasureNumber};
use crate::monoid::Monoid;

/// The smallest sample value seen so far.
///
/// The identity element is `+infinity` (merging with an empty bucket never lowers the minimum),
/// and [`Min::value`] returns `None` if no sample has ever been folded in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Min<T>(Option<T>);

impl<T> Min<T>
where
    T: Copy,
{
    /// The current minimum, or `None` if this state has never absorbed a sample.
    pub fn value(&self) -> Option<T> {
        self.0
    }
}

impl<T> Default for Min<T>
where
    T: MeasureNumber,
{
    fn default() -> Self {
        Self(None)
    }
}

impl<T> Monoid for Min<T>
where
    T: MeasureNumber + PartialOrd,
{
    fn identity() -> Self {
        Self(None)
    }

    fn merge(&self, other: &Self) -> Self {
        match (self.0, other.0) {
            (None, None) => Self(None),
            (Some(value), None) => Self(Some(value)),
            (None, Some(value)) => Self(Some(value)),
            (Some(a), Some(b)) => Self(Some(if a <= b { a } else { b })),
        }
    }
}

impl<T> Aggregator for Min<T>
where
    T: MeasureNumber + FromValue + PartialOrd,
{
    type Input = T;
    const NAME: &'static str = "min";

    fn update(&self, value: T) -> Self {
        match self.0 {
            None => Self(Some(value)),
            Some(current) => Self(Some(if value < current { value } else { current })),
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn identity_has_no_value() {
        assert_eq!(Min::<f64>::identity().value(), None);
    }

    #[test]
    fn update_tracks_minimum() {
        let m = Min::identity().update(5.0).update(-2.0).update(10.0);
        assert_eq!(m.value(), Some(-2.0));
    }

    #[test]
    fn merge_takes_smaller() {
        let a = Min(Some(3.0));
        let b = Min(Some(-1.0));
        assert_eq!(a.merge(&b).value(), Some(-1.0));
    }

    #[test]
    fn merge_with_identity_is_noop() {
        let a = Min(Some(3.0));
        assert_eq!(a.merge(&Min::identity()).value(), a.value());
    }
}
