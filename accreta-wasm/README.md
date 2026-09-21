# accreta-wasm

WebAssembly bindings for [`accreta`](../accreta), generated with [`wasm-bindgen`](https://rustwasm.github.io/wasm-bindgen/). Lets you build and query an `accreta` aggregation engine from JavaScript or TypeScript, in the browser or in Node via `wasm-bindgen`'s Node target.

This binds directly to the `accreta` crate — not through `accreta-ffi`'s C ABI — the same scope decision as [`accreta-node`](../accreta-node). Only the fixed set of built-in aggregates is exposed:

- `sum`, `count`, `min`, `max` — plain numeric aggregates
- `tdigest` — approximate quantiles, returned as a handle you query with `.quantile(q)`

Custom/generic aggregate state isn't reachable from JS, since a JS caller can't supply a Rust type at compile time.

## Status

This crate has **not yet been build-verified** against a real `accreta` checkout — see the "Known gaps" section below before relying on it.

## Building

Requires [`wasm-pack`](https://rustwasm.github.io/wasm-pack/):

```bash
# Browser (bundler-friendly ESM output)
wasm-pack build --target bundler

# Node.js
wasm-pack build --target nodejs

# Browser, no bundler (script-tag friendly)
wasm-pack build --target web
```

Pick the target that matches how you plan to consume the package; `wasm-pack` produces a different `pkg/` layout for each.

## Quick start

```js
import init, { Engine } from "./pkg/accreta_wasm.js";

await init(); // instantiate the wasm module — only needed for the `web`/`bundler` targets

const engine = new Engine({
  dimensions: ["region", "host"],
  measures: [
    { name: "request_latency_ms", valueType: "f64", aggregates: ["count", "sum", "tdigest"] },
    { name: "error_count", valueType: "u64", aggregates: ["sum", "count"] },
  ],
  retention: [
    { level: "second", maxAgeMs: 60 * 60 * 1000 },        // keep 1 hour of second buckets
    { level: "hour", maxAgeMs: 7 * 24 * 60 * 60 * 1000 }, // keep 7 days of hour buckets
  ],
});

// Raw samples always land in `second` buckets.
engine.ingest(Date.now(), [42.5, 1], ["us-east", "host-01"]);

// Every coarser level (minute, hour, ...) is derived by rollup().
engine.rollup();

const totals = engine.queryRange("minute", startMs, endMs, /* measureIndex */ 0);
// totals: { sum, count, min, max } — plain JS object

const digest = engine.queryRangeTdigest("minute", startMs, endMs, 0);
if (digest) {
  console.log("p99:", digest.quantile(0.99));
}

const byRegion = engine.queryRangeGrouped("minute", startMs, endMs, 0, ["region"]);
// byRegion: [{ dimensions, dimensionValues, aggregate }, ...]

// The mean isn't an aggregate of its own: derive it from sum and count.
const mean = totals.count ? totals.sum / totals.count : null;
```

> The retention policy above is only safe in an ingest → `rollup()` → `prune()` → read flow. See
> "Bucket levels and retention" below before combining retention with periodic rollups.

## Bucket levels and retention

`BucketLevel` runs `second, minute, hour, day, week, month, year` (finest to coarsest). Raw
samples are always ingested into `second` buckets; every other level is derived by
`engine.rollup()`, so until the first `rollup()` only `"second"` holds data.

`rollup()` rebuilds every level above `second` from the level below on each call. That has one
important consequence for retention: once `prune()` has dropped old `second` (or `minute`, ...)
buckets, the *next* `rollup()` recomputes the coarser levels from only the surviving buckets, and
the pruned data disappears from them as well. Until `accreta` gains an incremental rollup, use
retention only in a batch-style flow (ingest → `rollup()` → `prune()` → read), not in a loop that
keeps calling `rollup()` after pruning.

## API notes

- **Field names are camelCase** on the JS side (`valueType`, `maxAgeMs`, `dimensionValues`, etc.) via `serde`'s `rename_all`, matching `accreta-node`'s convention.
- **`buckets()`, `queryRange()`, and `queryRangeGrouped()` return plain JS values** (objects/arrays), not typed classes — see "Differences from accreta-node" below for why.
- **`Engine` and `TDigestHandle` are real JS classes** (backed by opaque Rust handles), same as their `accreta-node` counterparts.
- **`ingest()` truncates timestamps to the second** — sub-second precision is discarded.
- **Errors surface as thrown JS exceptions** (`Error` instances), not error codes or `Result`-shaped returns.

## Differences from `accreta-node`

If you're familiar with `accreta-node`, most of the API maps over directly. A few things had to change because of how `wasm-bindgen` (vs. `napi-rs`) marshals data:

| | `accreta-node` | `accreta-wasm` | Why |
|---|---|---|---|
| Nested struct results (`buckets`, `queryRange`, `queryRangeGrouped`) | Typed `#[napi(object)]` structs | `JsValue` built via `serde-wasm-bindgen` | `wasm-bindgen` has no equivalent to napi's auto-marshaled `Vec<CustomStruct>` |
| Dimension/measure name storage | Unconditional `Box::leak` per `Engine::new` call | Interned (deduped) leak — see below | A browser tab can outlive many `Engine` rebuilds (e.g. hot-reload); Node processes are shorter-lived and this was a smaller risk there |
| Concurrency primitive for internal caches | `Mutex`/none needed | `thread_local!` + `RefCell` | `wasm32-unknown-unknown` is single-threaded by default (no SharedArrayBuffer/threads proposal) |
| Errors | `napi::Error` | `wasm_bindgen::JsError` | Native to each binding generator |

## Known gaps

- **Compile-time verification.** This crate was hand-ported from `accreta-node` and has not yet been run through `cargo check`/`wasm-pack build` against a real `accreta` checkout. Expect minor path/visibility fixups on first build.
- **`tdigest` compression default is still config-file-backed in `accreta` core**, which has no filesystem access in a browser. This needs an upstream fix in `accreta` core (moving the default to a `build.rs`-generated compile-time constant, plus ideally an explicit per-measure override) — not something this wrapper alone can resolve. Until that lands, registering `tdigest` will fail or panic wherever `accreta` core's current file read happens.
- **Name interning bounds leaks by distinct names, not by `Engine`s created.** If your application registers genuinely new dimension/measure names over time (rather than rebuilding the same schema repeatedly), each new name still leaks for the life of the WASM instance — nothing here reclaims memory on `Engine` drop. A from-scratch fix would need `accreta` core to accept non-`'static` names via an arena tied to `Engine`'s own lifetime, which is a breaking change to `accreta` core's public API, not a wrapper-level fix.
- **No TypeScript `.d.ts` beyond what `wasm-bindgen`/`wasm-pack` auto-generates for the `#[wasm_bindgen]` classes.** `SchemaSpec`/`MeasureSpec`/`RetentionSpec` and the various result shapes are plain `JsValue` on the wire, so consumers get no compile-time shape-checking for them unless you hand-write or generate types (e.g. via `tsify`) separately.
- **There is no `average` aggregate.** It was removed from `accreta` in 0.2.0 because it is derivable from `sum` and `count`; register both on the measure and compute `sum / count` on the JS side. Requesting `"average"` in a schema is rejected as an unknown aggregate.
- **Memory grows faster with `second` as the base level.** Every distinct ingest second gets its own bucket holding one aggregate state per dimension combination seen in it (a `tdigest` per group, if registered), and a browser tab has a small memory budget. Keep the retention caveat above in mind, and prefer ingesting pre-batched samples over one call per event when volume is high.

## Retention levels

One of: `"second"`, `"minute"`, `"hour"`, `"day"`, `"week"`, `"month"`, `"year"`. Matches `accreta::BucketLevel`.

## Value types

One of `"f64"`, `"i64"`, `"u64"` for `MeasureSpec.valueType`. `tdigest` works on any of these — for `i64`/`u64` measures, an internal shadow `f64` measure is registered and populated automatically; this is invisible from the JS side (it never shows up in `measureNames` or bucket results).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Licensed under either of

- Apache License, Version 2.0 (([LICENSE-APACHE](../LICENSE-APACHE)))
- MIT license ([LICENSE-MIT](../LICENSE-MIT))

at your option.
