use pyo3::prelude::*;

mod aggregate_set;
mod bucket;
mod dimensions;
mod engine;
mod errors;
mod measures;
mod schema;

use aggregate_set::PyAggregateSet;
use bucket::PyBucketLevel;
use dimensions::{PyDimensionKey, PyDimensionMask};
use engine::PyEngine;
use errors::{IngestError, SchemaError};
use measures::PyMeasureId;
use schema::{PyRetention, PySchema, PySchemaBuilder};

/// Python module entry point. The module name here ("accreta") must match `[lib] name` in
/// Cargo.toml for `maturin develop`/`maturin build` to produce an importable `accreta` module —
/// double check that against what you already set up in accreta-py's Cargo.toml.
#[pymodule]
fn accreta(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PySchemaBuilder>()?;
    m.add_class::<PySchema>()?;
    m.add_class::<PyRetention>()?;
    m.add_class::<PyEngine>()?;
    m.add_class::<PyBucketLevel>()?;
    m.add_class::<PyDimensionMask>()?;
    m.add_class::<PyDimensionKey>()?;
    m.add_class::<PyMeasureId>()?;
    m.add_class::<PyAggregateSet>()?;

    m.add("IngestError", py.get_type::<IngestError>())?;
    m.add("SchemaError", py.get_type::<SchemaError>())?;

    Ok(())
}
