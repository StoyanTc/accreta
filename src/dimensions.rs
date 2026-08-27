//! `DimensionMask` (GROUP BY selection) and `DimensionKey` (a resolved group's dimension
//! values) wrappers.
//!
//! # GAP — `Engine` doesn't expose dimension-value resolution
//!
//! `DimensionKey` as returned by `accreta` is a list of opaque `DimensionValueId`s (`u32`).
//! Resolving those back to the original strings needs `DimensionDictionaries::resolve`, but
//! that lives on `Engine`'s private `dictionaries` field — there's no public
//! `Engine::resolve_dimension(id, value) -> Option<&str>` (or similar) in what you've shared.
//! Until `accreta` core exposes one, `PyDimensionKey` below carries the **raw `u32` IDs**, not
//! the original strings — so a Python caller sees `{0: 3}` instead of `{"browser": "Firefox"}`.
//! That's usable (the IDs are stable and consistent within one `Engine`) but not ergonomic.
//! Cheapest fix: add a resolve method to `Engine` in the core crate; then `engine.rs` here can
//! resolve before handing keys to Python.

use accreta::dimensions::{DimensionId, DimensionMask};
use pyo3::prelude::*;

#[pyclass(name = "DimensionMask", from_py_object)]
#[derive(Clone, Copy, Default)]
pub struct PyDimensionMask(pub DimensionMask);

#[pymethods]
impl PyDimensionMask {
    #[new]
    fn new() -> Self {
        Self(DimensionMask::EMPTY)
    }

    /// Return a new mask with `dimension_index` added to the selection.
    fn with_dimension(&self, dimension_index: u8) -> Self {
        Self(self.0.with(DimensionId(dimension_index)))
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn __repr__(&self) -> String {
        format!("DimensionMask(bits={:#x})", self.0.bits())
    }
}

/// A GROUP BY key: the raw `DimensionValueId` for each dimension the query's `DimensionMask`
/// selected, in mask iteration order (ascending `DimensionId`). See the module-level GAP note —
/// these are opaque integers, not resolved strings, until `Engine` grows a resolve method.
/// Returned from `Engine.query_range_grouped`, not constructed directly.
#[pyclass(name = "DimensionKey", frozen, from_py_object)]
#[derive(Clone)]
pub struct PyDimensionKey {
    #[pyo3(get)]
    pub values: Vec<u32>,
}

#[pymethods]
impl PyDimensionKey {
    fn __repr__(&self) -> String {
        format!("DimensionKey(raw_values={:?})", self.values)
    }
}
