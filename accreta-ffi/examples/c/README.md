# C example

This directory contains a small C application that exercises the public
`accreta-ffi` C ABI.

The example is intentionally built as a normal C consumer of the generated
header and native Rust library. It does not compile any Rust source itself.

## Prerequisites

You need:

- a Rust toolchain;
- a C compiler;
- CMake 3.15 or newer.

The C compiler can be GCC, Clang, Apple Clang, MSVC, or another compiler
supported by your CMake installation.

## Build the Rust library first

From the `accreta-ffi` repository root:

```bash
cargo build --release
```

This produces the native library under:

```text
target/release/
```

and generates the C header:

```text
include/accreta_ffi.h
```

The C example expects these files to exist before CMake configuration.

## Build with CMake

From the repository root:

```bash
cmake -S examples/c -B examples/c/build
cmake --build examples/c/build --config Release
```

CMake uses the shared `accreta-ffi` library by default.

### Linux

Run:

```bash
./examples/c/build/accreta_c_example
```

The CMake configuration sets the runtime search path so the example can find
the repository-local `libaccreta_ffi.so`.

### macOS

Run:

```bash
./examples/c/build/accreta_c_example
```

The CMake configuration sets the runtime search path so the example can find
the repository-local `libaccreta_ffi.dylib`.

### Windows

With Visual Studio or another multi-configuration generator:

```powershell
cmake -S examples/c -B examples/c/build
cmake --build examples/c/build --config Release
```

The generated `accreta_ffi.dll` is copied beside the executable, so the
example can be run without setting `PATH` manually:

```powershell
.\examples\c\build\Release\accreta_c_example.exe
```

With a single-configuration generator, the executable location depends on the
generator's normal CMake layout.

## Static linking

The example can also link against the Rust static library.

Configure with:

```bash
cmake -S examples/c -B examples/c/build-static \
    -DACCRETA_LINK_STATIC=ON
```

Then build:

```bash
cmake --build examples/c/build-static --config Release
```

The static library is expected at:

```text
target/release/libaccreta_ffi.a
```

on Unix-like systems and:

```text
target/release/accreta_ffi.lib
```

on Windows.

Static linking can require additional platform/system libraries, especially on
Windows. The supplied CMake configuration adds the Windows system libraries
needed by the Rust static library for the supported MSVC setup.

## What the example demonstrates

The example follows the normal C ABI usage pattern:

1. Create a schema builder.
2. Add dimensions.
3. Add measures and aggregate kinds.
4. Build the schema.
5. Create an engine from the schema.
6. Ingest timestamped measurements.
7. Roll up the ingested data.
8. Execute an ungrouped range query.
9. Read aggregate values.
10. Execute a grouped range query.
11. Iterate grouped results.
12. Release all returned ABI objects.

It also demonstrates the error boundary through
`accreta_last_error_message()`.

## Important ABI details demonstrated by the example

### Measure and dimension order

Dimensions are supplied in the same order in which they were registered in the
schema.

Measures are supplied in measure-ID order. Registration order determines the
measure ID.

For example:

```text
dimension 0 = host
dimension 1 = region

measure 0 = cpu
measure 1 = requests
```

### Query ranges

Range queries use inclusive boundaries:

```text
[start, end]
```

Timestamps are Unix timestamps in milliseconds.

### Grouping

Grouped queries use a dimension bit mask.

For example, if `host` is dimension 0:

```c
1ULL << 0
```

groups by host.

If `region` is dimension 1:

```c
1ULL << 1
```

groups by region.

Both dimensions can be selected with:

```c
(1ULL << 0) | (1ULL << 1)
```

### Dimension value IDs

Grouped results expose numeric dimension value IDs.

The current C ABI does not provide a reverse lookup from a value ID to the
original dimension string.

### Ownership

Objects returned by the ABI must be released using their corresponding
`*_free` function.

In particular:

```text
AccretaSchema              -> accreta_schema_free()
AccretaEngine              -> accreta_engine_free()
AccretaAggregateSet        -> accreta_aggregate_set_free()
AccretaGroupedQueryCursor  -> accreta_grouped_query_cursor_free()
AccretaDimensionKey        -> accreta_dimension_key_free()
```

Each key and aggregate set returned by a grouped-query cursor iteration is
independently allocated and must be released after use.

The engine clones the schema, so the schema handle can be released immediately
after creating the engine.

A successful schema build takes care of releasing the builder.

## Generated header

`accreta_ffi.h` is generated automatically by the Rust build using `build.rs`
and `cbindgen.toml`.

Do not edit the generated header manually.

The header contains `extern "C"` guards and can therefore be included from both
C and C++.

## Cleaning the build

To remove the CMake build directory:

```bash
rm -rf examples/c/build
```

On Windows, remove the directory using the normal Windows file-management
tools or PowerShell:

```powershell
Remove-Item -Recurse -Force examples\c\build
```

The CMake build directory is separate from Cargo's `target` directory.

## Source files

```text
examples/c/
├── CMakeLists.txt
├── README.md
└── example.c
```

`example.c` is the actual C consumer. `CMakeLists.txt` only supplies the
platform-specific build and linking configuration needed to compile it against
the Rust-produced library.
