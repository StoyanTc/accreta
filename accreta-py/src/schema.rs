//! `Schema` / `SchemaBuilder` wrappers.
//!
//! `SchemaBuilder::dimension`/`measure` take `name: &'static str` — a fine signature when names
//! are compile-time literals, but Python strings are runtime values. We bridge that with
//! `Box::leak`, deliberately: a `Schema` is built once, near process/interpreter startup, and
//! lives for the lifetime of the `Engine`(s) built from it — leaking a handful of short-lived
//! `String`s once at schema-build time is the same one-time cost `&'static str` literals would
//! have paid anyway, just paid explicitly instead of at compile time. This is *not* safe to do
//! on a hot path (e.g. per-request schema construction) — don't build schemas in a loop.
//!
//! `SchemaBuilder::measure`/`with`/`with_any` are also generic over `T`/`A` at the Rust level.
//! Since Python passes the numeric type and the aggregate list as runtime strings, `measure()`
//! below does the dispatch that the Rust generics would otherwise do at compile time. `Sum`,
//! `Min`, `Max`, and `Average` are all registered via `.with::<T>()` (generic over the measure
//! type); only `Count` uses `.with_any()`, confirmed against your `basic_usage` example.

use accreta::aggregate_set::{Schema, SchemaBuilder};
use accreta::aggregates::{Average, Count, Max, Min, Sum, TDigest};
use accreta::measures::MeasureType;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;

fn leak_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

#[pyclass(name = "SchemaBuilder")]
pub struct PySchemaBuilder {
    // `Option` so `build(&mut self)` can `.take()` and consume the inner builder without
    // needing `self` to be consumed (PyO3 methods take `&mut self`, not `self`, for pyclasses).
    inner: Option<SchemaBuilder>,
}

#[pymethods]
impl PySchemaBuilder {
    #[new]
    fn new() -> Self {
        Self {
            inner: Some(SchemaBuilder::default()),
        }
    }

    /// Register a dimension. Re-registering the same name is a no-op (matches the Rust API).
    fn dimension(&mut self, name: String) -> PyResult<()> {
        let builder = self.builder_mut()?;
        builder.dimension(leak_str(name));
        Ok(())
    }

    /// Register a measure of type `dtype` ("i64" | "u64" | "f64") with the given aggregates
    /// (any of "sum", "min", "max" — value-based; "count" — value-independent; "average";
    /// "tdigest" — approximate quantiles, **f64 measures only**).
    ///
    /// Raises `ValueError` for an unknown dtype/aggregate name, or for "tdigest" on an i64/u64
    /// measure (`TDigest` is concrete `f64`, not generic like `Sum<T>`/`Average<T>`, so it can
    /// only attach to an f64 measure — matches the `MeasureBuilder<T>::with::<A>()` bound
    /// enforced at compile time in the Rust API). Raises `PanicException` (via PyO3's default
    /// panic-catching) for a duplicate measure or aggregate name, or more than 64 dimensions —
    /// same conditions that panic in the Rust API.
    #[pyo3(signature = (name, dtype, aggregates))]
    fn measure(&mut self, name: String, dtype: &str, aggregates: Vec<String>) -> PyResult<()> {
        let name = leak_str(name);
        let measure_type = parse_dtype(dtype)?;
        let builder = self.builder_mut()?;

        match measure_type {
            MeasureType::F64 => {
                let mut mb = builder.measure::<f64>(name);
                for agg in &aggregates {
                    match agg.as_str() {
                        "sum" => {
                            mb.with::<Sum<f64>>();
                        }
                        "min" => {
                            mb.with::<Min<f64>>();
                        }
                        "max" => {
                            mb.with::<Max<f64>>();
                        }
                        "count" => {
                            mb.with_any::<Count>();
                        }
                        "average" => {
                            mb.with::<Average<f64>>();
                        }
                        "tdigest" => {
                            mb.with::<TDigest>();
                        }
                        other => return Err(unknown_aggregate(other)),
                    }
                }
            }
            MeasureType::I64 => {
                let mut mb = builder.measure::<i64>(name);
                for agg in &aggregates {
                    match agg.as_str() {
                        "sum" => {
                            mb.with::<Sum<i64>>();
                        }
                        "min" => {
                            mb.with::<Min<i64>>();
                        }
                        "max" => {
                            mb.with::<Max<i64>>();
                        }
                        "count" => {
                            mb.with_any::<Count>();
                        }
                        "average" => {
                            mb.with::<Average<i64>>();
                        }
                        "tdigest" => return Err(tdigest_requires_f64()),
                        other => return Err(unknown_aggregate(other)),
                    }
                }
            }
            MeasureType::U64 => {
                let mut mb = builder.measure::<u64>(name);
                for agg in &aggregates {
                    match agg.as_str() {
                        "sum" => {
                            mb.with::<Sum<u64>>();
                        }
                        "min" => {
                            mb.with::<Min<u64>>();
                        }
                        "max" => {
                            mb.with::<Max<u64>>();
                        }
                        "count" => {
                            mb.with_any::<Count>();
                        }
                        "average" => {
                            mb.with::<Average<u64>>();
                        }
                        "tdigest" => return Err(tdigest_requires_f64()),
                        other => return Err(unknown_aggregate(other)),
                    }
                }
            }
        }

        Ok(())
    }

    /// Finalize into a `Schema`. Consumes the builder — calling `build()` twice, or calling
    /// `dimension`/`measure` after `build()`, raises `RuntimeError`.
    fn build(&mut self) -> PyResult<PySchema> {
        let builder = self
            .inner
            .take()
            .ok_or_else(|| PyRuntimeError::new_err("SchemaBuilder.build() already called"))?;

        builder
            .build()
            .map(PySchema)
            .map_err(crate::errors::schema_err)
    }
}

impl PySchemaBuilder {
    fn builder_mut(&mut self) -> PyResult<&mut SchemaBuilder> {
        self.inner
            .as_mut()
            .ok_or_else(|| PyRuntimeError::new_err("SchemaBuilder was already consumed by build()"))
    }
}

fn parse_dtype(s: &str) -> PyResult<MeasureType> {
    match s {
        "i64" => Ok(MeasureType::I64),
        "u64" => Ok(MeasureType::U64),
        "f64" => Ok(MeasureType::F64),
        other => Err(PyValueError::new_err(format!(
            "unknown measure dtype '{other}' (expected one of: i64, u64, f64)"
        ))),
    }
}

fn unknown_aggregate(name: &str) -> PyErr {
    PyValueError::new_err(format!(
        "unknown aggregate '{name}' (expected one of: sum, min, max, count, average, tdigest)"
    ))
}

/// `TDigest` is concrete `f64` (not generic over the measure type like `Sum<T>`/`Average<T>`),
/// so it can only be registered on an f64 measure — mirrors the up-front validation
/// accreta-ffi's `accreta_schema_builder_add_measure` does before calling into the Rust
/// registration, rather than letting a compile-time bound violation surface some other way.
fn tdigest_requires_f64() -> PyErr {
    PyValueError::new_err(
        "aggregate 'tdigest' can only be registered on an f64 measure (TDigest isn't generic \
         over the measure type)",
    )
}

#[pyclass(name = "Retention", from_py_object)]
#[derive(Clone, Default)]
pub struct PyRetention(pub accreta::retention::Retention);

#[pymethods]
impl PyRetention {
    #[new]
    fn new() -> Self {
        Self(accreta::retention::Retention::new())
    }

    /// Keep buckets at `level` ("minute" | "hour" | ... or a `BucketLevel`) for at most
    /// `max_age_hours` past the newest bucket currently stored at that level. Returns a new
    /// `Retention` (matches the Rust builder's `self -> Self` chaining) — call this repeatedly,
    /// reassigning, to configure multiple levels: `r = r.keep("minute", 1).keep("hour", 24)`.
    fn keep(&self, level: &Bound<'_, PyAny>, max_age_hours: f64) -> PyResult<Self> {
        let level = crate::bucket::parse_level(level)?;
        let duration = chrono::Duration::milliseconds((max_age_hours * 3_600_000.0) as i64);
        Ok(Self(self.0.keep(level, duration)))
    }
}

#[pyclass(name = "Schema", frozen, from_py_object)]
#[derive(Clone)]
pub struct PySchema(pub Schema);

#[pymethods]
impl PySchema {
    fn dimension_count(&self) -> usize {
        self.0.dimension_count()
    }

    fn measure_count(&self) -> usize {
        self.0.measure_count()
    }
}
