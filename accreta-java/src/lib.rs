//! JNI bindings for `accreta`, mirroring accreta-node's "bind directly to the crate" approach
//! rather than accreta-ffi's C ABI — see each Java wrapper class for the parallel to the Rust
//! builder/engine API.
//!
//! Scope matches accreta-node/accreta-ffi for now: built-in aggregates only (`Sum`, `Count`,
//! `Min`, `Max`, `Average`, `TDigest`), f64 measures only. i64/u64 measures are a follow-up —
//! same shape as `schema.rs`'s `nativeMeasureF64`, mirroring accreta-ffi's per-type
//! `register_measure_*` split.

mod aggregate_set;
mod engine;
mod error;
mod handles;
mod retention;
mod schema;
mod tdigest;
