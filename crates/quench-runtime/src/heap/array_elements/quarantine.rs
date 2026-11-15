//! Backings freed while a concurrent cycle reads them (the quarantine), and the heap values a
//! concurrent marker reads from a backing.

use super::*;

thread_local! {
    /// Concurrent cycles on this thread that hold freed backings (`begin_quarantine`).
    static QUARANTINES: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// The backings freed while a quarantine holds, with their layouts.
    static QUARANTINED: std::cell::RefCell<Vec<(NonNull<u8>, Layout)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Holds every backing this thread frees, until `end_quarantine`: a concurrent marker may
/// still read a backing whose array dropped or grew it since the marker loaded its pointer.
pub(crate) fn begin_quarantine() {
    QUARANTINES.with(|count| count.set(count.get() + 1));
}

/// Ends a `begin_quarantine`; the last one frees what they held. The caller's marker no
/// longer reads any backing.
pub(crate) fn end_quarantine() {
    let left = QUARANTINES.with(|count| {
        let left = count.get().checked_sub(1).expect("no array backing quarantine to end");
        count.set(left);
        left
    });
    if left == 0 {
        for (start, layout) in QUARANTINED.with(|held| std::mem::take(&mut *held.borrow_mut())) {
            // SAFETY: each held allocation was made with this layout and no owner remains.
            unsafe { dealloc(start.as_ptr(), layout) };
        }
    }
}

/// Bytes the quarantine holds.
pub(crate) fn quarantined_bytes() -> usize {
    QUARANTINED.with(|held| held.borrow().iter().map(|(_, layout)| layout.size()).sum())
}

pub(super) fn quarantining() -> bool {
    QUARANTINES.with(|count| count.get() != 0)
}

/// Frees a backing allocation no owner holds, or quarantines it (`begin_quarantine`).
///
/// # Safety
/// `start` was allocated with `layout` and nothing uses it after this call.
pub(super) unsafe fn release(start: *mut u8, layout: Layout) {
    if quarantining() {
        // SAFETY: allocations are never null.
        let start = unsafe { NonNull::new_unchecked(start) };
        QUARANTINED.with(|held| held.borrow_mut().push((start, layout)));
    } else {
        // SAFETY: the caller's contract.
        unsafe { dealloc(start, layout) };
    }
}

/// Pushes the heap values of the backing whose header pointer is `raw`
/// (`ArrayElements::into_raw`) for a concurrent marker, reading the header and values with
/// relaxed atomic loads while the mutator may write them.
///
/// # Safety
/// `raw` was a backing's header pointer at some point during the quarantine
/// (`begin_quarantine`) or before it, and its array record is not freed.
pub(crate) unsafe fn concurrent_heap_values(raw: u64, mut visit: impl FnMut(Value)) {
    use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};
    let header = raw as usize as *mut Header;
    // SAFETY: a header pointer stays readable through the quarantine, and no header moves
    // back over values while it holds (`UniqueElements::prepend`, `unbias`), so the length
    // read here bounds values in the same allocation.
    unsafe {
        if AtomicU32::from_ptr(std::ptr::addr_of_mut!((*header).others)).load(Relaxed) == 0 {
            return;
        }
        let len = AtomicU32::from_ptr(std::ptr::addr_of_mut!((*header).len)).load(Relaxed);
        if len & DOUBLE_LEN != 0 {
            // Numbers and holes only.
            return;
        }
        if len & NARROW_LEN != 0 {
            visit_narrow_heap_values(header, (len & !NARROW_LEN) as usize, visit);
            return;
        }
        let len = len as usize;
        let values = header.cast::<u8>().add(VALUES_OFFSET).cast::<u64>();
        for index in 0..len {
            let value = Value::from_bits(AtomicU64::from_ptr(values.add(index)).load(Relaxed));
            if value.is_heap() {
                visit(value);
            }
        }
    }
}

/// Visits the heap values of a narrow backing of `len` values: its values change only by
/// whole-word replacement (`replace_narrow_in_place`), each logged by the store barrier as a
/// wide one is.
///
/// # Safety
/// `header` is a readable narrow backing's header, with `len` values after it.
unsafe fn visit_narrow_heap_values(header: *const Header, len: usize, mut visit: impl FnMut(Value)) {
    // SAFETY: the caller's contract; the values follow the header.
    let values = unsafe { header.cast::<u8>().add(VALUES_OFFSET).cast::<u32>() };
    for index in 0..len {
        // SAFETY: `index` is below `len`.
        let narrow = unsafe { values.add(index).read() };
        if Value::narrow_is_heap(narrow) {
            visit(Value::from_narrow(narrow));
        }
    }
}
