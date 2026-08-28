//! `AggregateSet` wrapper.
//!
//! `accreta::aggregate_set::AggregateSet::get<T>()` is generic over the concrete aggregate type
//! and looked up by Rust type, not by name. To hand Python a plain `{name: value}` dict, we go
//! the other way instead: iterate `set.iter()` (`(&'static str name, &dyn ErasedState)`) and
//! downcast each state against the built-in kinds — confirmed directly against
//! `aggregates/{sum,min,max,count,average}.rs`:
//!
//! - `Sum<T>`, `Min<T>`, `Max<T>` — generic over `T`, registered via `.with::<T>()`.
//!   `Sum::value(&self) -> T`. `Min`/`Max::value(&self) -> Option<T>` — `None` for an empty
//!   bucket (identity element), surfaced here as Python `None`.
//! - `Count` — not generic, registered via `.with_any::<Count>()`, `.value(&self) -> u64`.
//! - `Average<T>` — generic, registered via `.with::<T>()` like `Sum`/`Min`/`Max`. No
//!   `.value()` — exposes `.sum() -> T` and `.count() -> u64`. This wrapper reports the
//!   computed ratio as a Python `float`, and `None` when `count() == 0` rather than dividing by
//!   zero (a case the core crate itself doesn't need to handle, since `Average` never exposes
//!   division directly).

use std::any::Any;

use accreta::aggregate_set::AggregateSet;
use accreta::aggregates::{Average, Count, Max, Min, Sum}; // ASSUMED module path + type names
use accreta::measures::MeasureType;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

#[pyclass(name = "AggregateSet", frozen)]
pub struct PyAggregateSet(pub AggregateSet);

#[pymethods]
impl PyAggregateSet {
    /// All aggregate values in this set, as `{name: value}` — e.g.
    /// `{"sum": 20.0, "count": 2}`. `dtype` must be the `MeasureType` of the measure this set
    /// aggregates ("i64" | "u64" | "f64"); `PyEngine` methods that return a `PyAggregateSet`
    /// pass it in automatically, so you shouldn't normally need to call this directly with an
    /// explicit `dtype`.
    fn values<'py>(&self, py: Python<'py>, dtype: &str) -> PyResult<Bound<'py, PyDict>> {
        let dtype = parse_dtype(dtype)?;
        let dict = PyDict::new(py);

        for (name, state) in self.0.iter() {
            let value = extract_aggregate_value(py, name, dtype, state)?;
            dict.set_item(name, value)?;
        }

        Ok(dict)
    }
}

fn parse_dtype(s: &str) -> PyResult<MeasureType> {
    match s {
        "i64" => Ok(MeasureType::I64),
        "u64" => Ok(MeasureType::U64),
        "f64" => Ok(MeasureType::F64),
        other => Err(PyRuntimeError::new_err(format!(
            "unknown measure dtype '{other}'"
        ))),
    }
}

/// Downcast one erased aggregate state to a concrete built-in type and convert it into a
/// Python object, per the shapes confirmed in the module-level doc comment.

pub fn extract_aggregate_value<'py>(
    py: Python<'py>,
    name: &str,
    dtype: MeasureType,
    state: &dyn accreta::erased::ErasedState,
) -> PyResult<Bound<'py, PyAny>> {
    let any: &dyn Any = state.as_any();

    // Helper macro to eliminate repetitive downcasting boilerplate
    // Explicitly coerce the error type to PyErr using .map_err(Into::into)
    macro_rules! downcast_val {
        ($ty:ty, $expr:expr) => {
            any.downcast_ref::<$ty>().map(|s| {
                ($expr)(s)
                    .into_pyobject(py)
                    .map(|b| b.into_any())
                    .map_err(PyErr::from)
            })
        };
    }

    let value: Option<PyResult<Bound<'py, PyAny>>> = match (name, dtype) {
        ("sum", MeasureType::F64) => downcast_val!(Sum<f64>, |s: &Sum<f64>| s.value()),
        ("sum", MeasureType::I64) => downcast_val!(Sum<i64>, |s: &Sum<i64>| s.value()),
        ("sum", MeasureType::U64) => downcast_val!(Sum<u64>, |s: &Sum<u64>| s.value()),

        // Option<T> implements IntoPyObject naturally (None maps directly to Python None)
        ("min", MeasureType::F64) => downcast_val!(Min<f64>, |s: &Min<f64>| s.value()),
        ("min", MeasureType::I64) => downcast_val!(Min<i64>, |s: &Min<i64>| s.value()),
        ("min", MeasureType::U64) => downcast_val!(Min<u64>, |s: &Min<u64>| s.value()),

        ("max", MeasureType::F64) => downcast_val!(Max<f64>, |s: &Max<f64>| s.value()),
        ("max", MeasureType::I64) => downcast_val!(Max<i64>, |s: &Max<i64>| s.value()),
        ("max", MeasureType::U64) => downcast_val!(Max<u64>, |s: &Max<u64>| s.value()),

        ("count", _) => downcast_val!(Count, |s: &Count| s.value()),

        ("average", MeasureType::F64) => downcast_val!(Average<f64>, |s: &Average<f64>| {
            let count = s.count();
            if count == 0 {
                None
            } else {
                Some(s.sum() / count as f64)
            }
        }),
        ("average", MeasureType::I64) => downcast_val!(Average<i64>, |s: &Average<i64>| {
            let count = s.count();
            if count == 0 {
                None
            } else {
                Some(s.sum() as f64 / count as f64)
            }
        }),
        ("average", MeasureType::U64) => downcast_val!(Average<u64>, |s: &Average<u64>| {
            let count = s.count();
            if count == 0 {
                None
            } else {
                Some(s.sum() as f64 / count as f64)
            }
        }),

        (other, _) => {
            return Err(PyRuntimeError::new_err(format!(
                "accreta-py doesn't yet know how to read aggregate '{other}' — \
                 it isn't one of the built-in sum/min/max/count/average kinds this wrapper handles"
            )));
        }
    };

    match value {
        Some(res) => res,
        None => Err(PyRuntimeError::new_err(format!(
            "aggregate '{name}' didn't downcast to the expected type for dtype {dtype} — \
             the schema likely registered this aggregate under a different measure dtype \
             than the one passed to AggregateSet.values()"
        ))),
    }
}
