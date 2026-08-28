//! Running count of samples seen.

use crate::aggregator::Aggregator;
use crate::monoid::Monoid;

/// The number of samples seen so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Count(u64);

impl Count {
    /// The current count.
    pub fn value(&self) -> u64 {
        self.0
    }
}

impl Monoid for Count {
    fn identity() -> Self {
        Count(0)
    }

    fn merge(&self, other: &Self) -> Self {
        Count(self.0 + other.0)
    }
}

impl Aggregator for Count {
    type Input = ();

    const NAME: &'static str = "count";

    fn update(&self, _input: ()) -> Self {
        Self(self.0 + 1)
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn identity_is_zero() {
        assert_eq!(Count::identity().value(), 0);
    }

    #[test]
    fn update_increments() {
        let c = Count::identity().update(()).update(());
        assert_eq!(c.value(), 2);
    }

    #[test]
    fn merge_sums_counts() {
        assert_eq!(Count(3).merge(&Count(4)).value(), 7);
    }
}
