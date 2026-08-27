//! `Engine` wrapper: ingest, rollup, prune, and the two query methods.
//!
//! Timestamps use PyO3's `chrono` conversion feature (`DateTime<Utc>` <-> Python
//! `datetime.datetime`) — make sure `pyo3 = { version = "0.29", features = ["chrono"] }` (in
//! addition to whatever features you already have) and `chrono = { version = "...", features =
//! ["clock"] }` are in Cargo.toml, or these signatures won't compile. A naive Python
//! `datetime` (no tzinfo) will fail the conversion at the PyO3 layer with a clear error rather
//! than silently being treated as UTC — callers need to pass timezone-aware datetimes.

use accreta::aggregate_set::Schema;
use accreta::dimensions::DimensionMask;
use accreta::engine::Engine;
use accreta::measures::MeasureId;
use chrono::{DateTime, Utc};
use pyo3::prelude::*;
use pyo3::types::PyDict;

use crate::aggregate_set::PyAggregateSet;
use crate::bucket::parse_level;
use crate::dimensions::{PyDimensionKey, PyDimensionMask};
use crate::errors::{ingest_err, schema_err};
use crate::measures::py_to_measure_value;
use crate::schema::{PyRetention, PySchema};

#[pyclass(name = "Engine")]
pub struct PyEngine {
    inner: Engine,
    schema: Schema,
}

#[pymethods]
impl PyEngine {
    /// `retention=None` (the default) keeps every bucket forever, matching `Engine::new`.
    /// Pass a `Retention` to bound memory use, matching `Engine::with_retention`.
    #[new]
    #[pyo3(signature = (schema, retention=None))]
    fn new(schema: PySchema, retention: Option<PyRetention>) -> Self {
        let retention = retention.map(|r| r.0).unwrap_or_default();
        Self {
            inner: Engine::with_retention(schema.0.clone(), retention),
            schema: schema.0,
        }
    }

    /// Replace this engine's retention policy. Takes effect on the next `prune()` call — it
    /// does not retroactively discard anything by itself.
    fn set_retention(&mut self, retention: PyRetention) {
        self.inner.set_retention(retention.0);
    }

    /// Fold one sample into the engine.
    ///
    /// `measures` is a list of Python numbers, one per measure, in schema registration order —
    /// each converted according to that measure's declared dtype (see `measures.py_to_measure_value`).
    /// `dimensions` is a list of strings, one per dimension, in schema registration order.
    fn ingest(
        &mut self,
        timestamp: DateTime<Utc>,
        measures: Vec<Bound<'_, PyAny>>,
        dimensions: Vec<String>,
    ) -> PyResult<()> {
        let measure_values: PyResult<Vec<_>> = measures
            .iter()
            .zip(self.schema.measures())
            .map(|(value, definition)| py_to_measure_value(value, definition.data_type))
            .collect();
        let measure_values = measure_values?;

        self.inner
            .ingest(timestamp, measure_values, dimensions)
            .map_err(ingest_err)
    }

    /// Recompute every level above `minute` by merging bucket states upward. Safe to call
    /// repeatedly (idempotent) — see `Engine::rollup`'s Rust docs.
    fn rollup(&mut self) {
        self.inner.rollup();
    }

    /// Discard buckets older than the configured retention window, per level.
    fn prune(&mut self) {
        self.inner.prune();
    }

    /// How many buckets are currently stored at `level` ("minute" | "hour" | "day" | "week" |
    /// "month" | "year", or a `BucketLevel`).
    fn bucket_count(&self, level: &Bound<'_, PyAny>) -> PyResult<usize> {
        let level = parse_level(level)?;
        Ok(self.inner.bucket_count(level))
    }

    /// Merge every bucket in `[range_start, range_end)` at `level` into one total
    /// `AggregateSet` for `measure_index` (all dimension groups combined).
    fn query_range(
        &self,
        level: &Bound<'_, PyAny>,
        range_start: DateTime<Utc>,
        range_end: DateTime<Utc>,
        measure_index: u8,
    ) -> PyResult<PyAggregateSet> {
        let level = parse_level(level)?;
        let measure = MeasureId(measure_index);

        self.inner
            .query_range(level, range_start, range_end, measure)
            .map(PyAggregateSet)
            .map_err(schema_err)
    }

    /// Like `query_range`, but grouped by the dimensions selected in `group_by`. Returns a
    /// `dict` from `DimensionKey` (raw dimension-value IDs — see `dimensions.rs`'s GAP note) to
    /// `AggregateSet`. `DimensionKey` doesn't define `__eq__`/`__hash__`, so the dict is only
    /// safe to *iterate*, not to look up by a separately-constructed key — Python falls back to
    /// identity hashing for it.
    fn query_range_grouped<'py>(
        &self,
        py: Python<'py>,
        level: &Bound<'_, PyAny>,
        range_start: DateTime<Utc>,
        range_end: DateTime<Utc>,
        measure_index: u8,
        group_by: &PyDimensionMask,
    ) -> PyResult<Bound<'py, PyDict>> {
        let level = parse_level(level)?;
        let measure = MeasureId(measure_index);
        let mask: DimensionMask = group_by.0;

        let grouped = self
            .inner
            .query_range_grouped(level, range_start, range_end, measure, mask)
            .map_err(schema_err)?;

        let dict = PyDict::new(py);
        for (key, set) in grouped {
            let py_key = Py::new(
                py,
                PyDimensionKey {
                    values: key.values().to_vec(),
                },
            )?;
            let py_set = Py::new(py, PyAggregateSet(set))?;
            dict.set_item(py_key, py_set)?;
        }

        Ok(dict)
    }
}
