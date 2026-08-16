//! Running maximum.

use crate::aggregator::Aggregator;
use crate::measures::{FromValue, MeasureNumber};
use crate::monoid::Monoid;

/// The largest sample value seen so far.
///
/// The identity element is `-infinity`, and [`Max::value`] returns `None` if no sample has ever
/// been folded in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Max<T>(Option<T>);

impl<T> Max<T>
where
    T: Copy,
{
    /// The current maximum, or `None` if no value has been absorbed.
    pub fn value(&self) -> Option<T> {
        self.0
    }
}

impl<T> Default for Max<T>
where
    T: MeasureNumber,
{
    fn default() -> Self {
        Self(None)
    }
}

impl<T> Monoid for Max<T>
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
            (Some(a), Some(b)) => Self(Some(if a >= b { a } else { b })),
        }
    }
}

impl<T> Aggregator for Max<T>
where
    T: MeasureNumber + FromValue + PartialOrd,
{
    type Input = T;

    const NAME: &'static str = "max";

    fn update(&self, value: T) -> Self {
        match self.0 {
            None => Self(Some(value)),
            Some(current) => Self(Some(if value > current { value } else { current })),
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn identity_has_no_value() {
        assert_eq!(Max::<f64>::identity().value(), None);
    }

    #[test]
    fn update_tracks_maximum() {
        let m = Max::identity().update(5.0).update(-2.0).update(10.0);
        assert_eq!(m.value(), Some(10.0));
    }

    #[test]
    fn merge_takes_larger() {
        let a = Max(Some(3.0));
        let b = Max(Some(9.0));
        assert_eq!(a.merge(&b).value(), Some(9.0));
    }

    #[test]
    fn merge_with_identity_is_noop() {
        let a = Max(Some(3.0));
        assert_eq!(a.merge(&Max::identity()).value(), a.value());
    }
}
