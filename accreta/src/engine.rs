//! [`Engine`]: manages the in-memory bucket hierarchy and drives rollups.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::{DateTime, Utc};

use crate::aggregate_set::{AggregateSet, Schema};
use crate::bucket::{Bucket, BucketLevel};
use crate::dimensions::{DimensionDictionaries, DimensionKey, DimensionMask};
use crate::errors::{IngestError, SchemaError};
use crate::measures::{MeasureId, MeasureValue, MeasureValues};
use crate::retention::Retention;
use crate::sample::Sample;
use crate::{DimensionId, DimensionValues};

/// The in-memory aggregation engine.
///
/// `Engine` owns one [`Bucket`] map per [`BucketLevel`]. Raw samples are only ever folded into
/// [`BucketLevel::Second`] buckets (via [`Engine::ingest`]); every coarser level is derived
/// exclusively by [`Engine::rollup`], which merges each level's buckets into the level above,
/// never touching a [`Sample`] again.
///
/// All aggregate types tracked by every bucket, at every level, are defined by one [`Schema`]
/// supplied at construction — this is what lets rollups add new aggregates later (Variance,
/// t-Digest, HyperLogLog, ...) without any change to `Engine` itself.
///
/// By default an `Engine` keeps every bucket it ever creates, at every level, forever — the
/// right choice for a bounded batch job, but not for long-running ingestion. See
/// [`Engine::with_retention`] and [`Engine::prune`] to bound memory use for buckets that are no
/// longer the most recent data at their level.
#[derive(Debug, Clone)]
pub struct Engine {
    schema: Schema,
    dictionaries: DimensionDictionaries,
    buckets: BTreeMap<BucketLevel, BTreeMap<DateTime<Utc>, Bucket>>,
    retention: Retention,
    /// Child bucket starts, per level, created or updated since that level was last consumed as
    /// a rollup source. Drives incremental [`Engine::rollup`]: only the parent buckets these
    /// could affect get recomputed, not every parent bucket at that level. Always fully drained
    /// by a completed `rollup()` call — see that method's docs for why [`Engine::prune`] depends
    /// on that invariant.
    dirty: HashMap<BucketLevel, BTreeSet<DateTime<Utc>>>,
}

impl Engine {
    /// Create a new engine tracking the aggregates described by `schema`, with no retention
    /// limit — every bucket is kept forever. Use [`Engine::with_retention`] to bound memory use
    /// instead.
    pub fn new(schema: Schema) -> Self {
        Self::with_retention(schema, Retention::new())
    }

    /// Create a new engine tracking the aggregates described by `schema`, discarding buckets
    /// older than `retention` allows for their level whenever [`Engine::prune`] is called.
    pub fn with_retention(schema: Schema, retention: Retention) -> Self {
        let dictionaries = DimensionDictionaries::new(schema.dimension_count());

        let buckets = BucketLevel::ALL
            .into_iter()
            .map(|level| (level, BTreeMap::new()))
            .collect();
        Self {
            schema,
            dictionaries,
            buckets,
            retention,
            dirty: HashMap::new(),
        }
    }

    /// The schema every bucket in this engine is built from.
    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// This engine's current retention policy.
    pub fn retention(&self) -> Retention {
        self.retention
    }

    /// Replace this engine's retention policy. Takes effect on the next [`Engine::prune`] call —
    /// it does not retroactively discard anything by itself.
    pub fn set_retention(&mut self, retention: Retention) {
        self.retention = retention;
    }

    /// Validate and convert raw measure inputs into [`MeasureValues`], checking that the count
    /// and type of each value matches this engine's [`Schema`].
    fn build_measure_values<I>(&self, values: I) -> Result<MeasureValues, IngestError>
    where
        I: IntoIterator,
        I::Item: Into<MeasureValue>,
    {
        let values: Vec<MeasureValue> = values.into_iter().map(Into::into).collect();

        if values.len() != self.schema.measure_count() {
            return Err(IngestError::MeasureCount {
                expected: self.schema.measure_count(),
                actual: values.len(),
            });
        }

        for (definition, value) in self.schema.measures().zip(&values) {
            let actual = value.data_type();

            if actual != definition.data_type {
                return Err(IngestError::MeasureType {
                    id: definition.id,
                    name: definition.name,
                    expected: definition.data_type,
                    actual,
                });
            }
        }

        Ok(MeasureValues::new(values))
    }

    /// Validate and convert raw dimension string values into [`DimensionValues`], interning each
    /// one into this engine's dimension dictionaries and checking that the count matches this
    /// engine's [`Schema`].
    fn build_dimension_values<I>(&mut self, values: I) -> Result<DimensionValues, IngestError>
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        let values: Vec<_> = values.into_iter().collect();

        if values.len() != self.schema.dimension_count() {
            return Err(IngestError::DimensionCount {
                expected: self.schema.dimension_count(),
                actual: values.len(),
            });
        }

        let ids = values
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                self.dictionaries
                    .get_or_insert(DimensionId(index as u8), value.as_ref())
            })
            .collect();

        Ok(DimensionValues::new(ids))
    }

    /// Fold one raw sample into the appropriate second bucket, creating it if necessary.
    ///
    /// This is the only entry point for raw data. Coarser levels are not touched here; call
    /// [`Engine::rollup`] to propagate the change upward.
    pub fn ingest<M, D>(
        &mut self,
        timestamp: DateTime<Utc>,
        measures: M,
        dimensions: D,
    ) -> Result<(), IngestError>
    where
        M: IntoIterator,
        M::Item: Into<MeasureValue>,
        D: IntoIterator,
        D::Item: AsRef<str>,
    {
        let measures = self.build_measure_values(measures)?;
        let dimensions = self.build_dimension_values(dimensions)?;

        let sample = Sample {
            timestamp,
            measures,
            dimensions,
        };
        let start = BucketLevel::Second.truncate(sample.timestamp);
        let schema = self.schema.clone();
        let second_buckets = self
            .buckets
            .get_mut(&BucketLevel::Second)
            .expect("Second level always present");
        second_buckets
            .entry(start)
            .or_insert_with(|| Bucket::new(BucketLevel::Second, start))
            .update(&sample, &schema);

        self.dirty
            .entry(BucketLevel::Second)
            .or_default()
            .insert(start);

        Ok(())
    }

    /// Fold a batch of samples in one call.
    pub fn ingest_all<I, M, D>(&mut self, samples: I) -> Result<(), IngestError>
    where
        I: IntoIterator<Item = (DateTime<Utc>, M, D)>,
        M: IntoIterator,
        M::Item: Into<MeasureValue>,
        D: IntoIterator,
        D::Item: AsRef<str>,
    {
        for (timestamp, measures, dimensions) in samples {
            self.ingest(timestamp, measures, dimensions)?;
        }

        Ok(())
    }

    /// Recompute every coarser bucket affected by data that changed since the last call.
    ///
    /// This is incremental, not a full rebuild: only the parent buckets whose children actually
    /// changed — new samples ingested, or a previously-rolled-up child updated by late or
    /// out-of-order data — get reprocessed. A bucket at a coarser level whose children haven't
    /// changed since the last `rollup()` call is left completely untouched.
    ///
    /// Each affected parent is still fully recomputed from its *current* complete set of children
    /// at the level below it (never a delta merged into the old parent state). That's what keeps
    /// this correct without needing an inverse operation for aggregates like `Min`/`Max` that
    /// don't have one — recomputing from scratch is always correct, it's just now scoped to the
    /// handful of buckets that could actually have changed, rather than every bucket at every
    /// coarser level on every call. Calling `rollup()` repeatedly with nothing newly dirty in
    /// between is a cheap no-op, not a repeated full rebuild.
    ///
    /// A change still cascades all the way from `Second` up through `Year` in one call: levels
    /// are processed in [`BucketLevel::ALL`] order, and a level's affected parents are marked
    /// dirty for the level above before moving on, so they're picked up within the same call. In
    /// particular, this method always leaves `self.dirty` fully empty when it returns — see
    /// [`Engine::prune`], which depends on that.
    pub fn rollup(&mut self) {
        for level in BucketLevel::ALL {
            let Some(dirty_children) = self.dirty.remove(&level) else {
                continue;
            };
            if dirty_children.is_empty() {
                continue;
            }

            for &parent_level in level.rollup_targets() {
                let mut affected_parents: BTreeSet<DateTime<Utc>> = BTreeSet::new();
                for &child_start in &dirty_children {
                    affected_parents.insert(parent_level.truncate(child_start));
                }

                let children = &self.buckets[&level];
                let mut rebuilt = Vec::with_capacity(affected_parents.len());
                for &parent_start in &affected_parents {
                    let parent_end = parent_level.bucket_end(parent_start);
                    let mut parent = Bucket::new(parent_level, parent_start);
                    for child in children.range(parent_start..parent_end).map(|(_, b)| b) {
                        parent.merge(child);
                    }
                    rebuilt.push((parent_start, parent));
                }

                let parent_buckets = self
                    .buckets
                    .get_mut(&parent_level)
                    .expect("every level is always present");
                for (start, bucket) in rebuilt {
                    parent_buckets.insert(start, bucket);
                }

                self.dirty
                    .entry(parent_level)
                    .or_default()
                    .extend(affected_parents);
            }
        }
    }

    /// Look up a single bucket by level and exact start time.
    ///
    /// `start` must already be truncated to `level` (e.g. via [`BucketLevel::truncate`]) to
    /// match; this mirrors how buckets are keyed internally.
    pub fn bucket(&self, level: BucketLevel, start: DateTime<Utc>) -> Option<&Bucket> {
        self.buckets.get(&level)?.get(&start)
    }

    /// Iterate over every bucket currently stored at `level`, in chronological order.
    pub fn buckets(&self, level: BucketLevel) -> impl Iterator<Item = &Bucket> {
        self.buckets[&level].values()
    }

    /// How many buckets are currently stored at `level`.
    pub fn bucket_count(&self, level: BucketLevel) -> usize {
        self.buckets[&level].len()
    }

    /// Merge every matching bucket into one total [`AggregateSet`].
    ///
    /// This preserves the original, non-grouped query behavior: all dimension
    /// groups are merged together. For a grouped query use [`Self::query_range_grouped`].
    pub fn query_range(
        &self,
        level: BucketLevel,
        range_start: DateTime<Utc>,
        range_end: DateTime<Utc>,
        measure: MeasureId,
    ) -> Result<AggregateSet, SchemaError> {
        let mut acc = self.schema.empty_set(measure)?;

        for bucket in self.buckets(level) {
            if bucket.start() < range_end && bucket.end() > range_start {
                for sets in bucket.groups() {
                    let aggregates = sets
                        .1
                        .get(measure.index())
                        .expect("bucket measure sets are aligned with schema");

                    acc.merge(aggregates);
                }
            }
        }

        Ok(acc)
    }

    /// Query a time range and group the result by the dimensions selected by `group_by`.
    ///
    /// Buckets store complete dimension keys. The query projects those keys onto
    /// `group_by` and merges aggregate states having the same projected key.
    ///
    /// An empty mask produces one empty [`DimensionKey`] containing the total
    /// across all groups.
    pub fn query_range_grouped(
        &self,
        level: BucketLevel,
        range_start: DateTime<Utc>,
        range_end: DateTime<Utc>,
        measure: MeasureId,
        group_by: DimensionMask,
    ) -> Result<HashMap<DimensionKey, AggregateSet>, SchemaError> {
        // Validate the measure before doing any work.
        self.schema
            .measure(measure)
            .ok_or(SchemaError::InvalidMeasureId(measure))?;

        let mut result = HashMap::new();

        for bucket in self.buckets(level) {
            if bucket.start() >= range_end || bucket.end() <= range_start {
                continue;
            }

            for (full_key, sets) in bucket.groups() {
                let aggregates = sets
                    .get(measure.index())
                    .expect("bucket measure sets are aligned with schema");

                let key = full_key.project(group_by);

                result
                    .entry(key)
                    .or_insert_with(|| {
                        self.schema
                            .empty_set(measure)
                            .expect("measure was validated above")
                    })
                    .merge(aggregates);
            }
        }

        Ok(result)
    }

    /// Discard buckets older than their level's configured [`Retention`] window.
    ///
    /// For each level with a retention limit set, "older" is measured from the newest bucket
    /// currently stored *at that level* — not from wall-clock time — so this works the same way
    /// for live ingestion and for replaying historical data (see [`Retention::keep`]). Levels
    /// with no configured limit, and levels with no buckets yet, are left untouched.
    ///
    /// This is deliberately not called automatically by [`Engine::rollup`]: rollups only ever
    /// merge state (never deleting anything), which is the crate's core invariant, while `prune`
    /// is the one place data actually leaves the engine. Call it explicitly on whatever schedule
    /// suits your workload — typically right after `rollup`.
    ///
    /// A bucket that hasn't been rolled up into its parent yet is never discarded, even if it's
    /// past its retention cutoff — pruning it before [`Engine::rollup`] has folded it upward
    /// would permanently lose its contribution to every coarser level, since `rollup()` can only
    /// ever see what's still present in this engine's bucket storage. Calling `rollup()` before
    /// `prune()`, as documented above, means this case does not normally arise — a completed
    /// `rollup()` call always leaves nothing dirty — so this is a backstop for out-of-order calls,
    /// not a substitute for calling `rollup()` first.
    pub fn prune(&mut self) {
        for level in BucketLevel::ALL {
            let Some(max_age) = self.retention.max_age_for(level) else {
                continue;
            };
            let Some(latest_end) = self.buckets(level).map(Bucket::end).max() else {
                continue;
            };
            let cutoff = latest_end - max_age;

            let dirty_at_level = self.dirty.get(&level);
            let has_pending_rollup =
                |start: &DateTime<Utc>| dirty_at_level.is_some_and(|d| d.contains(start));

            self.buckets
                .get_mut(&level)
                .expect("every level is always present")
                .retain(|start, bucket| bucket.end() > cutoff || has_pending_rollup(start));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate_set::Schema;
    use crate::aggregates::{Count, Sum};
    use chrono::{Duration as ChronoDuration, TimeZone};

    fn schema() -> Schema {
        let mut builder = Schema::builder();
        builder
            .dimension("host")
            .measure("value")
            .with::<Sum<f64>>()
            .with_any::<Count>();
        builder.build().unwrap()
    }

    #[test]
    fn rollup_is_a_noop_with_nothing_dirty() {
        let mut engine = Engine::new(schema());
        let t0 = Utc.with_ymd_and_hms(2026, 3, 15, 10, 5, 0).unwrap();
        engine.ingest(t0, [1.0], ["a"]).unwrap();
        engine.rollup();
        engine.rollup(); // nothing new since the first call

        let minute_start = BucketLevel::Minute.truncate(t0);
        let (_, sets) = engine
            .bucket(BucketLevel::Minute, minute_start)
            .unwrap()
            .groups()
            .next()
            .unwrap();
        assert_eq!(sets[0].get::<Sum<f64>>().unwrap().value(), 1.0);
    }

    #[test]
    fn late_update_to_an_already_rolled_up_second_still_propagates() {
        let mut engine = Engine::new(schema());
        let t0 = Utc.with_ymd_and_hms(2026, 3, 15, 10, 5, 0).unwrap();
        engine.ingest(t0, [1.0], ["a"]).unwrap();
        engine.rollup();

        // Lands in the *same* second bucket, after it's already been rolled up once.
        engine.ingest(t0, [4.0], ["a"]).unwrap();
        engine.rollup();

        let minute_start = BucketLevel::Minute.truncate(t0);
        let (_, sets) = engine
            .bucket(BucketLevel::Minute, minute_start)
            .unwrap()
            .groups()
            .next()
            .unwrap();
        assert_eq!(sets[0].get::<Sum<f64>>().unwrap().value(), 5.0);
        assert_eq!(sets[0].get::<Count>().unwrap().value(), 2);
    }

    #[test]
    fn pruning_an_already_merged_child_does_not_erase_its_contribution_upstream() {
        let policy = Retention::new().keep(BucketLevel::Second, ChronoDuration::seconds(0));
        let mut engine = Engine::with_retention(schema(), policy);
        let t0 = Utc.with_ymd_and_hms(2026, 3, 15, 10, 5, 0).unwrap();
        engine.ingest(t0, [7.0], ["a"]).unwrap();
        engine.rollup();
        engine.prune();
        assert_eq!(engine.bucket_count(BucketLevel::Second), 0);

        // Nothing dirty at Second anymore, so Minute is left alone rather than rebuilt from the
        // now-empty surviving seconds.
        engine.rollup();
        let minute_start = BucketLevel::Minute.truncate(t0);
        let (_, sets) = engine
            .bucket(BucketLevel::Minute, minute_start)
            .unwrap()
            .groups()
            .next()
            .unwrap();
        assert_eq!(sets[0].get::<Sum<f64>>().unwrap().value(), 7.0);
    }

    #[test]
    fn day_fans_out_to_week_and_month_incrementally() {
        let mut engine = Engine::new(schema());
        let t0 = Utc.with_ymd_and_hms(2026, 3, 15, 10, 5, 0).unwrap();
        engine.ingest(t0, [2.0], ["a"]).unwrap();
        engine.rollup();
        assert!(
            engine
                .bucket(BucketLevel::Week, BucketLevel::Week.truncate(t0))
                .is_some()
        );
        assert!(
            engine
                .bucket(BucketLevel::Month, BucketLevel::Month.truncate(t0))
                .is_some()
        );
    }

    #[test]
    fn pruning_before_rollup_does_not_lose_dirty_data() {
        let policy = Retention::new().keep(BucketLevel::Second, ChronoDuration::seconds(0));
        let mut engine = Engine::with_retention(schema(), policy);
        let t0 = Utc.with_ymd_and_hms(2026, 3, 15, 10, 5, 0).unwrap();
        engine.ingest(t0, [3.0], ["a"]).unwrap();

        // prune() called before rollup(): the Second bucket is still dirty, so it must survive
        // even though it's already past its (zero-length) retention window.
        engine.prune();
        assert_eq!(engine.bucket_count(BucketLevel::Second), 1);

        engine.rollup();
        engine.prune();
        // Now that it's been rolled up, prune() can safely remove it.
        assert_eq!(engine.bucket_count(BucketLevel::Second), 0);

        let minute_start = BucketLevel::Minute.truncate(t0);
        let (_, sets) = engine
            .bucket(BucketLevel::Minute, minute_start)
            .unwrap()
            .groups()
            .next()
            .unwrap();
        assert_eq!(sets[0].get::<Sum<f64>>().unwrap().value(), 3.0);
    }
}
