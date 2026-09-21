# Changelog

All notable changes to the Accreta project are documented here.

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
