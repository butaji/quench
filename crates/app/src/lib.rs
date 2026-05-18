//! # App Library
//!
//! Hot-reloadable application logic.
//! Exports a single `update(state)` function that the host calls.

mod native;

use protocol::AppState;

/// Global state pointer — set by `update` before calling generated code.
///
/// # Safety
/// This is only accessed from the host's single thread during `update`.
static mut STATE_PTR: *mut AppState = std::ptr::null_mut();

/// Access the host-owned application state.
///
/// # Panics
/// Panics if called outside of an `update` invocation.
pub fn state() -> &'static mut AppState {
    unsafe {
        assert!(!STATE_PTR.is_null(), "state() called outside of update()");
        &mut *STATE_PTR
    }
}

/// The single export from the dylib.
///
/// # Safety
/// `state_ptr` must point to a valid `AppState` allocated by the host.
#[no_mangle]
pub unsafe extern "C" fn update(state_ptr: *mut AppState) {
    STATE_PTR = state_ptr;
    generated::main::update();
    STATE_PTR = std::ptr::null_mut();
}
