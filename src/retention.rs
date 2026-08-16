//! [`Retention`]: how long buckets are kept at each [`BucketLevel`] before being discarded.

use chrono::Duration;

use crate::bucket::BucketLevel;

/// A retention policy for [`crate::engine::Engine`], independent of any particular bucket's
/// storage representation.
///
/// This exists because efficient per-bucket storage doesn't, by itself, bound memory use: an
/// engine that never discards a [`Bucket`](crate::bucket::Bucket) grows without limit for as
/// long as samples keep arriving, regardless of how cheaply each individual bucket is
/// represented. `Retention` says, per level, how long a bucket is worth keeping once it stops
/// being the newest data at that level; [`Engine::prune`](crate::engine::Engine::prune) is what
/// actually acts on it.
///
/// Every level keeps its buckets forever by default — an `Engine` with no retention configured
/// behaves exactly as before this existed. Configuring only the finest levels is a common
/// pattern: keep raw `Minute` resolution for a day or a week for detailed recent lookback, and
/// let coarser rollups (`Day`, `Month`, `Year`, ...) accumulate indefinitely, since there are
/// vastly fewer of them.
#[derive(Debug, Clone, Copy, Default)]
pub struct Retention {
    max_age: [Option<Duration>; BucketLevel::ALL.len()],
}

impl Retention {
    /// A policy that keeps every level forever (the default).
    pub fn new() -> Self {
        Self::default()
    }

    /// Keep buckets at `level` for at most `max_age` past the *newest* bucket currently stored at
    /// that level, rather than past the wall-clock time [`Engine::prune`](crate::engine::Engine::prune)
    /// happens to run at.
    ///
    /// Measuring age from the newest known bucket, not from wall-clock `now`, is what makes this
    /// correct for historical backfill or replay, not just live ingestion: a batch job replaying
    /// a year-old CSV file gets the same relative retention window as a live stream would, with
    /// no dependency on when `prune` happens to actually be called.
    pub fn keep(mut self, level: BucketLevel, max_age: Duration) -> Self {
        self.max_age[level as usize] = Some(max_age);
        self
    }

    /// The configured retention window for `level`, if any.
    pub(crate) fn max_age_for(&self, level: BucketLevel) -> Option<Duration> {
        self.max_age[level as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unconfigured_levels_have_no_limit() {
        let retention = Retention::new();
        assert_eq!(retention.max_age_for(BucketLevel::Minute), None);
    }

    #[test]
    fn keep_sets_only_the_requested_level() {
        let retention = Retention::new().keep(BucketLevel::Minute, Duration::hours(24));
        assert_eq!(
            retention.max_age_for(BucketLevel::Minute),
            Some(Duration::hours(24))
        );
        assert_eq!(retention.max_age_for(BucketLevel::Hour), None);
    }

    #[test]
    fn keep_can_configure_multiple_levels_by_chaining() {
        let retention = Retention::new()
            .keep(BucketLevel::Minute, Duration::hours(24))
            .keep(BucketLevel::Hour, Duration::days(30));
        assert_eq!(
            retention.max_age_for(BucketLevel::Minute),
            Some(Duration::hours(24))
        );
        assert_eq!(
            retention.max_age_for(BucketLevel::Hour),
            Some(Duration::days(30))
        );
        assert_eq!(retention.max_age_for(BucketLevel::Day), None);
    }
}
