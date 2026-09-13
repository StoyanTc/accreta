# accreta-java

JNI bindings for `accreta`, binding directly to the Rust crate (same approach as
`accreta-node`/`accreta-py`) rather than going through `accreta-ffi`'s C ABI.

## Layout

```
accreta-java/
  Cargo.toml            # cdylib crate, depends on ../accreta
  src/
    lib.rs               # module wiring
    handles.rs            # opaque-handle helpers (Box<T> <-> jlong)
    error.rs               # SchemaError/IngestError -> thrown Java exceptions
    schema.rs               # SchemaBuilder, MeasureBuilder<'static, f64>, Schema natives
    engine.rs                # Engine natives (new/ingest/rollup/prune/queryRange)
    aggregate_set.rs          # AggregateSet natives (getSum/getCount/getMin/getMax/getQuantile)
  java/
    pom.xml
    src/main/java/com/accreta/
      NativeLibrary.java       # System.loadLibrary("accreta_java")
      SchemaBuilder.java         # mirrors SchemaBuilder
      MeasureBuilder.java          # mirrors MeasureBuilder<'a, f64>
      Schema.java                   # mirrors Schema
      Engine.java                    # mirrors Engine
      AggregateSet.java                # mirrors AggregateSet
      BucketLevel.java                  # mirrors BucketLevel (ordinal-matched!)
      Retention.java                    # mirrors Retention
      TDigest.java                      # standalone TDigest value (not tied to an AggregateSet)
      SchemaException.java              # mirrors SchemaError
      IngestException.java              # mirrors IngestError
      examples/
        BasicUsageExample.java           # port of basic_usage.rs
        TDigestQuantilesExample.java      # port of tdigest_quantiles.rs
```

## Scope (matches accreta-node/accreta-ffi's current scope)

- Built-in aggregates only: `Sum`, `Count`, `Min`, `Max`, `TDigest`.
- **f64 measures only.** i64/u64 measures aren't wired up yet — same shape as
  `schema.rs::nativeMeasureF64`, one native method per type, mirroring accreta-ffi's
  `register_measure_f64/i64/u64` split. `TDigest` will remain f64-only either way (its
  `Aggregator::Input` is fixed at `f64` on the Rust side).
- Ungrouped `query_range` only — `query_range_grouped` (returning a `HashMap<DimensionKey,
  AggregateSet>`) is a follow-up; it needs a `DimensionKey` Java wrapper first.

## Building

This crate hasn't been compiled against your actual `accreta` sources yet — it was written
against the files you shared, not built in CI here, so expect the first `cargo build` to surface
a few naming/signature mismatches to fix (most likely spots: exact `Min`/`Max`/`Average` method
names in `aggregates.rs`, and whether `f64`/`String` really implement `Into<MeasureValue>`
directly vs. needing `.into()` at a different point).

### macOS / Linux

```sh
# 1. Adjust the `accreta = { path = "../accreta" }` line in Cargo.toml if your workspace layout
#    differs from accreta-node/accreta-ffi's.
cd accreta-java
cargo build --release

# 2. Compile the Java sources.
cd java
javac -d target/classes $(find src/main/java -name '*.java')

# 3. Run one of the examples, with the native lib on the library path:
java -Djava.library.path=../../target/release -cp target/classes com.accreta.examples.BasicUsageExample
# (or com.accreta.examples.TDigestQuantilesExample, or your own class once you write one)
```

### Windows

Same three steps, just Windows-flavored: `cargo build --release` produces
`accreta_java.dll` instead of `libaccreta_java.so`/`.dylib` (the `[lib] name = "accreta_java"`
in `Cargo.toml` already accounts for this — Cargo adds the right prefix/extension per platform on
its own).

The macOS/Linux compile step above (`javac -d target/classes $(find src/main/java -name
'*.java')`) won't run as-is in PowerShell or cmd.exe: both have their own `find` on `PATH` — a
text-search tool, not Unix `find` — so `find src/main/java -name '*.java'` means something
different there and doesn't produce a file list. Each block below swaps that piece for the native
equivalent; the JVM invocation only needs path separators adjusted otherwise. (If you're building
from **Git Bash** or **WSL** instead of native PowerShell/cmd.exe, the original macOS/Linux block
works completely unchanged — it's only native Windows shells that need this.)

**PowerShell:**

```powershell
# 1. Same Cargo.toml path check as above.
cd accreta-java
cargo build --release

# 2. Compile the Java sources.
cd java
Get-ChildItem -Recurse -Filter *.java src\main\java | ForEach-Object { $_.FullName } > sources.txt
javac -d target\classes "@sources.txt"

# 3. Run one of the examples, with the native lib on the library path. The DLL is under
#    target\release, not target/release.
java "-Djava.library.path=..\..\target\release" -cp target\classes com.accreta.examples.BasicUsageExample
```

**cmd.exe**, if that's your shell instead:

```bat
cd accreta-java
cargo build --release

cd java
dir /s /b src\main\java\*.java > sources.txt
javac -d target\classes @sources.txt

java -Djava.library.path=..\..\target\release -cp target\classes com.accreta.examples.BasicUsageExample
```

Two Windows-specific gotchas worth knowing up front:

- **MSVC toolchain.** `cargo build` needs a linker; on Windows that means either the
  `stable-x86_64-pc-windows-msvc` Rust toolchain with the Visual Studio Build Tools (C++ build
  tools workload) installed, or the `-gnu` toolchain with a MinGW toolchain on `PATH`. If
  `cargo build` fails with a linker-not-found error, that's almost always this — `rustup show`
  will tell you which toolchain is active.
- **DLL discovery.** `-Djava.library.path` is the direct equivalent of `LD_LIBRARY_PATH`/
  `DYLD_LIBRARY_PATH` here, but Windows will also happily find `accreta_java.dll` if it's simply
  next to your `java.exe`/on `PATH`, or in the working directory you launch from — useful if
  `-Djava.library.path` ever seems to be ignored (a stale JVM-cached lookup is the usual culprit;
  restarting the JVM process fixes it).

## Example (mirrors `lib.rs`'s Quick Start)

```java
import com.accreta.*;
import java.time.Instant;

Schema schema;
try (SchemaBuilder builder = new SchemaBuilder()) {
    builder.dimension("host");
    builder.measureF64("value").withSum().withCount().done();
    schema = builder.build();
}

try (Engine engine = new Engine(schema)) {
    Instant t0 = Instant.parse("2026-03-15T10:05:00Z");
    engine.ingest(t0, new double[]{12.0}, new String[]{"server-a"});
    engine.ingest(t0.plusSeconds(60), new double[]{8.0}, new String[]{"server-a"});

    engine.rollup();

    try (AggregateSet hourTotal = engine.queryRange(
            BucketLevel.HOUR, t0, t0.plusSeconds(3600), /* measureId */ 0)) {
        System.out.println(hourTotal.getSum());   // 20.0
        System.out.println(hourTotal.getCount()); // 2
    }
}
schema.close();
```

## Examples

`examples/TDigestQuantilesExample.java` is a full port of `tdigest_quantiles.rs` — every part of
that example maps onto the current wrapper 1:1, including the free-standing `TDigest` merge-order
demonstration (section 4), via the new standalone `TDigest` class (`src/tdigest.rs` /
`TDigest.java`) — distinct from `AggregateSet.getQuantile()`, which only reads a digest already
living inside a queried set.

`examples/BasicUsageExample.java` ports `basic_usage.rs`'s ingest → rollup → query → retention
flow, **with one simplification**: the Rust original ingests several dimension values (browsers)
and prints a per-browser breakdown by iterating each `Bucket`'s `groups()` directly. Neither
`Bucket` nor `query_range_grouped`/`DimensionKey` are wrapped yet, so this port uses a single
dimension value throughout and reads results back with `Engine.queryRange`, which is only a
faithful substitute *because* there's a single group. The retention section (`Retention`,
`Engine(Schema, Retention)`, `prune()`) is fully wired up and ported without simplification.

## Known gaps / next steps

1. **Compile against your real `aggregates.rs`/`measures.rs`/`dimensions.rs`** — I only saw
   these through doc comments and usage sites, not their full source, so `Min<f64>::value()` /
   `Max<f64>::value()` / `count()` signatures, and `Retention::keep`'s
   exact `self`-by-value-vs-`Copy` shape, are inferred from usage, not confirmed against the
   trait/struct definitions themselves.
2. i64/u64 measure support.
3. `query_range_grouped` + a `DimensionKey`/`DimensionMask`/`Bucket` Java wrapper — this is what
   `BasicUsageExample`'s per-browser breakdown is waiting on.
4. Packaging the native library into the jar's resources so `System.loadLibrary` doesn't need
   `-Djava.library.path` at all (a small `NativeLibrary.load()` change to extract-and-load from
   a classpath resource, same trick napi-rs's prebuild step effectively does for Node).
