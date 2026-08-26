# accreta-ffi

C ABI wrapper for the [`accreta`](https://crates.io/crates/accreta) Rust library.

`accreta-ffi` exposes accreta's core functionality through a C-compatible API so it can be used from C, C++, and other languages that can call a C ABI.

The complete ABI contract is defined by the generated `accreta_ffi.h` header.

## Quick start

### 1. Build the Rust library

From the repository root:

```bash
cargo build --release
```

This produces the native libraries under:

```text
target/release/
```

and generates:

```text
include/accreta_ffi.h
```

The crate produces:

* a shared library (`cdylib`);
* a static library (`staticlib`);
* a Rust library (`rlib`).

### 2. Build the C example

The repository contains a complete C consumer example in [`examples/c`](examples/c).

From the repository root:

```bash
cmake -S examples/c -B examples/c/build
cmake --build examples/c/build --config Release
```

The example uses the shared `accreta-ffi` library by default and works with
Linux, macOS, and Windows.

See [`examples/c/README.md`](examples/c/README.md) for platform-specific build
and runtime instructions, static linking, and ABI usage details.

## Using the C ABI

A C application needs:

1. `accreta_ffi.h`;
2. the native `accreta-ffi` library;
3. application code calling the ABI.

Include the generated header:

```c
#include "accreta_ffi.h"
```

The header is generated automatically by `build.rs` using `cbindgen.toml`.

**Do not edit `accreta_ffi.h` manually.**

The generated header contains `extern "C"` guards and can therefore also be
included from C++.

## API overview

The typical usage pattern is:

```text
SchemaBuilder
     │
     ▼
  Schema
     │
     ▼
  Engine
     │
     ├── ingest()
     │
     ├── rollup()
     │
     └── query()
             │
             ├── AggregateSet
             │
             └── GroupedQueryCursor
```

A schema defines the dimensions and measures accepted by the engine.

After creating an engine from the schema, data can be ingested, rolled up, and
queried.

## Schema

The schema builder provides the C-facing schema construction API.

A schema consists of:

* dimensions;
* measures;
* aggregate kinds associated with each measure.

For example:

```c
AccretaSchemaBuilder *builder = accreta_schema_builder_new();

accreta_schema_builder_add_dimension(builder, "host");
accreta_schema_builder_add_dimension(builder, "region");

const AccretaAggregateKind aggregates[] = {
    ACCRETA_AGGREGATE_KIND_SUM,
    ACCRETA_AGGREGATE_KIND_COUNT,
    ACCRETA_AGGREGATE_KIND_MIN,
    ACCRETA_AGGREGATE_KIND_MAX,
    ACCRETA_AGGREGATE_KIND_AVERAGE
};

AccretaSchema *schema = NULL;

accreta_schema_builder_add_measure(
    builder,
    "cpu",
    ACCRETA_MEASURE_TYPE_F64,
    aggregates,
    sizeof(aggregates) / sizeof(aggregates[0])
);

accreta_schema_builder_build(builder, &schema);
```

Measure IDs are assigned according to registration order.

For example:

```text
measure 0 = cpu
measure 1 = requests
```

Those IDs are subsequently used by the ingestion and query APIs.

### Builder ownership

A successful `accreta_schema_builder_build()` finalizes the schema and releases
the builder's memory.

The C caller does not need to free the builder after a successful build.

The resulting schema must eventually be released with:

```c
accreta_schema_free(schema);
```

## Engine

An engine is created from a schema:

```c
AccretaEngine *engine = accreta_engine_new(schema);
```

The engine clones the schema internally. The original schema can therefore be
released immediately after the engine has been created:

```c
AccretaEngine *engine = accreta_engine_new(schema);

accreta_schema_free(schema);
```

The engine owns its internal state until:

```c
accreta_engine_free(engine);
```

### Ingest

Data is ingested using:

```c
accreta_engine_ingest(...)
```

Dimensions are supplied in schema order, and measures are supplied in
measure-registration order.

For example:

```text
dimensions:
    0 = host
    1 = region

measures:
    0 = cpu
    1 = requests
```

The dimension and measure counts passed to the ingestion function must
correspond to the schema.

## Rollup

After data has been ingested, call:

```c
accreta_engine_rollup(engine);
```

This performs the configured rollup operations for the data stored in the
engine.

Rollup targets are defined by accreta's bucket hierarchy. A source level can
have multiple rollup targets; for example, day-level data can contribute
independently to week and month rollups.

## Queries

The C ABI provides both ungrouped and grouped range queries.

### Range boundaries

Query ranges use **inclusive** boundaries:

```text
[start, end]
```

Both `start` and `end` are Unix timestamps in milliseconds.

For example:

```c
AccretaAggregateSet *result = NULL;

accreta_engine_query_range(
    engine,
    ACCRETA_BUCKET_LEVEL_HOUR,
    start,
    end,
    0,
    &result
);
```

### Ungrouped query

An ungrouped range query returns an `AccretaAggregateSet`.

Individual aggregate values can then be retrieved:

```c
AccretaMeasureValue value;

accreta_aggregate_set_get_value(
    result,
    ACCRETA_AGGREGATE_KIND_SUM,
    ACCRETA_MEASURE_TYPE_F64,
    &value
);
```

When finished:

```c
accreta_aggregate_set_free(result);
```

## Aggregate values

The ABI represents aggregate results using C-compatible value types.

Some aggregate types have a result type independent of the original measure
type:

* `Count` is always returned as `u64`;
* `Average` is always returned as `f64`.

Average is represented as a computed `f64` result by the C API. Internally,
accreta maintains the mergeable average state as `(sum, count)`.

## Grouped queries

Grouped queries return a cursor:

```c
AccretaGroupedQueryCursor *cursor = NULL;

accreta_engine_query_range_grouped(
    engine,
    ACCRETA_BUCKET_LEVEL_HOUR,
    start,
    end,
    0,
    1ULL << 0,
    &cursor
);
```

The final argument is a dimension mask.

For example:

```text
dimension 0 = host
dimension 1 = region
```

Grouping by host:

```c
1ULL << 0
```

Grouping by region:

```c
1ULL << 1
```

Grouping by both:

```c
(1ULL << 0) | (1ULL << 1)
```

### Iterating grouped results

Each call to:

```c
accreta_grouped_query_cursor_next(...)
```

returns the next group.

The returned dimension key and aggregate set are independently allocated for
that result and must be released by the caller:

```c
accreta_dimension_key_free(key);
accreta_aggregate_set_free(group);
```

After iteration is complete, release the cursor:

```c
accreta_grouped_query_cursor_free(cursor);
```

## Dimension values

Dimension values supplied during ingestion are internally represented by
numeric value IDs.

A grouped query therefore exposes dimension IDs through
`AccretaDimensionKey`:

```c
uint32_t value_id = 0;

accreta_dimension_key_get(
    key,
    dimension_index,
    &value_id
);
```

The current C ABI does **not** provide a reverse lookup from a dimension value
ID back to its original string.

For example, a grouped result may expose a numeric ID corresponding internally
to:

```text
"server-01"
```

but the current ABI only provides the numeric ID.

## Strings and ownership

C strings passed into the ABI are borrowed for the duration of the call.

The caller does not need to keep those strings alive after the FFI call returns.

This is particularly important for schema names. The underlying
`accreta::Schema` uses `&'static str`; `accreta-ffi` therefore owns the backing
storage required to keep those strings alive for the lifetime of the relevant
schema/engine objects.

C callers do not directly allocate or release this Rust-owned memory.

## Ownership rules

Objects returned by the ABI are owned by the C caller and must be released
using their corresponding `*_free` function.

| Object                      | Release with                          |
| --------------------------- | ------------------------------------- |
| `AccretaSchema`             | `accreta_schema_free()`               |
| `AccretaEngine`             | `accreta_engine_free()`               |
| `AccretaAggregateSet`       | `accreta_aggregate_set_free()`        |
| `AccretaGroupedQueryCursor` | `accreta_grouped_query_cursor_free()` |
| `AccretaDimensionKey`       | `accreta_dimension_key_free()`        |

The engine clones the schema when it is created, so the schema handle may be
released immediately afterward.

Objects returned by each grouped-query cursor iteration are independently
allocated and must be released after use.

## Errors and panics

ABI functions return `AccretaStatus` values for operations that can fail.

A failed operation can be inspected using:

```c
accreta_last_error_message()
```

A typical error helper is:

```c
static void check(int32_t status, const char *operation)
{
    if (status == ACCRETA_STATUS_OK)
        return;

    fprintf(
        stderr,
        "%s failed: status=%d: %s\n",
        operation,
        status,
        accreta_last_error_message()
    );

    exit(EXIT_FAILURE);
}
```

Rust panics do not unwind through the C ABI.

Operations that execute Rust logic are protected by a panic boundary and convert
panics into an appropriate `AccretaStatus` value.

C applications should therefore treat a non-OK status as the failure boundary
and should not rely on Rust panics crossing the ABI.

## Generated header

`accreta_ffi.h` is generated automatically by the Rust build using `build.rs`
and `cbindgen.toml`.

The relevant files are:

```text
build.rs
cbindgen.toml
include/accreta_ffi.h
```

`accreta_ffi.h` is the consumer-facing ABI contract.

Do not manually edit the generated header. Changes to the ABI should be made in
the Rust source and reflected by regenerating the header.

## Source layout

The implementation is split into small modules rather than putting the
complete ABI in `src/lib.rs`.

```text
src/
├── lib.rs       # public ABI surface / re-exports
├── types.rs     # C-compatible types and opaque handles
├── support.rs   # unsafe-pointer, string, ownership, panic helpers
├── schema.rs    # schema builder functions
├── engine.rs    # engine lifecycle, ingest, rollup, prune
├── query.rs     # range queries and grouped-result lifecycle/access
├── aggregate.rs # aggregate state decoding
└── status.rs    # status and aggregate-name strings
```

### `types.rs`

Contains the C-facing data model:

* `AccretaStr`
* `AccretaStatus`
* `AccretaMeasureType`
* `AccretaAggregateType`
* `AccretaBucketLevel`
* `AccretaValue`
* `AccretaValueUnion`
* `AccretaAggregateValue`
* opaque Rust handles

### `schema.rs`

Contains the schema construction API:

* builder creation;
* dimension registration;
* measure registration;
* built-in aggregate registration;
* schema finalization;
* dimension and measure counts.

### `engine.rs`

Contains the mutable engine API:

* engine creation/destruction;
* ingestion;
* rollup;
* pruning;
* bucket counts.

### `query.rs`

Contains:

* range queries;
* grouped range queries;
* grouped-result iteration/access;
* aggregate-set lifecycle.

### `aggregate.rs`

Converts concrete Rust aggregate states into stable C-compatible
representations.

### `support.rs`

Contains shared FFI mechanics such as:

* raw pointer/count validation;
* UTF-8 conversion;
* ownership and destruction;
* panic boundaries;
* backing storage for schema strings.

### `status.rs`

Contains status handling and aggregate-name string support.

## Rust 2024

This crate uses Rust edition 2024.

Rust 2024 requires unsafe attributes such as `no_mangle` to be written
explicitly:

```rust
#[unsafe(no_mangle)]
pub extern "C" fn ...
```

The ABI uses this form throughout.

This is required because `no_mangle` affects the global symbol namespace and
therefore carries safety requirements that cannot be verified by the compiler.

## Development

After changing the Rust implementation, run:

```bash
cargo fmt
cargo check
cargo test
```

For a release build:

```bash
cargo build --release
```

To build and test the C example:

```bash
cmake -S examples/c -B examples/c/build
cmake --build examples/c/build --config Release
```

See [`examples/c/README.md`](examples/c/README.md) for the complete C example
documentation and platform-specific instructions.

## License

Licensed under either of

- Apache License, Version 2.0 (([LICENSE-APACHE](LICENSE-APACHE)))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
