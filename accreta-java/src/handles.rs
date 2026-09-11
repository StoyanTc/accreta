//! Opaque-handle plumbing shared by every wrapped type.
//!
//! Every Rust value we hand to Java crosses the boundary as a `jlong` holding a raw pointer
//! (`Box::into_raw`). This mirrors the cursor/handle model accreta-ffi already uses for C, just
//! with JNI's `jlong` playing the role of the C ABI's opaque pointer.
//!
//! # Safety contract
//!
//! - A handle is valid from the point it's returned to Java until the matching `nativeDrop` /
//!   `nativeDone` / `nativeBuild` call consumes it. Java-side wrapper classes are responsible for
//!   enforcing "call at most once" (via a `handle = 0` sentinel after use) — these helpers trust
//!   that discipline and do not re-check it themselves.
//! - `0` is reserved as "already consumed / invalid" and is never a value handed out by
//!   `into_handle`. Java wrappers must treat `0` as a no-op for drop calls (already closed).

/// Box `value` and hand back an opaque handle for it.
pub fn into_handle<T>(value: T) -> i64 {
    Box::into_raw(Box::new(value)) as i64
}

/// Reclaim and drop a previously-issued handle.
///
/// # Safety
/// `handle` must be a live value previously returned by [`into_handle::<T>`], not yet consumed by
/// any other `from_handle`/`borrow`/`borrow_mut` call on this same handle.
pub unsafe fn drop_handle<T>(handle: i64) {
    if handle == 0 {
        return;
    }
    drop(unsafe { Box::from_raw(handle as *mut T) });
}

/// Reclaim a handle by value (consuming it), for APIs like `SchemaBuilder::build` /
/// `MeasureBuilder::done` that take `self` rather than `&self`/`&mut self`.
///
/// # Safety
/// Same contract as [`drop_handle`].
pub unsafe fn take_handle<T>(handle: i64) -> T {
    *unsafe { Box::from_raw(handle as *mut T) }
}

/// Borrow a handle immutably without taking ownership.
///
/// # Safety
/// `handle` must be a live value previously returned by [`into_handle::<T>`].
pub unsafe fn borrow<'a, T>(handle: i64) -> &'a T {
    unsafe { &*(handle as *const T) }
}

/// Borrow a handle mutably without taking ownership.
///
/// # Safety
/// `handle` must be a live value previously returned by [`into_handle::<T>`], and no other live
/// borrow of it may exist for the duration of `'a` (upheld by the single-threaded,
/// one-handle-in-flight discipline the Java wrappers enforce).
pub unsafe fn borrow_mut<'a, T>(handle: i64) -> &'a mut T {
    unsafe { &mut *(handle as *mut T) }
}
