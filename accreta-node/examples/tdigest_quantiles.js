// Approximate quantiles with `tdigest`, mirroring accreta's own
// `examples/tdigest_quantiles.rs`, but from the Node.js side via accreta-node.
//
// Run with:
//
//   node examples/tdigest_quantiles.js

const { Engine } = require("../index"); // wherever napi-rs's generated entrypoint actually lives

// 1. Register `tdigest` alongside `count`/`average` on the same measure — same reasoning as
//    the Rust example: TDigest is heavier than the exact aggregates, so it only goes on the
//    one measure that actually needs quantiles. `valueType: "f64"` means tdigest registers
//    directly here; an "i64"/"u64" measure would work identically from this side too — the
//    shadow-measure indirection is entirely internal to Engine, see its module docs.
const engine = new Engine({
  dimensions: ["route"],
  measures: [
    {
      name: "request_latency_ms",
      valueType: "f64",
      aggregates: ["count", "average", "tdigest"],
    },
  ],
});

// 2. Ingest a spread of latency samples across a few minutes, all for the same route.
//    Ingestion looks identical to any other measure — tdigest doesn't change the call shape.
const t0 = Date.UTC(2026, 2, 15, 10, 0, 0); // month is 0-indexed, so 2 == March
const latenciesMs = [
  12, 15, 11, 20, 14, 9, 100, 13, 16, 12, 18, 250, 14, 15, 11, 13, 17, 12, 19, 500,
];
latenciesMs.forEach((latency, i) => {
  const t = t0 + i * 60_000;
  engine.ingest(t, [latency], ["/api/search"]);
});

// 3. Roll up to the hour, same as any other measure — tdigest merges through the rollup
//    hierarchy exactly like the exact aggregates do, it just doesn't guarantee an exact
//    answer at the end.
engine.rollup();

const hourStart = t0; // 10:00:00 UTC is already on an hour boundary
const hourEnd = hourStart + 60 * 60_000;

// 4. Read back the exact aggregates via queryRange, and the digest via the separate
//    queryRangeTDigest — tdigest isn't a field on AggregateResult (see Engine's module docs
//    for why: a quantile estimate needs a `q` parameter at query time, so it can't be
//    flattened into a plain number the way sum/min/max/average are).
const stats = engine.queryRange("hour", hourStart, hourEnd, 0);
const digest = engine.queryRangeTdigest("hour", hourStart, hourEnd, 0);

if (!digest) {
  throw new Error("expected a tdigest for request_latency_ms");
}

console.log(`samples ingested : ${stats.count}`);
console.log(`exact mean       : ${stats.average.toFixed(1)} ms`);
console.log(`p50 (median)     : ${digest.quantile(0.5).toFixed(1)} ms`);
console.log(`p95              : ${digest.quantile(0.95).toFixed(1)} ms`);
console.log(`p99              : ${digest.quantile(0.99).toFixed(1)} ms`);

// Same illustration as the Rust example: the mean is dragged upward by the outliers
// (100, 250, 500 ms) far more than the median is.
if (!(digest.quantile(0.5) < stats.average)) {
  throw new Error("median should sit below the outlier-skewed mean");
}

// NOTE: there is no JS equivalent of the Rust example's section 4 (manually building two
// TDigests with TDigest::identity()/update_in_place/merge_in_place to demonstrate approximate
// associativity). accreta-node deliberately doesn't expose bare TDigest construction or
// merging to JS at all — TDigestHandle only wraps an already-computed digest read back from
// the engine, with a single quantile(q) method. That's the same "only the fixed built-in
// surface is reachable from JS" scope decision the module docs describe for every aggregate,
// not something specific to tdigest — there's no way to demonstrate merge-order behavior
// from this side without accreta-node growing new API surface for it.
