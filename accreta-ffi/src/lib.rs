//! C ABI for the `accreta` mergeable-state aggregation engine.
//!
//! # Conventions used throughout this crate
//!
//! - Every type exposed to C is an **opaque handle**: a `Box<T>` turned into a raw pointer via
//!   [`Box::into_raw`]. Callers never see the Rust layout; they only ever hold a pointer and pass
//!   it back into the matching `*_free` function exactly once.
//! - Functions that can fail return an [`AccretaStatus`] (`i32`) and write their result through
//!   an `out` pointer parameter, following the common C ABI pattern of "status code + out param"
//!   rather than throwing or returning a sentinel that might collide with a valid value.
//!   [`accreta_last_error_message`] returns human-readable detail for the most recent non-OK
//!   status *on the calling thread*.
//! - No Rust panic is ever allowed to unwind across the FFI boundary (that's undefined behavior).
//!   Every `extern "C" fn" body is wrapped in [`ffi_guard`], which catches panics and converts
//!   them into [`AccretaStatus::Panic`] / a null pointer, as appropriate for the function's return
//!   type.
//! - Time is passed as `i64` **milliseconds since the Unix epoch** (UTC) — see [`types`] for the
//!   conversion helpers.
//! - Iteration over collections that don't have a natural fixed size in C (bucket groups, a
//!   grouped query's result set, all buckets at a level) uses a **cursor/handle** model: a
//!   `*_cursor` constructor returns an opaque cursor, `*_cursor_next` pulls one *owned* item at a
//!   time (returning `false`/`ACCRETA_NOT_FOUND` when exhausted), and `*_cursor_free` releases
//!   it. This was chosen over a callback-based model so that C callers keep normal, linear control
//!   flow and can break out of iteration early without needing to signal that back through a
//!   callback return value.
//!
//! This crate deliberately does **not** expose accreta's generic custom-aggregate machinery
//! (`Monoid` / `Aggregator` / `AggregateFactory`) across the C boundary — only the fixed set of
//! built-in aggregate kinds (`Sum`, `Count`, `Min`, `Max`, `Average`). A C caller selects which of
//! those to attach to a measure when building the schema; nothing outside this process can ever
//! register a new aggregate kind.

mod aggregate;
mod bucket;
mod dimension_key;
mod engine;
mod error;
mod schema;
mod types;

pub use aggregate::*;
pub use bucket::*;
pub use dimension_key::*;
pub use engine::*;
pub use error::*;
pub use schema::*;
pub use types::*;

use std::panic::{AssertUnwindSafe, catch_unwind};

/// Runs `f`, catching any Rust panic so it can never unwind across the FFI boundary.
///
/// On panic, records a message retrievable via [`accreta_last_error_message`] and returns
/// `on_panic` (typically [`AccretaStatus::Panic`] as an `i32`, or a null pointer, depending on
/// the wrapped function's return type).
pub(crate) fn ffi_guard<T>(on_panic: T, f: impl FnOnce() -> T) -> T {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => value,
        Err(payload) => {
            let message = panic_message(&payload);
            error::set_last_error(message);
            on_panic
        }
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        format!("panic in accreta-ffi: {s}")
    } else if let Some(s) = payload.downcast_ref::<String>() {
        format!("panic in accreta-ffi: {s}")
    } else {
        "panic in accreta-ffi: <non-string payload>".to_string()
    }
}
