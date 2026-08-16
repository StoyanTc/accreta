use thiserror::Error;

use crate::measures::{MeasureId, MeasureType};

#[derive(Debug, Error)]
pub enum IngestError {
    #[error("expected {expected} measures, got {actual}")]
    MeasureCount { expected: usize, actual: usize },

    #[error("measure {id:?} ('{name}') expects {expected}, got {actual}")]
    MeasureType {
        id: MeasureId,
        name: &'static str,
        expected: MeasureType,
        actual: MeasureType,
    },

    #[error("expected {expected} dimensions, got {actual}")]
    DimensionCount { expected: usize, actual: usize },
}

#[derive(Debug, Error)]
pub enum SchemaError {
    #[error("schema must define at least one dimension")]
    NoDimensions,

    #[error("schema must define at least one measure")]
    NoMeasures,

    #[error("invalid measure id: {0:?}")]
    InvalidMeasureId(MeasureId),
}
