//! Append buffers: storage that text built by appends grows into in place.
use std::mem::MaybeUninit;
use std::rc::Rc;

use super::stored_length;

/// Storage that text built by appends grows into, as a `Vec` grows: a string holding it is
/// its first `length` units, and an append to the string whose end is the buffer's end
/// writes past it, in place, leaving every unit a string already holds untouched. Each
/// append costs the units it adds, where a copy of the whole prefix (or a rope node and
/// copied leaf) cost one allocation and a copy of the prefix or a leaf each
/// (SpiderMonkey's extensible strings and CPython's in-place `+=` make the same trade).
pub(crate) struct AppendBuffer<T> {
    units: std::ptr::NonNull<MaybeUninit<T>>,
    capacity: usize,
    /// Units written; only units past it are ever written again.
    filled: std::cell::Cell<usize>,
    /// The length of the string that made the buffer, which is charged its whole capacity
    /// (`JsString::flat_unit_bytes`): the strings made by appends into it allocated nothing.
    made_length: usize,
}

/// An append buffer holds this many times the units it is made for, so appends copy each
/// unit a bounded number of times however long the text grows.
const APPEND_BUFFER_GROWTH: usize = 2;

impl<T: Copy> AppendBuffer<T> {
    /// A buffer holding `prefix` then `tail`, with room for as many units again.
    pub(super) fn made_of(prefix: impl ExactSizeIterator<Item = T>, tail: impl ExactSizeIterator<Item = T>) -> Rc<Self> {
        let length = prefix.len() + tail.len();
        let mut storage = Box::<[T]>::new_uninit_slice(length * APPEND_BUFFER_GROWTH);
        for (slot, unit) in storage.iter_mut().zip(prefix.chain(tail)) {
            slot.write(unit);
        }
        let capacity = storage.len();
        let units = std::ptr::NonNull::new(Box::into_raw(storage) as *mut MaybeUninit<T>)
            .expect("a boxed slice is non-null");
        Rc::new(Self { units, capacity, filled: std::cell::Cell::new(length), made_length: length })
    }

    /// The first `length` units, which are written and never written again.
    pub(super) fn prefix(&self, length: u32) -> &[T] {
        debug_assert!(length as usize <= self.filled.get());
        // SAFETY: units below `filled` are initialized, and appends write only past it.
        unsafe { std::slice::from_raw_parts(self.units.as_ptr() as *const T, length as usize) }
    }

    /// Appends `tail` when a string of `length` units ends where the buffer does and the
    /// tail fits: the length of the string that holds both, else `None`.
    pub(super) fn append(&self, length: u32, tail: impl ExactSizeIterator<Item = T>) -> Option<u32> {
        let (filled, added) = (self.filled.get(), tail.len());
        if filled != length as usize || self.capacity - filled < added {
            return None;
        }
        for (offset, unit) in tail.enumerate() {
            // SAFETY: `filled + offset` is below `capacity`, and no reference covers units at
            // or past `filled` (`prefix`).
            unsafe { self.units.as_ptr().add(filled + offset).write(MaybeUninit::new(unit)) };
        }
        self.filled.set(filled + added);
        Some(stored_length(filled + added))
    }

    /// The units written.
    pub(super) fn filled(&self) -> usize {
        self.filled.get()
    }

    /// The address and bytes of the buffer's storage.
    pub(super) fn storage(&self) -> (usize, usize) {
        (self.units.as_ptr() as usize, self.capacity_bytes())
    }

    fn capacity_bytes(&self) -> usize {
        self.capacity * size_of::<T>()
    }

    /// The bytes a string of `length` units holding the buffer is charged: the whole buffer
    /// for the string that made it, nothing for those appends made in place.
    pub(super) fn charge(&self, length: u32) -> usize {
        if length as usize == self.made_length { self.capacity_bytes() } else { 0 }
    }
}

impl<T> Drop for AppendBuffer<T> {
    fn drop(&mut self) {
        // SAFETY: `units` came from `Box::into_raw` of a slice of `capacity` units, and `T`
        // needs no drop.
        drop(unsafe {
            Box::from_raw(std::ptr::slice_from_raw_parts_mut(self.units.as_ptr(), self.capacity))
        });
    }
}

/// A flat left side at most this long takes a short right side by copy, into storage of their
/// exact length; past it, the text moves into an append buffer (`JsString::extended`), so
/// text built by appends copies each unit a bounded number of times. Up to 2048 units every
/// append copied the whole prefix: `r += "ab"` to 20000 units charged 2.7 allocations per
/// append, 65 -> 47 ns per append and 6501 -> 2700 collections for 10^7 appends (experiment
/// build, x86_64 4 vCPU, 2026-11-07). A copy this short is about one cache line.
pub(super) const APPEND_BUFFER_MIN_UNITS: usize = 64;
