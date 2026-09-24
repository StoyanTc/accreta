# accreta-metrics

Reference/demo tokio+axum service exposing the [`accreta`](https://crates.io/crates/accreta)
mergeable-state aggregation engine over an OpenAPI HTTP surface. It also includes a deterministic
synthetic workload generator for continuously feeding realistic time-series data through the
public ingestion API.

## Requirements

- Rust 1.85+ (the `accreta` crate requires edition 2024), or
- Docker / Podman for containerized deployment.

## Getting started

### Native Run

```sh
cargo build
cargo run
# Swagger UI: http://localhost:8080/swagger-ui
```

### Docker Run

Build and run the container locally:

```sh
# Build image
docker build -t accreta-metrics .

# Run container exposing port 8080
docker run -d -p 8080:8080 --name accreta-metrics accreta-metrics
```

Optionally pass custom configuration via environment variables:

```sh
docker run -d -p 8080:8080 \
  -e ACCRETA_METRICS_USERNAME="admin" \
  -e ACCRETA_METRICS_PASSWORD="secure_password" \
  -e ACCRETA_METRICS_ROLLUP_INTERVAL_SECS="5" \
  --name accreta-metrics accreta-metrics
```

Env vars (all optional): `ACCRETA_METRICS_ADDR` (default `0.0.0.0:8080`),
`ACCRETA_METRICS_USERNAME` / `ACCRETA_METRICS_PASSWORD` (default `demo` / `demo123` — the one
seeded demo credential for v1), `ACCRETA_METRICS_ROLLUP_INTERVAL_SECS` (default `30` — how often
the background sweep rolls second buckets up through minute/hour/day/week/month/year; lower this for local
testing so you don't have to wait to query at a coarser level than you ingested at).

---


## Synthetic Data Generator

The repository includes a deterministic synthetic workload generator for local
development and end-to-end testing. It continuously generates realistic
time-series observations and sends them through the real HTTP ingestion API:

```text
accreta-generator
      │
      ▼
POST /login
      │
      ▼
POST /schema
      │
      ▼
POST /schema/ingest
      │
      ▼
accreta-metrics → accreta
```

This deliberately does **not** write directly to the storage/engine. Running
the generator through the public API exercises authentication, schema
validation, ingestion, dimension dictionaries, and the Accreta aggregation
engine just like a real client.

### Run the generator

Start the service first:

```sh
cargo run -p accreta-metrics
```

Then, from another terminal:

```sh
cargo run -p accreta-metrics --bin accreta-generator
```

The generator logs in automatically and creates the demo schema if it does not
already exist.

For faster local testing, use accelerated simulated time:

```sh
cargo run -p accreta-metrics --bin accreta-generator -- \
  --mode accelerated \
  --time-scale 60 \
  --events-per-second 100 \
  --seed 42
```

The `--time-scale 60` setting makes simulated time advance roughly 60 times
faster than wall-clock time, allowing hour/day/week/month/year rollups to be
populated without waiting in real time.

### Generator options

| Option | Default | Description |
|---|---|---|
| `--url` | `http://127.0.0.1:8080` | Metrics service URL |
| `--username` | `demo` | Login username |
| `--password` | `demo123` | Login password |
| `--events-per-second` | `100` | Number of generated observations per second |
| `--interval-ms` | `1000` | Generation interval in milliseconds |
| `--mode` | `realtime` | `realtime` or `accelerated` |
| `--time-scale` | `60` | Simulated-time multiplier in accelerated mode |
| `--seed` | `42` | Deterministic PRNG seed |
| `--scenario` | `normal` | `normal`, `traffic-spike`, `latency-spike`, `error-spike`, or `mixed` |
| `--batch-size` | `100` | Number of observations sent per HTTP request |

For example, to continuously generate incidents while quickly advancing
simulated time:

```sh
cargo run -p accreta-metrics --bin accreta-generator -- \
  --mode accelerated \
  --time-scale 60 \
  --scenario mixed \
  --events-per-second 100 \
  --seed 42
```

The generator uses a small deterministic PRNG and therefore does not require
an additional random-number dependency.

### Generated schema

The generator creates the following demo schema:

**Dimensions**

- `service`
- `region`
- `endpoint`
- `status`

**Measures**

- `request_count` (`u64`) — `sum`, `count`
- `latency_ms` (`f64`) — `sum`, `count`, `min`, `max`, `tdigest`
- `error_count` (`u64`) — `sum`, `count`

`latency_ms` includes endpoint/region-specific variation and bounded noise.
The synthetic workload also varies traffic and error rates so that the
dashboard can demonstrate grouping, filtering, rollups, and percentile
queries. It does not configure retention, so the demo schema keeps every
bucket forever — see [Configuring retention](#configuring-retention) below to
try it out against your own schema.

### Scenarios

The generator can periodically introduce synthetic incidents:

- `normal` — baseline traffic and latency.
- `traffic-spike` — approximately 4× traffic during incident periods.
- `latency-spike` — approximately 3× latency during incident periods.
- `error-spike` — approximately 10× error probability during incident periods.
- `mixed` — combines traffic, latency, and error-rate changes.

These scenarios are intended for dashboard/demo development rather than
benchmarking. Use a fixed `--seed` when reproducibility is useful.

### Suggested local workflow

For the complete development loop:

```text
Terminal 1                         Terminal 2
──────────                         ──────────
cargo run -p accreta-metrics      cargo run -p accreta-metrics --bin accreta-generator
                                      │
                                      ▼
                                  HTTP ingestion
                                      │
                                      ▼
                                  Accreta engine

Browser
   │
   ▼
Swagger UI / Svelte dashboard
   │
   ▼
query API
```

This generator is Stage 0 of the dashboard development plan. Stage 1 adds the
Svelte/TypeScript dashboard on top of the HTTP API. Stage 2 can add
`accreta-wasm` for client-side/offline aggregation without changing the
dashboard's core query model.

## Service Walkthrough

With the server running locally or in Docker, this walkthrough (the same sequence used to smoke-test the service)
creates a schema, ingests a few samples, and runs a couple of queries:

```sh
# 1. Log in and grab a token
TOKEN=$(curl -s -X POST http://localhost:8080/login \
  -H 'content-type: application/json' \
  -d '{"username":"demo","password":"demo123"}' | jq -r .token)

# 2. Create a schema (one-shot — a second call 409s). "retention" is optional; see
#    "Configuring retention" below for what it does — omit it entirely to keep every bucket
#    forever, as before.
curl -s -X POST http://localhost:8080/schema \
  -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' -d '{
    "name": "web_requests",
    "dimensions": ["host", "region"],
    "measures": [
      {"name": "latency_ms", "value_type": "f64", "aggregates": ["sum", "count", "tdigest"]},
      {"name": "request_count", "value_type": "i64", "aggregates": ["sum", "count"]}
    ],
    "retention": {"second": 3600, "minute": 604800}
  }'

# 3. Ingest a batch of samples
curl -s -X POST http://localhost:8080/schema/ingest \
  -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' -d '{
    "samples": [
      {"ts": "2026-08-01T10:05:00Z", "measures": [12.0, 1], "dimensions": ["server-a", "us-east"]},
      {"ts": "2026-08-01T10:06:00Z", "measures": [8.0, 1],  "dimensions": ["server-a", "us-west"]},
      {"ts": "2026-08-01T10:07:00Z", "measures": [100.0, 1],"dimensions": ["server-b", "us-east"]}
    ]
  }'

# 4. Query at "second" level — populated immediately, no wait needed
curl -s -X POST http://localhost:8080/schema/query \
  -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' -d '{
    "level": "second",
    "time_range": {"start": "2026-08-01T00:00:00Z", "end": "2026-08-02T00:00:00Z"},
    "group_by": ["region"],
    "select": [
      {"measure": "latency_ms", "aggregate": "sum"},
      {"measure": "latency_ms", "aggregate": "count"},
      {"measure": "latency_ms", "aggregate": "tdigest", "quantile": 0.95},
      {"measure": "request_count", "aggregate": "sum"}
    ]
  }'

# 5. Querying a coarser level ("minute", "hour", "day", ...) needs the background rollup sweep to
#    have run at least once first (default every 30s — see "Defaults and gotchas" below). Either
#    wait, or restart with ACCRETA_METRICS_ROLLUP_INTERVAL_SECS=2 for fast local iteration, then:
curl -s -X POST http://localhost:8080/schema/query \
  -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' -d '{
    "level": "hour",
    "time_range": {"start": "2026-08-01T00:00:00Z", "end": "2026-08-02T00:00:00Z"},
    "group_by": ["region"],
    "select": [{"measure": "latency_ms", "aggregate": "sum"}]
  }'
```

Or skip the `curl` and just open `http://localhost:8080/swagger-ui` — log in via `POST /login`
in the UI, hit "Authorize" with the returned token, and drive the same sequence from "Try it out".

## Configuring retention

`POST /schema` accepts an optional `retention` object, keyed by level name (`second`, `minute`,
`hour`, `day`, `week`, `month`, `year`) with a max-age value in whole seconds:

```json
{
  "name": "web_requests",
  "dimensions": ["host", "region"],
  "measures": [ /* ... */ ],
  "retention": {
    "second": 3600,
    "minute": 604800
  }
}
```

This example keeps `second`-level buckets for one hour and `minute`-level buckets for a week,
past the newest bucket currently stored at each of those levels — not wall-clock time, so replaying
historical data behaves the same way live ingestion does. A level omitted from `retention` is kept
forever, same as today. `GET /schema` echoes back whatever was configured. There's no endpoint to
change it after schema creation in v1 — like everything else about the schema, it's set once at
`POST /schema` time.

Retention is safe to configure on **any** level, including `second`, `minute`, `hour`, `day`, or
`month` — levels that feed a coarser rollup (`day` feeds both `week` and `month`; only `week` and
`year` feed nothing further). `accreta`'s background rollup is incremental: a bucket's contribution
is durably folded into its parent as soon as it's rolled up, so pruning it afterward never causes a
later sweep to recompute a coarser level from only whichever finer buckets happen to still exist.
The background sweep here always calls `rollup()` before `prune()` (see `rollup.rs`), which is what
makes this safe in practice.

Configuring `second`-level retention is the most common reason to reach for this at all: per the
"Defaults and gotchas" note below, `second` buckets are the most numerous by a wide margin, so
bounding how long they're kept is usually the first lever worth pulling if memory use from a
long-running deployment becomes a concern.

## Defaults and gotchas

Behavior that's correct but easy to get tripped up by, since none of it is obvious from the
endpoint shapes alone:

- **Querying a coarser level than you just ingested at returns `{"buckets":[]}`, not an error,
  until the background rollup sweep has run.** `Engine::ingest` only ever writes `second`
  buckets; `minute`/`hour`/`day`/`week`/`month`/`year` are only populated when `Engine::rollup()` runs,
  which happens on `rollup.rs`'s background timer (default every 30s,
  `ACCRETA_METRICS_ROLLUP_INTERVAL_SECS` to change it) — not inline on ingest. Each sweep tick
  fully cascades whatever changed at `second` all the way up to `year`, so it's a one-time wait
  after ingestion starts, not a per-level one. If a query comes back empty, try `"level": "second"`
  first to confirm the data's actually there before assuming something's wrong.
- **`second` is the finest level and the only one written on ingest, so it is also the most
  numerous.** Every distinct ingest timestamp (truncated to the second) gets its own bucket
  holding one aggregate state per dimension combination seen in that second, so memory grows
  faster than it did with `minute` as the base level — `tdigest` measures especially. `accreta`'s
  rollup is incremental (it only reprocesses buckets that actually changed, not the whole
  hierarchy on every sweep), so cost no longer scales with total bucket count the way a
  from-scratch rebuild would — but bucket *count itself* is still unbounded unless you configure
  retention on `second` (see [Configuring retention](#configuring-retention) above), which is the
  main reason that field exists.
- **`{"buckets":[]}` and `404 {"error":"no_schema"}` mean different things** — empty buckets means
  the schema exists but nothing (yet) matches the query (often the rollup-timing case above);
  `no_schema` means `POST /schema` was never called for this tenant at all.
- **CORS is wide open** (`CorsLayer::permissive()`) — fine for local/demo use, not something to
  point at anything less trusted without tightening it first.
- **JWT tokens expire after 30 minutes** (`TOKEN_TTL_MINUTES` in `auth.rs`) — not specified in the
  design summary beyond "short expiry"; 30 was picked here and isn't configurable via env var yet.

## Verification status

- `dispatch.rs`, `state.rs`'s shadow dictionaries, `schema_api.rs`, `ingest_api.rs`, and
  `query_api.rs` — the accreta-integration logic — were compiled *and run* against the real
  `accreta` 0.1.1 crate (via a local edition-2021-patched copy, to work around this sandbox's
  older toolchain). Output was hand-checked against the ingested data and was correct, including
  `filter` + `group_by` + multi-measure `select` + `tdigest` quantile together.
- The full HTTP surface was then compiled, run, and exercised end-to-end with `curl` (login,
  schema create/get, ingest incl. a validation-error case, query, 409 on duplicate schema, 401 on
  bad/missing token) against axum 0.7 + utoipa 4 (the newest versions this sandbox's rustc 1.75
  could resolve without hitting edition-2024 transitive dependencies) plus the real `accreta`
  crate. The one real API difference between axum 0.7 and 0.8 that was hit (`FromRequestParts`
  needing `#[async_trait]` on 0.7 vs. a plain `async fn` on 0.8, which is what's shipped here) is
  documented in `auth.rs`.
- `auth.rs`'s password hashing (`argon2` 0.6) and JWT-key generation were **not** compiled here —
  `argon2` 0.6's dependency chain also requires edition 2024. This code was instead verified by
  fetching and reading the exact published source of `argon2` 0.6.0, `password-hash` 0.6.0, and
  `phc` 0.6.0 from crates.io line-by-line (trait signatures, feature gates, re-export paths) after
  an initial version of this code — written against the older, more familiar `argon2` 0.5-era API
  — was found to reference `rand_core::OsRng`, which no longer exists in the `rand_core` 0.10 that
  `password-hash` 0.6 now pulls in.
- **`jsonwebtoken` 11 changed its default features**: unlike earlier versions, it no longer
  bundles a crypto backend by default (`default = ["use_pem"]` only). Without picking one, JWT
  signing/verification panics at runtime with a `CryptoProvider` message rather than failing to
  compile — this surfaced only when the service was actually run and `POST /login` was called, not
  during any of the compiles above. `Cargo.toml` now pins `jsonwebtoken`'s `rust_crypto` feature
  (the lighter, pure-Rust backend — sufficient since this service only ever uses HS256/HMAC, never
  RSA/EC/EdDSA, so there's no need for the heavier `aws_lc_rs` option). Confirmed by fetching and
  reading `jsonwebtoken` 11.0.0's actual source (`src/crypto/mod.rs`), which matches the panic
  message exactly.
- Given two runtime-only surprises have now turned up in dependencies that couldn't be compiled in
  this sandbox (`argon2`/`jsonwebtoken`), **please run the full `curl` walkthrough above once**
  after `cargo build` succeeds, rather than assuming a clean build means the auth path works too.
- **The `retention` field on `POST /schema` (`dispatch.rs`, `schema_api.rs`, `state.rs`) and
  `accreta`'s incremental `rollup()`/backstop `prune()` it depends on have not yet been compiled or
  run anywhere** — they were written against the `accreta` source as shared, not against a
  buildable checkout. Bump the `accreta` dependency to the version carrying the incremental-rollup
  change before relying on this, and re-run the `curl` walkthrough (steps 2 and 5 above exercise
  `retention` and the rollup timing it depends on) before treating it as verified.

## Notable implementation decisions not spelled out in the design summary

- `accreta::Schema` never exposes dimension names, only `dimension_count()` — this service tracks
  them itself (`state::SchemaMeta`). The same gap exists for retention (`accreta::Retention` never
  exposes back what was configured), tracked the same way in `SchemaMeta::retention`.
- `accreta::Engine` has no public way to resolve an interned dimension value back to its string,
  and `Engine::query_range_grouped` has no equality-filter parameter at all and only covers one
  measure per call. This service keeps its own shadow dictionaries in lockstep with every
  `Engine::ingest` call, and `query_api.rs` scans `Engine::buckets()` / `Bucket::groups()` (both
  public) directly rather than going through `query_range_grouped`. See the module docs at the top
  of `state.rs` and `query_api.rs` for the full reasoning.
- `TDigest::Aggregator::Input` is `f64` unconditionally, so schema-building and value-extraction
  dispatch (`dispatch.rs`) use separate f64/i64/u64 code paths rather than one generic function —
  a generic function that mentions `TDigest` won't compile for `T = i64`.
- Schema/measure/dimension names arriving as JSON `String`s are `Box::leak`'d into `&'static str`
  once, at schema-creation time, to satisfy `accreta::SchemaBuilder`'s API — sound here because v1
  schema creation is one-shot per process (`POST /schema` 409s on a second call).
- `dispatch::parse_level` is the single source of truth for bucket-level name strings, shared by
  `POST /schema/query`'s `level` field and `POST /schema`'s `retention` keys, so the two endpoints
  can never silently diverge on which level names are valid.

## Still open (per the design summary, not addressed here)

- Late/out-of-order ingest past a retention-pruned bucket.
- A health-check endpoint.
- Persistence (explicitly deferred to v2).


## Contributing

See [CONTRIBUTING.md](../CONTRIBUTING.md).

## License

Licensed under MIT license ([LICENSE-MIT](../LICENSE-MIT))
