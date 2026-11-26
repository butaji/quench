//! Address space handed out at the same addresses in every run of one binary: each anonymous
//! reservation the runtime makes takes the next range of `FIXED_SPACE_BASE` in request order
//! (`map_anonymous`). A run that reserves in the same order (an image's start, whose path the
//! image fixes) finds every reservation where an earlier run of the same image left it, so a
//! state saved from that run's memory can be mapped back at its own addresses. A range the
//! kernel refuses (taken, or a host that limits address space, such as Valgrind) falls back to
//! an address of the kernel's choice, which serves the run but breaks that guarantee
//! (`fixed_layout_held`).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// The first fixed address: far above a no-PIE executable, its heap and the default
/// `mmap` area's usual start, inside the 47-bit user address space.
const FIXED_SPACE_BASE: usize = 0x1000_0000_0000;
/// The end of the fixed space: the default `mmap` area grows down from below the stack, far
/// above this.
const FIXED_SPACE_END: usize = 0x5000_0000_0000;
/// Ranges start on this boundary, so a reservation's alignment never depends on its predecessor's
/// length.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const FIXED_ALIGN: usize = 2 << 20;

#[cfg(target_os = "linux")]
const MAP_FIXED_NOREPLACE: libc::c_int = 0x100000;

/// The next range's start.
static NEXT: AtomicUsize = AtomicUsize::new(FIXED_SPACE_BASE);
/// Cleared once a reservation fell back to the kernel's choice.
static HELD: AtomicBool = AtomicBool::new(true);

/// Whether every reservation so far took its fixed range.
pub fn fixed_layout_held() -> bool {
    HELD.load(Ordering::Relaxed)
}

/// The range every fixed reservation lies in.
pub fn fixed_space_range() -> std::ops::Range<usize> {
    FIXED_SPACE_BASE..FIXED_SPACE_END
}

/// Where the next reservation goes: a dump (`dump`) records it, so the run that adopts the dump
/// reserves past what the dump holds (`resume_fixed_space`).
pub fn fixed_space_next() -> usize {
    NEXT.load(Ordering::Relaxed)
}

/// Continues reservations from `next`, a dump's `fixed_space_next`, once its ranges are mapped.
pub fn resume_fixed_space(next: usize) {
    NEXT.store(next, Ordering::Relaxed);
}

/// An anonymous private mapping of `length` bytes with `protection` and the extra `flags`
/// (`MAP_NORESERVE`), at the next fixed range where the kernel grants it; `MAP_FAILED` as
/// `mmap` answers when no address can be had.
///
/// # Safety
/// As `libc::mmap` with a null hint: the mapping aliases nothing the process holds.
pub unsafe fn map_anonymous(length: usize, protection: libc::c_int, flags: libc::c_int) -> *mut libc::c_void {
    let flags = libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | flags;
    #[cfg(target_os = "linux")]
    {
        let span = length.div_ceil(FIXED_ALIGN) * FIXED_ALIGN;
        let start = NEXT.fetch_add(span, Ordering::Relaxed);
        if start.checked_add(span).is_some_and(|end| end <= FIXED_SPACE_END) {
            // SAFETY: `MAP_FIXED_NOREPLACE` maps only where nothing is mapped.
            let mapped = unsafe { libc::mmap(start as *mut libc::c_void, length, protection, flags | MAP_FIXED_NOREPLACE, -1, 0) };
            if mapped as usize == start {
                return mapped;
            }
            if mapped != libc::MAP_FAILED {
                // A kernel without `MAP_FIXED_NOREPLACE` takes it as a hint and maps elsewhere.
                // SAFETY: the mapping just made, which nothing names yet.
                unsafe { libc::munmap(mapped, length) };
            }
        }
        HELD.store(false, Ordering::Relaxed);
    }
    // SAFETY: the caller's contract.
    unsafe { libc::mmap(std::ptr::null_mut(), length, protection, flags, -1, 0) }
}
