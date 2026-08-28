//! [`Engine`]: manages the in-memory bucket hierarchy and drives rollups.

use std::collections::{BTreeMap, HashMap};

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
/// [`BucketLevel::Minute`] buckets (via [`Engine::ingest`]); every coarser level is derived
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

    /// Fold one raw sample into the appropriate minute bucket, creating it if necessary.
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
        let start = BucketLevel::Minute.truncate(sample.timestamp);
        let schema = self.schema.clone();
        let minute_buckets = self
            .buckets
            .get_mut(&BucketLevel::Minute)
            .expect("Minute level always present");
        minute_buckets
            .entry(start)
            .or_insert_with(|| Bucket::new(BucketLevel::Minute, start))
            .update(&sample, &schema);

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

    /// Recompute every level above [`BucketLevel::Minute`] by merging bucket states upward.
    ///
    /// Each level's buckets are rebuilt from scratch from the level directly below it (which may
    /// itself have just been rebuilt earlier in the same call), so calling `rollup` repeatedly is
    /// safe and idempotent — it never double-counts a sample, because it never touches samples at
    /// all, only merges the current [`Bucket`] states.
    pub fn rollup(&mut self) {
        for level in BucketLevel::ALL {
            for parent_level in level.rollup_targets() {
                let children = &self.buckets[&level];
                let mut parents: BTreeMap<DateTime<Utc>, Bucket> = BTreeMap::new();

                for child in children.values() {
                    let parent_start = parent_level.truncate(child.start());

                    parents
                        .entry(parent_start)
                        .and_modify(|parent| parent.merge(child))
                        .or_insert_with(|| {
                            let mut parent = Bucket::new(*parent_level, parent_start);
                            parent.merge(child);
                            parent
                        });
                }

                self.buckets.insert(*parent_level, parents);
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
    pub fn prune(&mut self) {
        for level in BucketLevel::ALL {
            let Some(max_age) = self.retention.max_age_for(level) else {
                continue;
            };
            let Some(latest_end) = self.buckets(level).map(Bucket::end).max() else {
                continue;
            };
            let cutoff = latest_end - max_age;
            self.buckets
                .get_mut(&level)
                .expect("every level is always present")
                .retain(|_, bucket| bucket.end() > cutoff);
        }
    }
}
