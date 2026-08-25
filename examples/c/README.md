# C example

This directory contains a small C program that exercises the public
`accreta-ffi` C ABI.

The generated public header is expected at:

    ../../include/accreta_ffi.h

The example demonstrates:

- creating a schema builder;
- registering dimensions;
- registering typed measures and aggregate kinds;
- building a schema;
- creating an engine;
- ingesting samples;
- rolling up data;
- querying an hourly aggregate;
- reading sum, count, and average;
- executing a grouped query;
- iterating grouped results;
- releasing all owned FFI handles.

## Directory

    examples/c/
    ├── Makefile
    ├── README.md
    └── example.c

## Prerequisites

You need:

- a working Rust toolchain;
- the `accreta-ffi` crate built in debug mode;
- a C compiler;
- the generated `include/accreta_ffi.h`.

From the root of `accreta-ffi`, first build the library:

    cargo build

This should create the platform-specific library under:

    target/debug/

## Build the example

From this directory:

    make

or:

    make build

## Run

    make run

On Linux the Makefile sets `LD_LIBRARY_PATH` so the dynamic linker can
find the debug FFI library in `target/debug`.

On macOS the example uses the library search path supplied at link time.
If the library is built as a dynamic library and the runtime loader needs
an explicit path in your environment, set it accordingly.

On Windows, use the generated import/library files and ensure the DLL is
available on `PATH` when running the example.

## What the example does

The schema contains two dimensions:

    host
    region

and two measures:

    cpu      f64    sum, count, min, max, average
    requests u64    sum, count

Three samples are ingested:

    server-01 / eu-west    cpu=20    requests=100
    server-01 / eu-west    cpu=40    requests=200
    server-02 / eu-west    cpu=60    requests=300

The example then queries the hourly CPU aggregate and prints:

    CPU sum
    CPU count
    CPU average

It also performs a grouped query by `host`. Dimension zero is `host`, so
the grouping mask uses bit zero:

    1ULL << 0

Grouped query results expose dimension value IDs. The current example
prints those IDs rather than resolving them back to their original
dictionary strings.

## Ownership

The example intentionally demonstrates the ownership rules of the C ABI.

`accreta_schema_builder_build()` consumes the builder.

The schema can be released after creating the engine because the engine
keeps its own schema reference.

Query results are owned by the caller and must be released with their
corresponding `*_free()` functions.

Grouped query iteration returns owned dimension-key and aggregate-set
handles. Each pair is released after it has been processed, and the
cursor itself is released after iteration completes.

## Note

The example includes the generated public header rather than duplicating
any ABI declarations. If the generated header changes enum or field names,
the example should be updated to match that generated API.
