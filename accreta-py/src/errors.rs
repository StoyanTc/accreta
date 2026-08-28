//! Maps `accreta`'s Rust error types onto Python exceptions.
//!
//! We don't try to preserve the enum structure on the Python side (matching on
//! `err.variant`, etc.) — that's a lot of surface area for callers who mostly just want to
//! `try/except` and read `str(e)`. Each Rust error enum gets one Python exception type, and the
//! Rust `Display` message (via `thiserror`) becomes the exception's message.

use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::PyErr;

create_exception!(accreta, IngestError, PyException, "Raised when ingesting a sample fails schema validation (wrong measure count/type, or wrong dimension count).");
create_exception!(accreta, SchemaError, PyException, "Raised when a Schema fails to build, or a query references an invalid measure.");

pub fn ingest_err(err: ::accreta::errors::IngestError) -> PyErr {
    IngestError::new_err(err.to_string())
}

pub fn schema_err(err: ::accreta::errors::SchemaError) -> PyErr {
    SchemaError::new_err(err.to_string())
}
