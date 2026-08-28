use std::collections::HashMap;

/// Identifies a dimension in the schema.
///
/// Up to 64 dimensions can be represented by [`DimensionMask`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DimensionId(pub u8);

impl DimensionId {
    /// Returns the single-bit `u64` mask corresponding to this dimension's
    /// position, for use with [`DimensionMask`].
    pub fn bit(self) -> u64 {
        1u64 << self.0
    }
}

/// Selects dimensions participating in a GROUP BY.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DimensionMask(u64);

impl DimensionMask {
    /// A mask that selects no dimensions.
    pub const EMPTY: Self = Self(0);

    /// Creates an empty mask, equivalent to [`Self::EMPTY`].
    pub fn new() -> Self {
        Self::EMPTY
    }

    /// Returns a copy of this mask with `dimension` added to the selection.
    pub fn with(self, dimension: DimensionId) -> Self {
        Self(self.0 | dimension.bit())
    }

    /// Returns `true` if `dimension` is selected by this mask.
    pub fn contains(self, dimension: DimensionId) -> bool {
        self.0 & dimension.bit() != 0
    }

    /// Returns `true` if this mask selects no dimensions.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Returns the raw bitset backing this mask, with bit `i` set if
    /// `DimensionId(i)` is selected.
    pub fn bits(self) -> u64 {
        self.0
    }

    /// Iterates over the dimensions selected by this mask, in ascending
    /// `DimensionId` order.
    pub fn iter(self) -> impl Iterator<Item = DimensionId> {
        (0..64).filter_map(move |i| {
            if self.0 & (1u64 << i) != 0 {
                Some(DimensionId(i))
            } else {
                None
            }
        })
    }
}

/// Compact value of a dimension.
///
/// The actual string value is stored in the dimension dictionary,
/// not in every Sample.
pub type DimensionValueId = u32;

/// Dimension values belonging to a sample.
///
/// The position corresponds to DimensionId.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DimensionValues {
    values: Vec<DimensionValueId>,
}

impl DimensionValues {
    /// Wraps a vector of per-dimension values, indexed by `DimensionId`.
    pub fn new(values: Vec<DimensionValueId>) -> Self {
        Self { values }
    }

    /// Returns the value for `dimension`, or `None` if this sample has no
    /// value at that position (e.g. the vector is shorter than expected).
    pub fn get(&self, dimension: DimensionId) -> Option<DimensionValueId> {
        self.values.get(dimension.0 as usize).copied()
    }

    /// Returns the underlying per-dimension values, indexed by `DimensionId`.
    pub fn values(&self) -> &[DimensionValueId] {
        &self.values
    }

    /// Build a key containing all dimensions, in DimensionId order.
    ///
    /// Buckets store this full key so that a later query can choose an
    /// arbitrary GROUP BY projection without requiring additional copies
    /// of the aggregation state at ingestion time.
    pub fn full_key(&self) -> DimensionKey {
        DimensionKey {
            values: self.values.clone(),
        }
    }

    /// Build a compact group key for the dimensions selected by `mask`.
    ///
    /// This is useful when a caller explicitly wants a projected key. The
    /// bucket storage path normally uses [`Self::full_key`] instead.
    pub fn group_key(&self, mask: DimensionMask) -> DimensionKey {
        let values = mask
            .iter()
            .map(|dimension| {
                self.get(dimension)
                    .expect("sample is missing a registered dimension")
            })
            .collect();

        DimensionKey { values }
    }
}

/// A concrete combination of dimension values.
///
/// Stored bucket keys contain all dimensions in DimensionId order. Query
/// results may contain a projected subset of those values.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DimensionKey {
    values: Vec<DimensionValueId>,
}

impl DimensionKey {
    /// Returns the values making up this key, in the order they were built
    /// (full keys are in `DimensionId` order; projected keys follow the
    /// mask's iteration order).
    pub fn values(&self) -> &[DimensionValueId] {
        &self.values
    }

    /// Project a full bucket key onto the dimensions selected by `mask`.
    ///
    /// The key must contain values for every dimension referenced by `mask`.
    pub fn project(&self, mask: DimensionMask) -> DimensionKey {
        let values = mask
            .iter()
            .map(|dimension| {
                self.values
                    .get(dimension.0 as usize)
                    .copied()
                    .expect("dimension key is missing a registered dimension")
            })
            .collect();

        DimensionKey { values }
    }
}

/// Maps human-readable dimension values to compact integer IDs.
///
/// One dictionary exists per dimension.
#[derive(Debug, Clone, Default)]
pub struct DimensionDictionary {
    values: HashMap<String, DimensionValueId>,
    names: Vec<String>,
}

impl DimensionDictionary {
    /// Returns the ID for `value`, inserting it with a freshly allocated ID
    /// if it hasn't been seen before.
    pub fn get_or_insert(&mut self, value: &str) -> DimensionValueId {
        if let Some(&id) = self.values.get(value) {
            return id;
        }

        let id = self.names.len() as DimensionValueId;

        self.values.insert(value.to_owned(), id);
        self.names.push(value.to_owned());

        id
    }

    /// Returns the ID already assigned to `value`, or `None` if it hasn't
    /// been inserted.
    pub fn get(&self, value: &str) -> Option<DimensionValueId> {
        self.values.get(value).copied()
    }

    /// Returns the string value assigned to `id`, or `None` if `id` was
    /// never allocated by this dictionary.
    pub fn resolve(&self, id: DimensionValueId) -> Option<&str> {
        self.names.get(id as usize).map(String::as_str)
    }
}

/// One [`DimensionDictionary`] per dimension in the schema, indexed by
/// `DimensionId`.
#[derive(Debug, Clone)]
pub struct DimensionDictionaries {
    pub dictionaries: Vec<DimensionDictionary>,
}

impl DimensionDictionaries {
    /// Creates `dimension_count` empty dictionaries, one per dimension.
    pub fn new(dimension_count: usize) -> Self {
        Self {
            dictionaries: (0..dimension_count)
                .map(|_| DimensionDictionary::default())
                .collect(),
        }
    }

    /// Returns the ID for `value` under `dimension`, inserting it into that
    /// dimension's dictionary if it hasn't been seen before.
    pub fn get_or_insert(&mut self, dimension: DimensionId, value: &str) -> DimensionValueId {
        self.dictionaries[dimension.0 as usize].get_or_insert(value)
    }

    /// Returns the string value assigned to `value` under `dimension`, or
    /// `None` if `dimension` is out of range or `value` was never allocated.
    pub fn resolve(&self, dimension: DimensionId, value: DimensionValueId) -> Option<&str> {
        self.dictionaries.get(dimension.0 as usize)?.resolve(value)
    }
}
