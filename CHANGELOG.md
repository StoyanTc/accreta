# Changelog

All notable changes to the Accreta project are documented here.

## [0.4.0] - 2026-09-24

### Core (`accreta`)

#### Changed

* `Engine::rollup()` is now incremental: only the coarser buckets whose children changed since the last call are recomputed, instead of rebuilding every level above `Second` from scratch on every call. Calling it repeatedly with nothing newly ingested is now a cheap no-op.
* `Engine::prune()` now refuses to discard a bucket that hasn't been rolled up into its parent yet, even if it is past its retention cutoff. This is a backstop for `prune()` being called before `rollup()`; the documented `rollup()`-then-`prune()` order is unaffected.

#### Fixed

* Configuring `Retention` on a non-leaf level (`Second`, `Minute`, `Hour`, `Day`, or `Month` — any level that feeds a coarser rollup) is now safe. Previously, pruning one of these levels could cause the next `rollup()` to silently recompute every coarser level from only the buckets that survived pruning, discarding already-merged history. `Week` and `Year` were always safe; this now holds for every level.

#### Added

* Tests covering incremental rollup (no-op re-rolls, late updates to an already-rolled-up bucket, day fan-out to week/month), and the new prune-before-rollup backstop.

### Bindings

No functional changes in this release — `accreta`'s public `Engine`/`Retention` signatures are unchanged, only `rollup()`'s and `prune()`'s internal behavior. Bump the pinned `accreta` version in each binding's manifest to pick up the fix; no binding source changes are required.

### Metrics (`accreta-metrics`)

Released as `0.3.0`.

#### Added

* `POST /schema` accepts an optional `retention` field: a map of level name (`second`, `minute`, `hour`, `day`, `week`, `month`, `year`) to max-age in seconds. A level not listed is kept forever, as before. `GET /schema` echoes back whatever was configured.

#### Changed

* Updated the `accreta` dependency to `0.4.0` — required for the `retention` field to be safe on non-leaf levels.

## [0.3.0] - 2026-09-21

### Core (`accreta`)

#### Added

* Added support for second-level time aggregation.
* Added support for custom second-based aggregation periods, such as 15, 30, and 45 seconds.
* Extended time-bucket handling to support sub-minute aggregation while preserving existing larger aggregation periods.

#### Changed

* Updated documentation and examples for second-based aggregation.
* Added tests covering second-level and custom-period aggregation.

### Bindings

Updated the following bindings to `0.3.0`:

* `accreta-ffi`
* `accreta-py`
* `accreta-node`
* `accreta-java`
* `accreta-wasm`

The bindings expose the new temporal-resolution functionality from `accreta` 0.3.0.

### Metrics (`accreta-metrics`)

Released as `0.2.0`.

#### Added

* Added support for second-level metric aggregation.
* Added support for configurable sub-minute aggregation periods.
* Updated the dashboard/API to support finer-grained time periods.

#### Changed

* Updated the `accreta` dependency to `0.3.0`.

---

## [0.2.0] - 2026-09-15

...

## [0.2.0] - 2026-09-13

### Removed

- Removed the `average` aggregate. The mean can be calculated from `sum / count`.
