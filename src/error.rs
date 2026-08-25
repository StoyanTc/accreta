use std::cell::RefCell;
use std::ffi::CString;
use std::os::raw::c_char;

/// Status code returned by every fallible `accreta_*` function.
///
/// `0` (`Ok`) always means success. Every other value is negative, so a caller who only checks
/// `status != 0` or `status < 0` both work. On any non-`Ok` status, call
/// [`accreta_last_error_message`] on the *same thread* for a human-readable description.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccretaStatus {
    Ok = 0,
    /// A required pointer argument was null.
    NullPointer = -1,
    /// A `const char*` argument was not valid UTF-8.
    InvalidUtf8 = -2,
    /// Schema construction failed (no dimension / no measure registered, or similar).
    Schema = -3,
    /// `accreta_engine_ingest` rejected the sample (wrong measure count/type, wrong dimension
    /// count).
    Ingest = -4,
    /// The requested item (bucket, measure id, aggregate kind for that measure, ...) does not
    /// exist.
    NotFound = -5,
    /// A Rust panic was caught at the FFI boundary. The operation did not complete; the object it
    /// was called on should be treated as unusable and freed.
    Panic = -6,
    /// The requested [`crate::types::AccretaAggregateKind`] was not registered for that measure,
    /// or the supplied [`crate::types::AccretaMeasureType`] didn't match the measure's actual
    /// type.
    TypeMismatch = -7,
    /// A length/index argument was out of range or inconsistent (e.g. a measures array whose
    /// length doesn't match what a subsequent call expects).
    InvalidArgument = -8,
}

thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

pub(crate) fn set_last_error(message: impl Into<Vec<u8>>) {
    let message = CString::new(message).unwrap_or_else(|_| {
        CString::new("accreta-ffi: error message contained an interior NUL byte").unwrap()
    });
    LAST_ERROR.with(|cell| *cell.borrow_mut() = Some(message));
}

pub(crate) fn clear_last_error() {
    LAST_ERROR.with(|cell| *cell.borrow_mut() = None);
}

/// Returns a status code and, as a side effect, records `message` as this thread's last error.
pub(crate) fn fail(status: AccretaStatus, message: impl Into<Vec<u8>>) -> AccretaStatus {
    set_last_error(message);
    status
}

/// The message associated with the most recent non-[`AccretaStatus::Ok`] status returned by a
/// call to this crate *on the calling thread*.
///
/// The returned pointer is valid until the next `accreta_*` call on this thread, and must not be
/// freed by the caller. Returns null if no error has been recorded on this thread yet (or the
/// last error has since been superseded by a successful call).
#[unsafe(no_mangle)]
pub extern "C" fn accreta_last_error_message() -> *const c_char {
    LAST_ERROR.with(|cell| {
        cell.borrow()
            .as_ref()
            .map(|s| s.as_ptr())
            .unwrap_or(std::ptr::null())
    })
}

/// Clears this thread's last-error message. Not required for correctness — every fallible call
/// either succeeds (leaving any prior message stale but harmless) or overwrites it — but useful
/// for tests or long-lived worker threads that want to assert no error occurred.
#[unsafe(no_mangle)]
pub extern "C" fn accreta_clear_last_error() {
    clear_last_error();
}
