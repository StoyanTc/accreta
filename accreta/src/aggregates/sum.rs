//! Running sum.

use std::ops::Add;

use crate::aggregator::Aggregator;
use crate::measures::{FromValue, MeasureNumber};
use crate::monoid::Monoid;

/// The sum of every sample value seen so far.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Sum<T>(T);

impl<T> Sum<T>
where
    T: MeasureNumber + Add<Output = T>,
{
    /// The current running sum.
    pub fn value(&self) -> T {
        self.0
    }
}

impl<T> Monoid for Sum<T>
where
    T: MeasureNumber + Add<Output = T>,
{
    fn identity() -> Self {
        Sum(T::default())
    }

    fn merge(&self, other: &Self) -> Self {
        Sum(self.0 + other.0)
    }
}

impl<T> Aggregator for Sum<T>
where
    T: MeasureNumber + FromValue + Default + Add<Output = T>,
{
    type Input = T;

    const NAME: &'static str = "sum";

    fn update(&self, value: T) -> Self {
        Self(self.0 + value)
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn identity_is_zero() {
        assert_eq!(Sum::<f64>::identity().value(), 0.0);
    }

    #[test]
    fn update_accumulates() {
        let s = Sum::<f64>::identity().update(3.0).update(4.0);
        assert_eq!(s.value(), 7.0);
    }

    #[test]
    fn merge_is_associative_and_commutative() {
        let a = Sum(1.0);
        let b = Sum(2.0);
        let c = Sum(3.0);
        assert_eq!(a.merge(&b).merge(&c).value(), a.merge(&b.merge(&c)).value());
        assert_eq!(a.merge(&b).value(), b.merge(&a).value());
    }

    #[test]
    fn merge_with_identity_is_noop() {
        let a = Sum(5.5);
        assert_eq!(a.merge(&Sum::identity()).value(), a.value());
    }
}
