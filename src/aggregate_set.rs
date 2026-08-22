//! [`AggregateSet`]: a collection of independently-tracked, named aggregate states.

use std::sync::Arc;

use crate::DimensionId;
use crate::aggregator::Aggregator;
use crate::erased::{AggregateFactory, ErasedState};
use crate::errors::SchemaError;
use crate::measures::{MeasureDefinition, MeasureId, MeasureNumber};
use crate::sample::Sample;

/// Defines a named dimension in the schema.
#[derive(Debug, Clone)]
pub struct DimensionDefinition {
    /// The dimension's identifier within the schema.
    pub id: DimensionId,
    /// The dimension's display name.
    pub name: &'static str,
}

/// The schema for an [`AggregateSet`]: which named aggregates it tracks.
///
/// A `Schema` is cheap to clone (it's an `Arc` under the hood) and is normally created once and
/// shared by an [`crate::engine::Engine`] across every bucket at every hierarchy level, which is
/// what keeps rollups correct: a child bucket and its parent always track exactly the same set
/// of aggregates.
#[derive(Debug, Clone)]
pub struct Schema {
    dimensions: Arc<Vec<DimensionDefinition>>,
    measures: Arc<Vec<MeasureDefinition>>,
}

impl Schema {
    /// A builder-style schema you can grow with [`SchemaBuilder::dimension`] and [`SchemaBuilder::measure`].
    pub fn builder() -> SchemaBuilder {
        SchemaBuilder::default()
    }

    /// Every measure registered in this schema, in registration order.
    pub fn measures(&self) -> impl Iterator<Item = &MeasureDefinition> {
        self.measures.iter()
    }

    /// Look up the definition for `id`, or `None` if it isn't registered in
    /// this schema.
    pub fn measure(&self, id: MeasureId) -> Option<&MeasureDefinition> {
        self.measures.get(id.0 as usize)
    }

    /// Instantiate a fresh, empty [`AggregateSet`] matching this schema.
    pub fn empty_set(&self, measure: MeasureId) -> Result<AggregateSet, SchemaError> {
        let definition = self
            .measure(measure)
            .ok_or(SchemaError::InvalidMeasureId(measure))?;

        let states = definition
            .factories
            .iter()
            .map(|factory| factory.identity())
            .collect();

        Ok(AggregateSet {
            schema: self.clone(),
            measure,
            states,
        })
    }

    /// The number of dimensions registered in this schema.
    pub fn dimension_count(&self) -> usize {
        self.dimensions.len()
    }

    /// The number of measures registered in this schema.
    pub fn measure_count(&self) -> usize {
        self.measures.len()
    }

    /// Iterates over every measure's ID, in registration order.
    pub fn measure_ids(&self) -> impl Iterator<Item = MeasureId> + '_ {
        (0..self.measures.len()).map(|i| MeasureId(i as u8))
    }
}

/// Ergonomic builder for [`Schema`].
#[derive(Default)]
pub struct SchemaBuilder {
    measures: Vec<MeasureDefinition>,
    dimensions: Vec<DimensionDefinition>,
}

impl SchemaBuilder {
    /// Register a dimension named `name`, assigning it the next available
    /// [`DimensionId`].
    ///
    /// Registering the same name twice is a no-op — the existing dimension
    /// is reused rather than re-registered.
    ///
    /// # Panics
    ///
    /// Panics if more than 64 dimensions have already been registered — a
    /// dimension mask can only represent 64 dimensions.
    pub fn dimension(&mut self, name: &'static str) -> &mut Self {
        if self.dimensions.iter().any(|d| d.name == name) {
            return self;
        }

        assert!(
            self.dimensions.len() < 64,
            "A Schema supports at most 64 dimensions"
        );

        let id = DimensionId(self.dimensions.len() as u8);

        self.dimensions.push(DimensionDefinition { id, name });

        self
    }

    /// Register a measure named `name` with numeric type `T`, returning a
    /// [`MeasureBuilder`] for attaching aggregates to it.
    ///
    /// # Panics
    ///
    /// Panics if another measure is already registered under `name`.
    pub fn measure<T>(&mut self, name: &'static str) -> MeasureBuilder<'_, T>
    where
        T: MeasureNumber,
    {
        let id = MeasureId(self.measures.len() as u8);

        assert!(
            !self.measures.iter().any(|m| m.name == name),
            "Schema: duplicate measure name '{name}'"
        );

        self.measures.push(MeasureDefinition {
            id,
            name,
            data_type: T::TYPE,
            factories: Vec::new(),
        });

        MeasureBuilder {
            schema: self,
            measure_id: id,
            _marker: std::marker::PhantomData,
        }
    }

    /// Finalize the builder into a [`Schema`].
    ///
    /// # Errors
    ///
    /// Returns [`SchemaError::NoDimensions`] if no dimension was registered,
    /// or [`SchemaError::NoMeasures`] if no measure was registered.
    pub fn build(self) -> Result<Schema, SchemaError> {
        if self.dimensions.is_empty() {
            return Err(SchemaError::NoDimensions);
        }

        if self.measures.is_empty() {
            return Err(SchemaError::NoMeasures);
        }

        Ok(Schema {
            measures: Arc::new(self.measures),
            dimensions: Arc::new(self.dimensions),
        })
    }
}

/// Builder for attaching aggregates to a single measure being registered on
/// a [`SchemaBuilder`].
///
/// Obtained from [`SchemaBuilder::measure`]; call [`Self::done`] to return
/// to the parent builder.
pub struct MeasureBuilder<'a, T> {
    schema: &'a mut SchemaBuilder,
    measure_id: MeasureId,
    _marker: std::marker::PhantomData<T>,
}

impl<'a, T> MeasureBuilder<'a, T>
where
    T: MeasureNumber,
{
    /// Register an aggregate whose input is the measure's own value type.
    ///
    /// # Panics
    ///
    /// Panics if another aggregate is already registered internally under `A::NAME` for
    pub fn with<A>(&mut self) -> &mut Self
    where
        A: Aggregator<Input = T> + Clone + 'static,
    {
        self.register::<A>()
    }

    /// Register an aggregate that ignores the value entirely (e.g. `Count`).
    ///
    /// # Panics
    ///
    /// Panics if another aggregate is already registered internally under `A::NAME`
    pub fn with_any<A>(&mut self) -> &mut Self
    where
        A: Aggregator<Input = ()> + Clone + 'static,
    {
        self.register::<A>()
    }

    /// Register aggregate type `A`.
    ///
    /// `A` is registered under its own [`Aggregator::NAME`] — there is no separate `name`
    /// parameter to independently supply, so a `Count` can never be registered under the string
    /// `"sum"`: the name isn't a choice made here at all, it's a property of `A`.
    ///
    /// # Panics
    ///
    /// Panics if another aggregate is already registered under `A::NAME` — including a second
    /// registration of `A` itself. Two aggregates silently sharing a name would mean one of them
    /// gets silently dropped from the resulting [`AggregateSet`] (only one state can live under
    /// a given name), so this is caught here instead, at the single place the collision was
    /// introduced.
    fn register<A>(&mut self) -> &mut Self
    where
        A: Aggregator + Clone + 'static,
    {
        let measure = self
            .schema
            .measures
            .iter_mut()
            .find(|m| m.id == self.measure_id)
            .expect("measure must exist");

        let name = A::NAME;
        assert!(
            !measure.factories.iter().any(|f| f.name() == name),
            "Measure: duplicate aggregate name '{name}' for measure '{}'",
            measure.name
        );

        measure.factories.push(AggregateFactory::new::<A>());
        self
    }

    /// Returns to the parent [`SchemaBuilder`], finishing this measure's
    /// registration.
    pub fn done(self) -> &'a mut SchemaBuilder {
        self.schema
    }
}
/// A named collection of aggregate states, all sharing one [`Schema`].
///
/// This is what a [`crate::bucket::Bucket`] holds: e.g. `{"sum": Sum(42.0), "count": Count(7),
/// "min": Min(1.0), "max": Max(9.0), "average": Average{...}}`. Adding a new aggregate type to
/// the system never requires changing this type — only registering one more
/// [`AggregateFactory`] in the [`Schema`]. Reading a value back out uses the type of the aggregate,
/// e.g. `set.get::<Sum>()`, rather than a name plus a turbofish.
///
/// States are stored in a plain `Vec`, in the same order as the [`Schema`]'s factories, rather
/// than a string-keyed map. A `Schema`'s shape never changes after `build()`, and every
/// `AggregateSet` sharing it always has one state per factory in the same order.
#[derive(Debug, Clone)]
pub struct AggregateSet {
    schema: Schema,
    measure: MeasureId,
    states: Vec<Box<dyn ErasedState>>,
}

impl AggregateSet {
    /// The schema this set was built from.
    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// The measure this set aggregates.
    pub fn measure(&self) -> MeasureId {
        self.measure
    }

    /// Fold a raw sample into every aggregate in this set, in place.
    ///
    /// This never allocates: each aggregate's existing `Box` is updated through
    /// [`ErasedState::update_erased_in_place`] rather than replaced.
    pub fn update(&mut self, sample: &Sample) {
        let value = sample
            .measures
            .get(self.measure)
            .expect("sample is missing the measure");

        for state in self.states.iter_mut() {
            state.update_erased_in_place(value);
        }
    }

    /// Merge another set of the same schema and measure into this one.
    pub fn merge(&mut self, other: &AggregateSet) {
        assert!(
            Arc::ptr_eq(&self.schema.measures, &other.schema.measures),
            "AggregateSet::merge: schema mismatch — both sets must be built from the same Schema"
        );

        assert_eq!(
            self.measure, other.measure,
            "AggregateSet::merge: measure mismatch — both sets must aggregate the same measure"
        );

        for (mine, theirs) in self.states.iter_mut().zip(other.states.iter()) {
            mine.merge_erased_in_place(theirs.as_ref());
        }
    }

    /// Merge two sets into a brand new one, leaving both inputs untouched.
    pub fn merged(&self, other: &AggregateSet) -> AggregateSet {
        let mut out = self.clone();
        out.merge(other);
        out
    }

    /// Look up the concrete state for a named aggregate of type `T`.
    ///
    /// This performs a linear scan over the aggregates registered for this
    /// set's measure.
    pub fn get<T>(&self) -> Option<&T>
    where
        T: Aggregator + 'static,
    {
        let definition = self.schema.measure(self.measure)?;

        let index = definition
            .factories
            .iter()
            .position(|f| f.name() == T::NAME)?;

        let state = self.states.get(index)?;

        state.as_any().downcast_ref::<T>()
    }

    /// Iterate over every named aggregate's erased state, in registration order.
    pub fn iter(&self) -> impl Iterator<Item = (&'static str, &dyn ErasedState)> {
        let definition = self
            .schema
            .measure(self.measure)
            .expect("AggregateSet contains an invalid MeasureId");

        definition
            .factories
            .iter()
            .map(|f| f.name())
            .zip(self.states.iter().map(|s| s.as_ref()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DimensionValues;
    use crate::aggregates::{Count, Sum};
    use crate::dimensions::DimensionDictionaries;
    use crate::measures::MeasureValue;
    use crate::measures::MeasureValues;
    use crate::sample::Sample;
    use chrono::Utc;

    fn schema() -> Schema {
        let mut builder = Schema::builder();
        builder
            .dimension("browser")
            .measure("visits")
            .with::<Sum<f64>>()
            .with_any::<Count>();
        builder.build().unwrap()
    }

    #[test]
    fn empty_set_starts_at_identity() {
        let schema = schema();
        let set = schema.empty_set(MeasureId(0)).unwrap();
        assert_eq!(set.get::<Sum<f64>>().unwrap().value(), 0.0);
        assert_eq!(set.get::<Count>().unwrap().value(), 0);
    }

    #[test]
    fn update_folds_into_every_aggregate() {
        let schema = schema();
        let mut set = schema.empty_set(MeasureId(0)).unwrap();
        let mut dictionaries = DimensionDictionaries::new(schema.dimension_count());
        let browser_id = dictionaries.dictionaries[0].get_or_insert("Firefox");
        set.update(&Sample::new(
            Utc::now(),
            MeasureValues::new(vec![MeasureValue::F64(10.0)]),
            DimensionValues::new(vec![browser_id]),
        ));
        set.update(&Sample::new(
            Utc::now(),
            MeasureValues::new(vec![MeasureValue::F64(20.0)]),
            DimensionValues::new(vec![browser_id]),
        ));
        assert_eq!(set.get::<Sum<f64>>().unwrap().value(), 30.0);
        assert_eq!(set.get::<Count>().unwrap().value(), 2);
    }

    #[test]
    fn merge_combines_matching_aggregates() {
        let schema = schema();
        let mut dictionaries = DimensionDictionaries::new(schema.dimension_count());
        let browser_id = dictionaries.dictionaries[0].get_or_insert("Firefox");
        let mut a = schema.empty_set(MeasureId(0)).unwrap();
        a.update(&Sample::new(
            Utc::now(),
            MeasureValues::new(vec![MeasureValue::F64(10.0)]),
            DimensionValues::new(vec![browser_id]),
        ));
        let mut b = schema.empty_set(MeasureId(0)).unwrap();
        b.update(&Sample::new(
            Utc::now(),
            MeasureValues::new(vec![MeasureValue::F64(5.0)]),
            DimensionValues::new(vec![browser_id]),
        ));
        b.update(&Sample::new(
            Utc::now(),
            MeasureValues::new(vec![MeasureValue::F64(5.0)]),
            DimensionValues::new(vec![browser_id]),
        ));

        a.merge(&b);
        assert_eq!(a.get::<Sum<f64>>().unwrap().value(), 20.0);
        assert_eq!(a.get::<Count>().unwrap().value(), 3);
    }

    #[test]
    #[should_panic(expected = "schema mismatch")]
    fn merge_panics_on_schema_mismatch() {
        let schema = schema();
        let mut a = schema.empty_set(MeasureId(0)).unwrap();
        let mut other_builder = Schema::builder();
        other_builder
            .dimension("browser")
            .measure("visits")
            .with::<Sum<f64>>();
        let other_schema = other_builder.build().unwrap();
        let b = other_schema.empty_set(MeasureId(0)).unwrap();
        a.merge(&b);
    }

    #[test]
    #[should_panic(expected = "duplicate aggregate name")]
    fn registering_the_same_type_twice_panics() {
        // Nothing here relies on a name string at all anymore (that hole is closed by
        // `Aggregator::NAME`), but registering the same aggregate type twice would still mean
        // one of the two states silently disappears from the resulting `AggregateSet` — so it's
        // caught here instead, at the point of registration.
        let mut builder = Schema::builder();
        builder
            .dimension("browser")
            .measure("visits")
            .with::<Sum<f64>>()
            .with::<Sum<f64>>();
    }
}
