# accreta-py

PyO3 bindings for [`accreta`](../accreta) — a mergeable-state aggregation engine with
hierarchical time-series rollups:
```text
                                                         +--merge--> week buckets   (dead end)
                                                         |
minute --merge--> hour --merge--> day buckets -----------+
 buckets           buckets                               |
                                                         +--merge--> month buckets --merge--> year buckets
```

## Install

Built with [Maturin](https://www.maturin.rs/):

```bash
maturin develop            # build + install into the active virtualenv, for local iteration
maturin build --release    # produce a wheel in target/wheels/
```

## Quick start

```python
import accreta
from datetime import datetime, timezone

# 1. Describe which aggregates every bucket should track.
builder = accreta.SchemaBuilder()
builder.dimension("browser")
builder.measure("visits", "f64", ["sum", "count", "min", "max", "average"])
schema = builder.build()

engine = accreta.Engine(schema)

# 2. Ingest raw samples — always into the minute-level bucket.
t0 = datetime(2026, 6, 1, 9, 0, tzinfo=timezone.utc)
engine.ingest(t0, [21.5], ["Firefox"])
engine.ingest(t0, [22.0], ["Firefox"])

# 3. Roll up. This only merges bucket states — it never re-reads a sample.
engine.rollup()

# 4. Query. `query_range` merges every bucket in range across all dimension groups.
result = engine.query_range("hour", t0, t0, measure_index=0)
```

> `query_range`'s `range_end` above is illustrative only — pass a real end timestamp; see
> `examples/basic_usage.py` for a complete, runnable version of this walkthrough.

## API surface

| Python | Wraps | Notes |
|---|---|---|
| `SchemaBuilder` | `accreta::aggregate_set::SchemaBuilder` | `.dimension(name)`, `.measure(name, dtype, aggregates)`, `.build() -> Schema` |
| `Schema` | `accreta::aggregate_set::Schema` | `.dimension_count()`, `.measure_count()` |
| `Engine` | `accreta::engine::Engine` | `.ingest()`, `.rollup()`, `.prune()`, `.bucket_count()`, `.query_range()`, `.query_range_grouped()` |
| `AggregateSet` | `accreta::aggregate_set::AggregateSet` | `.values(dtype) -> dict` |
| `BucketLevel` | `accreta::bucket::BucketLevel` | enum; methods accepting a level also take the equivalent lowercase string |
| `DimensionMask` | `accreta::dimensions::DimensionMask` | `.with_dimension(index)` |
| `DimensionKey` | `accreta::dimensions::DimensionKey` | `.values` — **raw `u32` dimension-value IDs**, see Known limitations |
| `MeasureId` | `accreta::measures::MeasureId` | rarely needed directly — most methods take a plain `measure_index: int` instead |

`dtype` strings throughout are `"i64"`, `"u64"`, or `"f64"`. Aggregate name strings are `"sum"`,
`"min"`, `"max"`, `"count"`, `"average"`.

### Errors

`accreta::errors::IngestError` and `SchemaError` are raised as `accreta.IngestError` /
`accreta.SchemaError` (both `Exception` subclasses), with the Rust `Display` message preserved.
A handful of conditions that `panic!` in the Rust API (duplicate measure/aggregate names, more
than 64 dimensions) surface as PyO3's `pyo3_runtime.PanicException` rather than a typed
exception — these indicate a bug in schema construction, not a runtime data problem, so they're
left as panics rather than given their own Python exception type.

## Known limitations

- **`query_range_grouped` returns raw dimension-value IDs, not resolved strings.** `Engine`
  doesn't currently expose a way to resolve a `DimensionValueId` back to the string you
  ingested — that lives in a private `dictionaries` field with no accessor. Until `accreta` core
  gains something like `Engine::resolve_dimension(id, value) -> Option<&str>`, a `DimensionKey`
  you get back from Python is a list of `u32`s in dimension-registration order, not `{"browser":
  "Firefox"}`. The IDs are stable and consistent within one `Engine`, so this is usable for
  grouping/counting distinct groups — just not for printing human-readable labels without also
  tracking your own id -> string mapping on the Python side (or resolving via a separate
  lookup you maintain, since the mapping is exactly what you passed into `ingest()`).
- Schema/dimension/measure names passed from Python are leaked (`Box::leak`) to satisfy
  `accreta`'s `&'static str` signatures — fine for schemas built once at startup, not for
  building schemas in a loop (see the doc comment in `src/schema.rs`).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Licensed under either of

- Apache License, Version 2.0 (([LICENSE-APACHE](LICENSE-APACHE)))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
