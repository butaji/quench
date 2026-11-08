//! Native-fault recovery composed with a host's POSIX signal delivery.

#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
use super::guard_aarch64;
use super::guard_support;
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
use super::resume_guard_fault;

/// Compose a host's POSIX signal delivery with owned native-fault recovery.
/// Prepare before installing the host observer, then let recovery consume its
/// owned faults before enqueuing ordinary host signal notifications.
pub struct NativeFaultHandler;

/// Whether a signal can be a fault on a guard page that native code owns.
fn is_guard_fault(signal: i32) -> bool {
    matches!(signal, libc::SIGSEGV | libc::SIGBUS)
}

impl NativeFaultHandler {
    pub fn prepare(signal: i32) {
        if is_guard_fault(signal) { let _ = guard_support(); }
    }

    /// # Safety
    /// Arguments must be the original POSIX SA_SIGINFO handler arguments on
    /// the interrupted thread. No locks, allocation or guest callbacks occur.
    pub unsafe fn resume(signal: i32, info: *mut libc::siginfo_t, context: *mut libc::c_void) -> bool {
        #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
        if signal == libc::SIGSEGV { return unsafe { resume_guard_fault(info, context) }; }
        #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
        if is_guard_fault(signal) { return unsafe { guard_aarch64::resume_fault(info, context) }; }
        let _ = (signal, info, context);
        false
    }
}
