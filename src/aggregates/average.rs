//! Running average, built by composing the [`Sum`] and [`Count`] monoids.
//!
//! This is a worked example of the general pattern for building a new aggregate on top of
//! existing ones: an average is not itself independently mergeable (you can't merge two averages
//! without knowing how many samples each was over), but `(sum, count)` is, and the average is a
//! cheap derived view of that pair.

use std::ops::Add;

use crate::measures::FromValue;
use crate::monoid::Monoid;
use crate::{aggregator::Aggregator, measures::MeasureNumber};

use super::{Count, Sum};

/// The arithmetic mean of every sample value seen so far, tracked as a `(sum, count)` pair so
/// that merging is exact regardless of how many pieces are combined or in what order.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Average<T> {
    sum: Sum<T>,
    count: Count,
}

impl<T> Average<T>
where
    T: MeasureNumber + Add<Output = T>,
{
    /// The running sum backing this average.
    pub fn sum(&self) -> T {
        self.sum.value()
    }

    /// The running count backing this average.
    pub fn count(&self) -> u64 {
        self.count.value()
    }
}

impl<T> Monoid for Average<T>
where
    T: MeasureNumber + Add<Output = T>,
{
    fn identity() -> Self {
        Average {
            sum: Sum::identity(),
            count: Count::identity(),
        }
    }

    fn merge(&self, other: &Self) -> Self {
        Average {
            sum: self.sum.merge(&other.sum),
            count: self.count.merge(&other.count),
        }
    }
}

impl<T> Aggregator for Average<T>
where
    T: MeasureNumber + FromValue + Add<Output = T>,
{
    const NAME: &'static str = "average";
    type Input = T;

    fn update(&self, value: T) -> Self {
        Average {
            sum: self.sum.update(value),
            count: self.count.update(()),
        }
    }
    /*  */
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn update_tracks_mean() {
        let a = Average::identity().update(2.0).update(4.0).update(9.0);
        assert_eq!(a.sum() / (a.count() as f64), 5.0);
    }

    #[test]
    fn merge_is_weighted_correctly() {
        // 2 samples averaging 10, merged with 1 sample of 1 -> mean should be (20 + 1) / 3.
        let a = Average::identity().update(10.0).update(10.0);
        let b = Average::identity().update(1.0);
        let merged = a.merge(&b);
        assert_eq!(merged.sum(), 21.0);
        assert_eq!(merged.count(), 3);
    }

    #[test]
    fn merge_with_identity_is_noop() {
        let a = Average::identity().update(7.0);
        assert_eq!(a.merge(&Average::identity()), a);
    }
}
