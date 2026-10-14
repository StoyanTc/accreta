# Changelog

All notable changes to the Accreta project are documented here.

## [0.4.1] - 2026-10-14

### Core (`accreta`)

#### Added

- `Schema::dimensions()`, `Schema::dimension(id)` and `Schema::dimension_ids()`, mirroring the
  existing measure accessors. Dimension names are now readable from a built `Schema`.
- `Engine::resolve_dimension_value(dim, id)`, which resolves an interned `DimensionValueId` back
  to the string it was ingested as. It returns `None` for an unknown dimension or a value id the
  engine never issued, rather than panicking.
- Tests for both, and for `TDigest::quantile` interpolating between interior centroids and
  being monotonic in `q`.

#### Changed

- **BREAKING:** `DimensionDefinition::name` and `MeasureDefinition::name` are now
  `Arc<str>` instead of `&'static str`. Code that reads these fields by value must
  borrow or clone them (e.g. `&def.name`, `def.name.clone()`, `&*def.name`).
- `SchemaBuilder::dimension` and `SchemaBuilder::measure` now accept
  `impl Into<Arc<str>>` instead of `&'static str`. Existing callers passing string
  literals continue to compile unchanged; `String`, `&str`, `Box<str>` and
  `Cow<str>` are now accepted as well. Passing `&String` requires `.as_str()` or
  `.clone()`.
- `TDigest::quantile` results differ from earlier versions for the same input (smoothly
  interpolated instead of stepped). Code or tests that asserted on exact quantile values
  from `0.4.0` or earlier may need updating. The extremes (`q <= 0` and `q >= 1`) are
  unchanged and still return the exact `min` and `max`.

#### Fixed

- Schemas no longer require callers to leak memory to build a `Schema` from runtime
  strings. Previously every language binding and the HTTP service had to call
  `Box::leak` on each dimension and measure name, and those allocations were never
  reclaimed, even after the `Schema` and `Engine` were dropped. Names are now owned
  by the schema and freed with it.
- Bindings and services no longer need to mirror the engine's dimension dictionaries or track
  dimension names separately in order to resolve query results.
- `TDigest::quantile` no longer returns a flat step function. For quantiles falling between
  interior centroids, the interpolation start and end were both the left centroid's mean, so
  every `q` inside a centroid's weight range returned that centroid's mean. Quantiles are now
  linearly interpolated between neighbouring centroid means, with each mean placed at the
  midpoint of its weight span, `min` anchored at the lower end and `max` at the upper end.
  This also fixes the first and last centroids being treated asymmetrically, and a digest
  with a single centroid could previously never return a value below its mean.

#### Unchanged

- `Aggregator::NAME` and `AggregateFactory::name()` remain `&'static str`, since
  they come from types rather than runtime input.

### Bindings

Updated to `0.3.1`. Bump the pinned `accreta` version to `0.4.1` in each binding's manifest.
`TDigest` quantile queries through `accreta-ffi`, `accreta-node`, `accreta-wasm`, `accreta-py`
and `accreta-java` pick up the interpolation fix automatically; no binding source changes
are required for it.

#### `accreta-ffi`

- **Fixed:** `accreta_schema_builder_add_dimension` and `accreta_schema_builder_add_measure`
  no longer leak the name. The C string is borrowed and copied once, only if the builder keeps
  it. Duplicate dimensions and rejected measures (e.g. TDigest on a non-F64 measure) no longer
  allocate.
- **Fixed:** `accreta_schema_free` now releases all memory associated with a schema.
- **Removed:** `leak_c_str`.

#### `accreta-node`

- **Fixed:** `Engine::new` no longer leaks dimension, measure, or `__tdigest_shadow__<name>`
  names, including when construction fails partway through.
- **Changed:** removed the mirrored dimension dictionaries (`dict_names`, `dict_lookup`) and the
  lock-step update in `ingest`; dimension values are now resolved through
  `accreta::Engine::resolve_dimension_value`.
- **Changed:** removed the "one-time intentional leak" and "mirrored dictionaries" caveats from
  the module docs. Constructing engines repeatedly is now safe.

#### `accreta-wasm`

- **Removed:** the `INTERNED` name table and `intern()`, which are no longer needed. Module docs
  updated accordingly.

#### `accreta-py`

- **Fixed:** `SchemaBuilder.dimension()` and `SchemaBuilder.measure()` no longer leak names.
- **Removed:** `leak_str`.

#### `accreta-java`

- **Fixed:** `nativeDimension` and `nativeMeasureF64` no longer leak names.

### Metrics (`accreta-metrics`)

Released as `0.3.1`.

#### Changed

- Updated the `accreta` dependency to `0.4.1`.
- Dimension names are read from `Schema::dimensions()` instead of being tracked separately in
  `SchemaMeta`.
- Removed `dispatch::leak`. `register_measure` now takes `&str`.
- Updated module docs: schema names are no longer leaked, so the one-shot `POST /schema`
  restriction is no longer a memory-safety requirement (it remains a v1 API restriction).
- Quantile (`tdigest`) values returned by the API are now interpolated rather than stepped,
  via the `accreta` `0.4.1` fix. Expect small differences from previous responses for the
  same data.

---

## [0.4.0] - 2026-09-24

(unchanged)

## [0.3.0] - 2026-09-21

(unchanged)

## [0.2.0] - 2026-09-13

### Core (`accreta`)

#### Removed

- Removed the `average` aggregate. The mean can be calculated from `sum / count`.
