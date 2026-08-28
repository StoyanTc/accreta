// Mirrors the Rust `basic_usage.rs` example: ingest a couple of hours of readings, roll them up,
// and read aggregates back at different granularities.
//
// Run with:
//   node examples/basic_usage.js
// (after `npm run build`, so the native addon exists)

const { Engine } = require("..");

const engine = new Engine({
  dimensions: ["browser"],
  measures: [
    {
      name: "visits",
      valueType: "f64",
      aggregates: ["sum", "count", "min", "max", "average"],
    },
  ],
});

// 1. Ingest some raw samples. Every sample only ever touches a minute-level bucket.
const start = Date.UTC(2026, 5, 1, 9, 0, 0); // 2026-06-01T09:00:00Z
const readings = [
  [0, 21.5, "Firefox"],
  [1, 22.0, "Firefox"],
  [3, 19.8, "Firefox"],
  [15, 23.1, "Firefox"],
  [47, 20.4, "Firefox"],
  [61, 25.0, "Firefox"], // rolls into the next hour
  [62, 24.7, "Firefox"],
  [125, 18.9, "Firefox"], // rolls into a third hour
];

for (const [minuteOffset, value, browser] of readings) {
  engine.ingest(start + minuteOffset * 60_000, [value], [browser]);
}
console.log(
  `ingested ${readings.length} raw samples into ${engine.bucketCount("minute")} minute buckets`
);

// 2. Roll everything up. This only merges bucket states upward — it never re-reads a sample.
engine.rollup();

// 3. Read aggregates back at whatever granularity is useful.
console.log("\nPer-hour breakdown:");
for (const bucket of engine.buckets("hour")) {
  for (const group of bucket.groups) {
    const agg = group.measures[0]; // "visits" is measure index 0
    const from = new Date(bucket.startMs).toISOString().slice(11, 16);
    const to = new Date(bucket.endMs).toISOString().slice(11, 16);
    console.log(
      `  [${group.dimensionValues.join(",")} ${from} .. ${to}) ` +
        `sum=${agg.sum.toFixed(2)} count=${agg.count} ` +
        `min=${agg.min.toFixed(2)} max=${agg.max.toFixed(2)} avg=${agg.average.toFixed(2)}`
    );
  }
}

console.log("\nWhole-day total (rolled all the way up):");
const dayStart = Date.UTC(2026, 5, 1, 0, 0, 0);
const dayBucket = engine.buckets("day").find((b) => b.startMs === dayStart);
if (dayBucket) {
  const agg = dayBucket.groups[0].measures[0];
  console.log(`  count=${agg.count} sum=${agg.sum.toFixed(2)} average=${agg.average.toFixed(2)}`);
}

// 4. Ad-hoc range query for the first two hours only — merges whichever buckets already exist,
//    without storing anything new.
console.log("\nAd-hoc query for the first two hours only:");
const range = engine.queryRange("hour", start, start + 2 * 3_600_000, 0);
console.log(`  count=${range.count} sum=${range.sum.toFixed(2)}`);

// 5. Grouped query — one row per distinct "browser" value.
console.log("\nGrouped by browser (whole range):");
for (const row of engine.queryRangeGrouped("hour", start, start + 3 * 3_600_000, 0, ["browser"])) {
  console.log(`  browser=${row.dimensionValues[0]} sum=${row.aggregate.sum.toFixed(2)}`);
}

// 6. Retention: keep only the last hour of minute-level detail.
console.log("\nRetention: keeping only the last hour of minute-level detail");
const boundedEngine = new Engine({
  dimensions: ["browser"],
  measures: [{ name: "visits", valueType: "f64", aggregates: ["sum", "count"] }],
  retention: [{ level: "minute", maxAgeMs: 3_600_000 }],
});
for (const [minuteOffset, value, browser] of readings) {
  boundedEngine.ingest(start + minuteOffset * 60_000, [value], [browser]);
}
console.log(`  before prune: ${boundedEngine.bucketCount("minute")} minute buckets`);
boundedEngine.prune();
console.log(`  after prune:  ${boundedEngine.bucketCount("minute")} minute buckets`);
