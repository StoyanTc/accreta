# accreta-node

Node.js bindings for [`accreta`](../accreta), a mergeable-state aggregation engine with
hierarchical time-series rollups (`minute → hour → day → week/month → year`).

Built with [napi-rs](https://napi.rs), binding **directly to the `accreta` Rust crate** — not
through `accreta-ffi`'s C ABI. That avoids a second marshaling layer: napi-rs talks to Rust
structs directly and generates the JS/TypeScript glue for you.

## Status / scope

- Only the fixed set of **built-in aggregates** is exposed: `sum`, `count`, `min`, `max`,
  `average`. Custom/generic aggregates (the kind you'd register in pure Rust via `Monoid` +
  `Aggregator`) aren't reachable from JS, for the same reason `accreta-ffi` doesn't expose them:
  a JS caller can't hand you a Rust type at compile time. If you need a custom aggregate from
  Node, it has to be one of the built-ins, or you extend this crate's `register_measure` /
  `read_aggregates` match arms yourself for the new type.
- Measure values cross the boundary as JS `number` (`f64`). `i64`/`u64` measures are cast on the
  way in and out — fine for realistic sums/counts, but values near or beyond `2^53` will lose
  precision, same as any other JS number.
- **This has not been compiled or run** — it was written directly against the `accreta` source
  you shared, without a Rust toolchain available to verify it. Run `cargo check` (or
  `npm run build:debug`) as your first step and expect to fix a few rough edges.

## Layout

```
accreta-node/
  Cargo.toml       # path-depends on ../accreta — adjust if your layout differs
  build.rs         # required by napi-rs
  package.json     # napi-rs CLI build scripts
  src/lib.rs        # all bindings
  examples/basic_usage.js
  examples/tdigest_quantiles.js
```

This assumes `accreta-node` sits next to `accreta` (and `accreta-ffi`), matching the layout you
already have. If that's not the case, change the `path = "../accreta"` line in `Cargo.toml`.

## Building

```bash
npm install
npm run build        # release build, produces accreta-node.<platform>.node + index.js/index.d.ts
# or, for a faster iteration loop:
npm run build:debug
```

`napi build` auto-generates `index.js` and `index.d.ts` from the `#[napi]` annotations in
`src/lib.rs` — you don't hand-write those.

For distributing prebuilt binaries across platforms, use napi-rs's standard CI pattern: build a
matrix of `additional` triples (already listed in `package.json`) in CI, then `napi artifacts` +
`napi prepublish -t npm` to publish per-platform optional-dependency packages. See the [napi-rs CI
guide](https://napi.rs/docs/deep-dive/release) — the same pattern Prisma and `@swc/core` use.

## Running the example

`examples/basic_usage.js` requires the native addon to already be built — it does
`require("..")`, which resolves to this package's `index.js`, which in turn loads whichever
`accreta-node.<platform>.node` file `npm run build` produced.

```bash
npm install
npm run build          # or: npm run build:debug for a faster, unoptimized build
node examples/basic_usage.js
node examples/tdigest_quantiles.js
```

If `node examples/basic_usage.js` fails with a "cannot find module" error, `npm run build` didn't
produce `index.js`/`index.d.ts` in the package root — check the build step's output for errors
before rerunning.

## API

### `new Engine(schema)`

```ts
new Engine({
  dimensions: string[],
  measures: {
    name: string,
    valueType: "f64" | "i64" | "u64",
    aggregates: ("sum" | "count" | "min" | "max" | "average")[],
  }[],
  retention?: { level: BucketLevel, maxAgeMs: number }[], // optional
})
```

`BucketLevel` is `"minute" | "hour" | "day" | "week" | "month" | "year"`.

### Methods

| Method | Notes |
|---|---|
| `engine.ingest(timestampMs, measures: number[], dimensions: string[])` | Folds one sample into the minute bucket. Throws if lengths don't match the schema. |
| `engine.rollup()` | Merges every level upward. Idempotent — safe to call repeatedly. |
| `engine.prune()` | Discards buckets past their level's configured retention window. No-op for unconfigured levels. |
| `engine.bucketCount(level)` | Number of buckets currently stored at `level`. |
| `engine.buckets(level)` | All buckets at `level`, each with every dimension group's resolved aggregates. |
| `engine.queryRange(level, startMs, endMs, measureIndex)` | Merges all buckets overlapping the range into one `AggregateResult`, across all groups. |
| `engine.queryRangeGrouped(level, startMs, endMs, measureIndex, groupBy: string[])` | Same, but broken out by the named dimensions. `groupBy: []` gives one row (the grand total). |
| `engine.dimensionNames` / `engine.measureNames` | Getters, in schema registration order. |

`AggregateResult` fields (`sum`, `count`, `min`, `max`, `average`) are `null` when that aggregate
wasn't registered for the measure, and `min`/`max`/`average` are also `null` if the bucket/range
has no data yet — matching `accreta`'s own `Option`-returning `.value()` semantics.

See `examples/basic_usage.js` for a full walkthrough (ports the Rust `basic_usage.rs` example).

## Implementation notes worth knowing before you touch this

- **Names must be `'static`.** `accreta`'s `SchemaBuilder::dimension`/`measure` take `&'static
  str`. Since JS gives us owned `String`s, the constructor `Box::leak`s each dimension/measure
  name once. This is a small, one-time leak at schema-build time, not a per-request leak — don't
  call `new Engine(...)` in a hot loop.
- **Dimension-value strings are resolved via a mirrored dictionary.** `accreta::Engine` doesn't
  expose its internal `DimensionDictionary` publicly, so there's no built-in way to turn a stored
  `DimensionValueId` back into the original string. This wrapper keeps its own `Vec<Vec<String>>`
  per dimension, updated in `ingest()` using the same first-seen-gets-next-id scheme as
  `accreta`'s internal dictionary — since both are driven by the same calls in the same order, the
  ids always agree. If `accreta` grows a public dictionary accessor, this mirroring can go away.
- **Duplicate names / duplicate aggregate registration panic** in the underlying `accreta` crate
  (its `assert!`s), same as calling it from Rust directly. napi-rs converts Rust panics into JS
  exceptions by default, but it's still worth validating your schema client-side rather than
  relying on that.

## Alternatives considered

- **`accreta-ffi` + `koffi`/`ffi-napi`**: reuses the existing C ABI, but adds a second marshaling
  layer (Node → C ABI → Rust) for no benefit when Node has first-class native-addon tooling.
  Slower and less type-safe than binding directly.
- **`wasm-bindgen`**: portable (runs in-browser too), but pays a real cost at the JS↔WASM boundary
  for buffer-heavy data, and loses native threading — not a good fit for the bucket/rollup
  workload here unless browser support becomes a requirement.
- **`neon`**: same idea as napi-rs (native N-API addon in Rust), but napi-rs currently has better
  macro ergonomics and TypeScript generation, and is what Prisma/`@swc/core`/Parcel use.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Licensed under either of

- Apache License, Version 2.0 (([LICENSE-APACHE](../LICENSE-APACHE)))
- MIT license ([LICENSE-MIT](../LICENSE-MIT))

at your option.
