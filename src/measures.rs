//! `MeasureId` and conversions between Python numbers and `accreta::measures::MeasureValue`.
//!
//! Python has no static distinction between `i64`/`u64`/`f64` the way Rust does, so ingest-side
//! conversion (`py_to_measure_value`) needs a declared `MeasureType` per measure to know which
//! variant to build — that's exactly what `Schema` already tracks per measure, so `PyEngine`
//! looks it up from `schema.measure(id).data_type` before converting each value. This keeps the
//! "wrong type for this measure" error a proper `IngestError` (raised from Rust, matching what
//! `Engine::ingest` would already reject) rather than a silent coercion.

use accreta::measures::{MeasureId, MeasureType, MeasureValue};
use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;

#[pyclass(frozen, name = "MeasureId", from_py_object)]
#[derive(Clone, Copy)]
pub struct PyMeasureId(pub MeasureId);

#[pymethods]
impl PyMeasureId {
    #[new]
    fn new(index: u8) -> Self {
        Self(MeasureId(index))
    }

    #[getter]
    fn index(&self) -> usize {
        self.0.index()
    }

    fn __repr__(&self) -> String {
        format!("MeasureId({})", self.0.0)
    }

    fn __eq__(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

/// Convert one Python object into a `MeasureValue`, per the measure's declared `MeasureType`.
///
/// `bool` is deliberately rejected even though Python's `bool` is an `int` subclass and would
/// otherwise silently extract as `0`/`1` — that's almost never what's meant by "give me a
/// measure value".
pub fn py_to_measure_value(
    value: &Bound<'_, PyAny>,
    expected: MeasureType,
) -> PyResult<MeasureValue> {
    if value.is_instance_of::<pyo3::types::PyBool>() {
        return Err(PyTypeError::new_err("measure values cannot be bool"));
    }

    Ok(match expected {
        MeasureType::I64 => MeasureValue::I64(value.extract::<i64>()?),
        MeasureType::U64 => MeasureValue::U64(value.extract::<u64>()?),
        MeasureType::F64 => MeasureValue::F64(value.extract::<f64>()?),
    })
}

/// Convert a `MeasureValue` back into a Python object, for reading aggregate results out.
pub fn measure_value_to_py<'py>(py: Python<'py>, value: MeasureValue) -> Bound<'py, PyAny> {
    match value {
        MeasureValue::I64(v) => v.into_pyobject(py).unwrap().into_any(),
        MeasureValue::U64(v) => v.into_pyobject(py).unwrap().into_any(),
        MeasureValue::F64(v) => v.into_pyobject(py).unwrap().into_any(),
    }
}
