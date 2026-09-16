# accreta-metrics

Reference/demo tokio+axum service exposing the [`accreta`](https://crates.io/crates/accreta)
mergeable-state aggregation engine over an OpenAPI HTTP surface. See the design summary you
provided for the full endpoint/behavior spec this implements.

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
the background sweep rolls minute buckets up into hour/day/week/month/year; lower this for local
testing so you don't have to wait to query at a coarser level than you ingested at).

---

## Service Walkthrough

With the server running locally or in Docker, this walkthrough (the same sequence used to smoke-test the service)
creates a schema, ingests a few samples, and runs a couple of queries:

```sh
# 1. Log in and grab a token
TOKEN=$(curl -s -X POST http://localhost:8080/login \
  -H 'content-type: application/json' \
  -d '{"username":"demo","password":"demo123"}' | jq -r .token)

# 2. Create a schema (one-shot — a second call 409s)
curl -s -X POST http://localhost:8080/schema \
  -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' -d '{
    "name": "web_requests",
    "dimensions": ["host", "region"],
    "measures": [
      {"name": "latency_ms", "value_type": "f64", "aggregates": ["sum", "count", "tdigest"]},
      {"name": "request_count", "value_type": "i64", "aggregates": ["sum", "count"]}
    ]
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

# 4. Query at "minute" level — populated immediately, no wait needed
curl -s -X POST http://localhost:8080/schema/query \
  -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' -d '{
    "level": "minute",
    "time_range": {"start": "2026-08-01T00:00:00Z", "end": "2026-08-02T00:00:00Z"},
    "group_by": ["region"],
    "select": [
      {"measure": "latency_ms", "aggregate": "sum"},
      {"measure": "latency_ms", "aggregate": "count"},
      {"measure": "latency_ms", "aggregate": "tdigest", "quantile": 0.95},
      {"measure": "request_count", "aggregate": "sum"}
    ]
  }'

# 5. Querying at a coarser level ("hour", "day", ...) needs the background rollup sweep to have
#    run at least once first (default every 30s — see "Defaults and gotchas" below). Either wait,
#    or restart with ACCRETA_METRICS_ROLLUP_INTERVAL_SECS=2 for fast local iteration, then:
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

## Defaults and gotchas

Behavior that's correct but easy to get tripped up by, since none of it is obvious from the
endpoint shapes alone:

- **Querying a coarser level than you just ingested at returns `{"buckets":[]}`, not an error,
  until the background rollup sweep has run.** `Engine::ingest` only ever writes `minute`
  buckets; `hour`/`day`/`week`/`month`/`year` are only populated when `Engine::rollup()` runs,
  which happens on `rollup.rs`'s background timer (default every 30s,
  `ACCRETA_METRICS_ROLLUP_INTERVAL_SECS` to change it) — not inline on ingest. One sweep tick
  fully cascades minute all the way up to year, so it's a one-time wait, not a per-level one. If
  a query comes back empty, try `"level": "minute"` first to confirm the data's actually there
  before assuming something's wrong.
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

## Notable implementation decisions not spelled out in the design summary

- `accreta::Schema` never exposes dimension names, only `dimension_count()` — this service tracks
  them itself (`state::SchemaMeta`).
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

## Still open (per the design summary, not addressed here)

- Late/out-of-order ingest past a retention-pruned bucket.
- A health-check endpoint.
- Persistence (explicitly deferred to v2).


## Contributing

See [CONTRIBUTING.md](../CONTRIBUTING.md).

## License

Licensed under MIT license ([LICENSE-MIT](../LICENSE-MIT))
