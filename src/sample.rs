//! Incoming measurements.

use chrono::{DateTime, Utc};

use crate::{measures::MeasureValues, DimensionValues};

/// A single incoming time-series measurement.
///
/// A `Sample` is the only thing raw enough to be "reprocessed"; everything above the minute
/// bucket level is derived exclusively by merging [`crate::monoid::Monoid`] states, never by
/// looking at samples again. See the crate-level docs for why that distinction matters.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    /// When the measurement was taken.
    pub timestamp: DateTime<Utc>,
    /// Numeric measure values.
    pub measures: MeasureValues,
    /// Compact dimension values.
    pub dimensions: DimensionValues,
}

impl Sample {
    /// Create a new sample.
    pub fn new(
        timestamp: DateTime<Utc>,
        measures: MeasureValues,
        dimensions: DimensionValues,
    ) -> Self {
        Self {
            timestamp,
            measures,
            dimensions,
        }
    }

    /// Create a sample stamped with the current time.
    pub fn now(measures: MeasureValues, dimensions: DimensionValues) -> Self {
        Self {
            timestamp: Utc::now(),
            measures,
            dimensions,
        }
    }
}
