//! Time windows ([`Bucket`]) organized into a fixed hierarchy of granularities
//! ([`BucketLevel`]).

use std::collections::HashMap;

use chrono::{DateTime, Datelike, Duration, TimeZone, Timelike, Utc, Weekday};

use crate::aggregate_set::{AggregateSet, Schema};
use crate::dimensions::DimensionKey;
use crate::sample::Sample;

/// A granularity in the bucket hierarchy, from finest to coarsest.
///
/// The hierarchy is fixed (`Minute -> Hour -> Day -> Week -> Month -> Year`); what's extensible
/// is the *set of aggregates* each bucket at each level tracks (see [`crate::aggregate_set`]),
/// not the levels themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BucketLevel {
    /// One-minute buckets, the finest granularity.
    Minute,
    /// One-hour buckets, rolled up from [`BucketLevel::Minute`].
    Hour,
    /// One-day buckets, rolled up from [`BucketLevel::Hour`].
    Day,
    /// One-week (Monday-start) buckets, rolled up from [`BucketLevel::Day`].
    Week,
    /// One-month buckets, rolled up from [`BucketLevel::Week`].
    Month,
    /// One-year buckets, the coarsest granularity, rolled up from
    /// [`BucketLevel::Month`].
    Year,
}

impl BucketLevel {
    /// Every level, from finest to coarsest. Rollups always proceed in this order.
    pub const ALL: [BucketLevel; 6] = [
        BucketLevel::Minute,
        BucketLevel::Hour,
        BucketLevel::Day,
        BucketLevel::Week,
        BucketLevel::Month,
        BucketLevel::Year,
    ];

    /// The next coarser level this one rolls up into, or `None` for [`BucketLevel::Year`], the
    /// top of the hierarchy.
    pub fn parent(self) -> Option<BucketLevel> {
        match self {
            BucketLevel::Minute => Some(BucketLevel::Hour),
            BucketLevel::Hour => Some(BucketLevel::Day),
            BucketLevel::Day => Some(BucketLevel::Week),
            BucketLevel::Week => Some(BucketLevel::Month),
            BucketLevel::Month => Some(BucketLevel::Year),
            BucketLevel::Year => None,
        }
    }

    pub fn rollup_targets(self) -> &'static [BucketLevel] {
        match self {
            BucketLevel::Minute => &[BucketLevel::Hour],
            BucketLevel::Hour => &[BucketLevel::Day],
            BucketLevel::Day => &[BucketLevel::Week, BucketLevel::Month],
            BucketLevel::Week => &[],
            BucketLevel::Month => &[BucketLevel::Year],
            BucketLevel::Year => &[],
        }
    }

    /// Truncate a timestamp down to the start of the bucket at this level that contains it.
    ///
    /// Weeks start on Monday (ISO 8601).
    pub fn truncate(self, dt: DateTime<Utc>) -> DateTime<Utc> {
        match self {
            BucketLevel::Minute => dt
                .with_second(0)
                .and_then(|d| d.with_nanosecond(0))
                .expect("valid truncation to minute"),
            BucketLevel::Hour => dt
                .with_minute(0)
                .and_then(|d| d.with_second(0))
                .and_then(|d| d.with_nanosecond(0))
                .expect("valid truncation to hour"),
            BucketLevel::Day => Utc
                .with_ymd_and_hms(dt.year(), dt.month(), dt.day(), 0, 0, 0)
                .single()
                .expect("valid truncation to day"),
            BucketLevel::Week => {
                let iso_week_monday = dt.date_naive().week(Weekday::Mon).first_day();
                Utc.with_ymd_and_hms(
                    iso_week_monday.year(),
                    iso_week_monday.month(),
                    iso_week_monday.day(),
                    0,
                    0,
                    0,
                )
                .single()
                .expect("valid truncation to week")
            }
            BucketLevel::Month => Utc
                .with_ymd_and_hms(dt.year(), dt.month(), 1, 0, 0, 0)
                .single()
                .expect("valid truncation to month"),
            BucketLevel::Year => Utc
                .with_ymd_and_hms(dt.year(), 1, 1, 0, 0, 0)
                .single()
                .expect("valid truncation to year"),
        }
    }

    /// The bucket at this level that `child_start` (already truncated to the child level below
    /// this one) rolls up into.
    pub fn parent_start(self, child_start: DateTime<Utc>) -> DateTime<Utc> {
        self.truncate(child_start)
    }

    /// A reasonable upper-bound duration for one bucket at this level, used only for display /
    /// sanity-checking purposes (months and years are not fixed-length, so this is approximate
    /// for those two).
    pub fn approx_duration(self) -> Duration {
        match self {
            BucketLevel::Minute => Duration::minutes(1),
            BucketLevel::Hour => Duration::hours(1),
            BucketLevel::Day => Duration::days(1),
            BucketLevel::Week => Duration::weeks(1),
            BucketLevel::Month => Duration::days(30),
            BucketLevel::Year => Duration::days(365),
        }
    }
}

impl std::fmt::Display for BucketLevel {
    /// Writes the lowercase level name (`"minute"`, `"hour"`, ...) — the exhaustive match still
    /// forces a compile error if a variant is ever added without updating this, but callers now
    /// get it for free through `{}`, `.to_string()`, `tracing` fields, error messages, etc.
    /// instead of a one-off `label()` method that only this crate knows how to call.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            BucketLevel::Minute => "minute",
            BucketLevel::Hour => "hour",
            BucketLevel::Day => "day",
            BucketLevel::Week => "week",
            BucketLevel::Month => "month",
            BucketLevel::Year => "year",
        };
        f.write_str(s)
    }
}

/// A single time window at a given [`BucketLevel`], containing one
/// [`AggregateSet`] per complete dimension combination.
///
/// Buckets deliberately store the *full* dimension key. This lets queries
/// choose an arbitrary `GROUP BY` projection later without materializing all
/// possible projections during ingestion.
#[derive(Debug, Clone)]
pub struct Bucket {
    level: BucketLevel,
    start: DateTime<Utc>,
    groups: HashMap<DimensionKey, Vec<AggregateSet>>,
}

impl Bucket {
    /// Create a new, empty bucket at `level` starting at `start`.
    pub fn new(level: BucketLevel, start: DateTime<Utc>) -> Self {
        Self {
            level,
            start,
            groups: HashMap::new(),
        }
    }

    /// The granularity of this bucket.
    pub fn level(&self) -> BucketLevel {
        self.level
    }

    /// The (inclusive) start of this bucket's time window.
    pub fn start(&self) -> DateTime<Utc> {
        self.start
    }

    /// The (exclusive) end of this bucket's time window, derived from
    /// [`Self::start`] and [`Self::level`].
    pub fn end(&self) -> DateTime<Utc> {
        match self.level {
            BucketLevel::Minute => self.start + Duration::minutes(1),
            BucketLevel::Hour => self.start + Duration::hours(1),
            BucketLevel::Day => self.start + Duration::days(1),
            BucketLevel::Week => self.start + Duration::weeks(1),
            BucketLevel::Month => {
                let (y, m) = if self.start.month() == 12 {
                    (self.start.year() + 1, 1)
                } else {
                    (self.start.year(), self.start.month() + 1)
                };
                Utc.with_ymd_and_hms(y, m, 1, 0, 0, 0)
                    .single()
                    .expect("valid month end")
            }
            BucketLevel::Year => Utc
                .with_ymd_and_hms(self.start.year() + 1, 1, 1, 0, 0, 0)
                .single()
                .expect("valid year end"),
        }
    }

    /// Read-only access to all dimension groups in this bucket.
    pub fn groups(&self) -> impl Iterator<Item = (&DimensionKey, &Vec<AggregateSet>)> {
        self.groups.iter()
    }

    /// Number of distinct full-dimension groups in this bucket.
    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    /// Get one full-dimension group's aggregate state.
    pub fn group(&self, key: &DimensionKey) -> Option<&Vec<AggregateSet>> {
        self.groups.get(key)
    }

    /// Fold all measures of a raw sample into its full-dimension group.
    pub fn update(&mut self, sample: &Sample, schema: &Schema) {
        let key = sample.dimensions.full_key();

        let sets = self.groups.entry(key).or_insert_with(|| {
            schema
                .measure_ids()
                .map(|measure| {
                    schema
                        .empty_set(measure)
                        .expect("measure ID must be valid for its schema")
                })
                .collect()
        });

        for set in sets {
            set.update(sample);
        }
    }

    /// Merge all dimension groups from another bucket.
    ///
    /// The dimension key is preserved unchanged. This is what makes the
    /// minute -> hour -> day -> ... rollup hierarchy mergeable without raw
    /// samples.
    pub fn merge(&mut self, other: &Bucket) {
        for (key, other_sets) in &other.groups {
            if let Some(sets) = self.groups.get_mut(key) {
                assert_eq!(sets.len(), other_sets.len());

                for (mine, theirs) in sets.iter_mut().zip(other_sets) {
                    mine.merge(theirs);
                }
            } else {
                self.groups.insert(key.clone(), other_sets.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn dt(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s)
            .single()
            .unwrap()
            .with_nanosecond(500)
            .unwrap()
    }

    #[test]
    fn minute_truncation_drops_seconds() {
        let t = dt(2026, 3, 15, 10, 42, 37);
        assert_eq!(
            BucketLevel::Minute.truncate(t),
            dt(2026, 3, 15, 10, 42, 0).with_nanosecond(0).unwrap()
        );
    }

    #[test]
    fn hour_truncation_drops_minutes() {
        let t = dt(2026, 3, 15, 10, 42, 37);
        assert_eq!(
            BucketLevel::Hour.truncate(t),
            Utc.with_ymd_and_hms(2026, 3, 15, 10, 0, 0).unwrap()
        );
    }

    #[test]
    fn day_truncation_drops_time_of_day() {
        let t = dt(2026, 3, 15, 10, 42, 37);
        assert_eq!(
            BucketLevel::Day.truncate(t),
            Utc.with_ymd_and_hms(2026, 3, 15, 0, 0, 0).unwrap()
        );
    }

    #[test]
    fn week_truncation_snaps_to_monday() {
        // 2026-03-15 is a Sunday.
        let sunday = dt(2026, 3, 15, 10, 0, 0);
        let monday = Utc.with_ymd_and_hms(2026, 3, 9, 0, 0, 0).unwrap();
        assert_eq!(BucketLevel::Week.truncate(sunday), monday);
    }

    #[test]
    fn month_truncation_snaps_to_first() {
        let t = dt(2026, 3, 15, 10, 0, 0);
        assert_eq!(
            BucketLevel::Month.truncate(t),
            Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap()
        );
    }

    #[test]
    fn year_truncation_snaps_to_jan_first() {
        let t = dt(2026, 3, 15, 10, 0, 0);
        assert_eq!(
            BucketLevel::Year.truncate(t),
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap()
        );
    }

    #[test]
    fn month_end_handles_december_rollover() {
        let dec_start = Utc.with_ymd_and_hms(2026, 12, 1, 0, 0, 0).unwrap();
        let bucket = Bucket::new(BucketLevel::Month, dec_start);
        assert_eq!(
            bucket.end(),
            Utc.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).unwrap()
        );
    }

    #[test]
    fn parent_chain_terminates_at_year() {
        assert_eq!(BucketLevel::Minute.parent(), Some(BucketLevel::Hour));
        assert_eq!(BucketLevel::Year.parent(), None);
    }

    #[test]
    fn display_gives_lowercase_name() {
        assert_eq!(BucketLevel::Minute.to_string(), "minute");
        assert_eq!(BucketLevel::Year.to_string(), "year");
    }
}
