use crate::Value;
use crate::staged::property_buffer::PropertyBufferHead;
use crate::heap::growth::{GrowthPolicy, grown_capacity};
use std::{
    mem::ManuallyDrop,
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

/// Preserve the arena's dense growth policy without making every allocation a
/// page-protection call. Capacity counts retained storage, not reserved addresses.
const DENSE_GROWTH_THRESHOLD: usize = 65_536;
const DENSE_GROWTH_DIVISOR: usize = 3;

#[derive(Clone, Copy)]
enum Allocation {
    Unallocated,
    Heap,
    #[cfg(unix)]
    Reserved {
        bytes: usize,
        page_values: usize,
    },
}

/// One base/length authority for initialized property values. Unix reserves an
/// address range once and exposes pages as needed. Unsupported targets or a failed
/// initial reservation retain ordinary Vec allocation. Access never branches on
/// allocation kind; only growth, compaction and destruction inspect it.
#[repr(C)]
pub(super) struct PropertyBuffer {
    /// The base and the initialized length, the words native code reads first (`staged::property_buffer`).
    head: PropertyBufferHead<NonNull<Value>>,
    capacity: usize,
    allocation: Allocation,
}

/// Native code reads the allocator's authoritative header, never a copied view.
#[derive(Clone, Copy)]
pub struct NativePropertyBufferLayout {
    pub base: usize,
    pub length: usize,
}

pub const NATIVE_PROPERTY_BUFFER_LAYOUT: NativePropertyBufferLayout = NativePropertyBufferLayout {
    base: std::mem::offset_of!(PropertyBuffer, head) + std::mem::offset_of!(PropertyBufferHead<NonNull<Value>>, base),
    length: std::mem::offset_of!(PropertyBuffer, head) + std::mem::offset_of!(PropertyBufferHead<NonNull<Value>>, length),
};

// SAFETY: like Vec<Value>, the buffer owns its allocation, mutation requires
// exclusive access, and shared access exposes only initialized, immutable Values.
unsafe impl Send for PropertyBuffer {}
unsafe impl Sync for PropertyBuffer {}

impl Default for PropertyBuffer {
    fn default() -> Self {
        Self {
            head: PropertyBufferHead {
                base: NonNull::dangling(),
                length: 0,
            },
            capacity: 0,
            allocation: Allocation::Unallocated,
        }
    }
}

impl Deref for PropertyBuffer {
    type Target = [Value];
    #[inline(always)]
    fn deref(&self) -> &[Value] {
        // SAFETY: every slot below length is initialized and accessible, even
        // when a reservation has inaccessible pages beyond capacity.
        unsafe { std::slice::from_raw_parts(self.head.base.as_ptr(), self.head.length) }
    }
}

impl DerefMut for PropertyBuffer {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut [Value] {
        // SAFETY: as above, with exclusive access inherited from the buffer.
        unsafe { std::slice::from_raw_parts_mut(self.head.base.as_ptr(), self.head.length) }
    }
}

impl PropertyBuffer {
    /// Whether the values keep their addresses as the buffer grows: a reservation's do,
    /// a heap allocation's may move.
    #[inline(always)]
    pub(super) fn keeps_addresses(&self) -> bool {
        #[cfg(unix)]
        return matches!(self.allocation, Allocation::Reserved { .. });
        #[cfg(not(unix))]
        false
    }

    #[inline]
    pub(super) fn capacity(&self) -> usize {
        self.capacity
    }

    /// Newly initialized slots contain undefined; failure leaves existing values
    /// and their addresses intact so the arena can use dictionary storage instead.
    #[inline]
    pub(super) fn resize(&mut self, length: usize) -> bool {
        if length > self.capacity && !self.grow(length) {
            return false;
        }
        if length > self.head.length {
            // SAFETY: grow exposed the entire tail, which is uniquely owned and
            // not yet included in the initialized slice.
            let tail = unsafe {
                std::slice::from_raw_parts_mut(
                    self.head.base.as_ptr().add(self.head.length),
                    length - self.head.length,
                )
            };
            tail.fill(Value::UNDEFINED);
        }
        self.head.length = length;
        true
    }

    /// `resize` to a longer `length` whose new values hold whatever bits their memory holds,
    /// where it is a reservation's (zero on first touch, else values a run held before): for
    /// space whose every run is written before it is read (`ValueArena::grow_nursery`), so
    /// pages are made resident by the runs that use them, not by a fill ahead of them. A
    /// heap allocation's tail is filled, as `resize` fills it.
    pub(super) fn extend_unfilled(&mut self, length: usize) -> bool {
        #[cfg(unix)]
        if length > self.head.length && (length <= self.capacity || self.grow(length)) && matches!(self.allocation, Allocation::Reserved { .. }) {
            self.head.length = length;
            return true;
        }
        self.resize(length)
    }

    pub(super) fn truncate(&mut self, length: usize) {
        self.head.length = self.head.length.min(length);
    }

    pub(super) fn clear(&mut self) {
        self.head.length = 0;
    }

    #[cold]
    fn grow(&mut self, required: usize) -> bool {
        if matches!(self.allocation, Allocation::Unallocated) {
            #[cfg(unix)]
            if !self.reserve_address_space(super::MAX_ARENA_START) {
                self.allocation = Allocation::Heap;
            }
            #[cfg(not(unix))]
            {
                self.allocation = Allocation::Heap;
            }
        }
        let wanted = grown_capacity(
            self.capacity,
            required,
            GrowthPolicy::PropertyBuffer {
                threshold: DENSE_GROWTH_THRESHOLD,
                divisor: DENSE_GROWTH_DIVISOR,
                minimum: super::MIN_CAPACITY,
            },
        );
        match self.allocation {
            #[cfg(unix)]
            Allocation::Reserved { bytes, page_values } => {
                let limit = bytes / size_of::<Value>();
                if required > limit {
                    return false;
                }
                let capacity = wanted.min(limit).next_multiple_of(page_values);
                // SAFETY: this page-aligned tail belongs to the reservation and
                // lies after all accessible values. No old page is changed.
                let status = unsafe {
                    libc::mprotect(
                        self.head.base.as_ptr().add(self.capacity).cast(),
                        (capacity - self.capacity) * size_of::<Value>(),
                        libc::PROT_READ | libc::PROT_WRITE,
                    )
                };
                if status != 0 {
                    return false;
                }
                self.capacity = capacity;
                true
            }
            Allocation::Heap => {
                // SAFETY: Heap owns exactly the raw parts previously obtained
                // from Vec. ManuallyDrop keeps ownership here on reserve failure.
                let mut values = ManuallyDrop::new(unsafe {
                    Vec::from_raw_parts(self.head.base.as_ptr(), self.head.length, self.capacity)
                });
                let success = values.try_reserve_exact(wanted - self.head.length).is_ok();
                self.head.base = NonNull::new(values.as_mut_ptr()).expect("Vec has a non-null base");
                self.capacity = values.capacity();
                success
            }
            Allocation::Unallocated => unreachable!("initial allocation resolved above"),
        }
    }

    #[cfg(unix)]
    fn reserve_address_space(&mut self, values: usize) -> bool {
        // A reservation must fit pointer offsets; 32-bit targets use the heap
        // fallback when the full property index space cannot be reserved.
        let Some(bytes) = values.checked_mul(size_of::<Value>()) else {
            return false;
        };
        // SAFETY: sysconf has no memory preconditions.
        let page_bytes = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        let Ok(page_bytes) = usize::try_from(page_bytes) else {
            return false;
        };
        if page_bytes == 0 || !page_bytes.is_multiple_of(size_of::<Value>()) {
            return false;
        }
        let Some(bytes) = bytes
            .checked_next_multiple_of(page_bytes)
            .filter(|bytes| *bytes > 0 && *bytes <= isize::MAX as usize)
        else {
            return false;
        };
        // SAFETY: an anonymous, inaccessible mapping reserves addresses without
        // touching existing mappings or allocating resident property values.
        let base = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                bytes,
                libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
                -1,
                0,
            )
        };
        if base == libc::MAP_FAILED {
            return false;
        }
        self.head.base = NonNull::new(base.cast()).expect("mmap returned a mapping");
        self.allocation = Allocation::Reserved {
            bytes,
            page_values: page_bytes / size_of::<Value>(),
        };
        true
    }

    /// Gives back the whole pages inside `[start, end)` (in values) when the storage is a
    /// reservation: they read back as zero and the next write faults them in again. Pages
    /// that hold any value outside the range stay resident, as does storage a heap allocation
    /// owns. The caller holds no live value in the range.
    #[cfg(unix)]
    pub(super) fn release_pages(&mut self, start: usize, end: usize) {
        let Allocation::Reserved { page_values, .. } = self.allocation else {
            return;
        };
        let first = start.next_multiple_of(page_values);
        let last = end.min(self.capacity) / page_values * page_values;
        if first >= last {
            return;
        }
        #[cfg(target_vendor = "apple")]
        let advice = libc::MADV_FREE;
        #[cfg(not(target_vendor = "apple"))]
        let advice = libc::MADV_DONTNEED;
        // SAFETY: `first..last` is page aligned, lies inside the accessible capacity of this
        // reservation, and holds no live value (the caller's contract). Nothing reads it until
        // a write initializes it, whether or not the OS reclaimed it.
        unsafe {
            libc::madvise(
                self.head.base.as_ptr().add(first).cast(),
                (last - first) * size_of::<Value>(),
                advice,
            );
        }
    }

    #[cfg(not(unix))]
    pub(super) fn release_pages(&mut self, _start: usize, _end: usize) {}

    /// Discard unused suffix storage after compaction while retaining every
    /// reserved address. Discarded pages may remain readable; only length, never
    /// page permissions, determines which Values are initialized.
    pub(super) fn shrink_to(&mut self, minimum: usize) {
        let wanted = self.head.length.max(minimum);
        if wanted >= self.capacity {
            return;
        }
        match self.allocation {
            #[cfg(unix)]
            Allocation::Reserved { page_values, .. } => {
                let capacity = wanted.next_multiple_of(page_values);
                if capacity == self.capacity {
                    return;
                }
                // Never replace/unmap a reservation suffix: failed MAP_FIXED
                // can leave a hole. Advice retains ownership even on failure.
                #[cfg(target_vendor = "apple")]
                let advice = libc::MADV_FREE;
                #[cfg(not(target_vendor = "apple"))]
                let advice = libc::MADV_DONTNEED;
                // SAFETY: the page-aligned suffix belongs to this allocation and
                // contains no initialized Values. Nothing reads it until resize
                // initializes it again, whether or not the OS reclaimed it.
                let address = unsafe { self.head.base.as_ptr().add(capacity) }.cast();
                let status = unsafe {
                    libc::madvise(
                        address,
                        (self.capacity - capacity) * size_of::<Value>(),
                        advice,
                    )
                };
                if status == 0 {
                    self.capacity = capacity;
                }
            }
            Allocation::Heap => {
                // SAFETY: the Heap allocation's raw parts have one owner.
                let mut values = ManuallyDrop::new(unsafe {
                    Vec::from_raw_parts(self.head.base.as_ptr(), self.head.length, self.capacity)
                });
                values.shrink_to(wanted);
                self.head.base = NonNull::new(values.as_mut_ptr()).expect("Vec has a non-null base");
                self.capacity = values.capacity();
            }
            Allocation::Unallocated => {}
        }
    }
}

impl From<Vec<Value>> for PropertyBuffer {
    fn from(values: Vec<Value>) -> Self {
        if values.is_empty() {
            return Self::default();
        }
        #[cfg(unix)]
        {
            let mut buffer = Self::default();
            if buffer.reserve_address_space(super::MAX_ARENA_START) && buffer.grow(values.len()) {
                // SAFETY: the fresh reservation owns accessible room for every
                // source Value, disjoint from the Vec. Initialize once from the
                // known payload rather than filling undefined and overwriting it.
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        values.as_ptr(),
                        buffer.head.base.as_ptr(),
                        values.len(),
                    );
                }
                buffer.head.length = values.len();
                return buffer;
            }
            // Drop an unsuccessful reservation before adopting the snapshot's Vec.
        }
        let mut values = ManuallyDrop::new(values);
        Self {
            head: PropertyBufferHead {
                base: NonNull::new(values.as_mut_ptr()).expect("Vec has a non-null base"),
                length: values.len(),
            },
            capacity: values.capacity(),
            allocation: Allocation::Heap,
        }
    }
}

impl Drop for PropertyBuffer {
    fn drop(&mut self) {
        match self.allocation {
            #[cfg(unix)]
            Allocation::Reserved { bytes, .. } => {
                // SAFETY: the entire reservation is owned here; Value is Copy
                // and has no destructor, including values in discarded ranges.
                unsafe {
                    libc::munmap(self.head.base.as_ptr().cast(), bytes);
                }
            }
            Allocation::Heap => {
                // SAFETY: exactly the Vec allocation adopted above, still owned
                // by this buffer with its current initialized length/capacity.
                drop(unsafe {
                    Vec::from_raw_parts(self.head.base.as_ptr(), self.head.length, self.capacity)
                });
            }
            Allocation::Unallocated => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exercise_native_header(mut buffer: PropertyBuffer) {
        let root = &mut buffer as *mut PropertyBuffer;
        let header = root.cast::<u8>();
        for length in [0, 4, DENSE_GROWTH_THRESHOLD + 1, 2, 0, 8] {
            // All mutation and native-style reads descend from one owner pointer,
            // as VM callouts and compiled code do. No shared header borrow survives.
            unsafe {
                assert!((*root).resize(length));
                (*root).shrink_to(length);
                let base = header.add(NATIVE_PROPERTY_BUFFER_LAYOUT.base).cast::<*const Value>().read();
                let initialized = header.add(NATIVE_PROPERTY_BUFFER_LAYOUT.length).cast::<usize>().read();
                assert_eq!(base, (*root).as_ptr());
                assert_eq!(initialized, length);
                if length != 0 {
                    (&mut *root)[length - 1] = Value::number(length as f64);
                    assert_eq!(base.add(length - 1).read(), Value::number(length as f64));
                }
            }
        }
    }

    #[test]
    fn native_header_tracks_heap_relocation_and_compaction() {
        exercise_native_header(PropertyBuffer { allocation: Allocation::Heap, ..PropertyBuffer::default() });
    }

    #[test]
    #[cfg(unix)]
    fn native_header_tracks_reserved_growth_and_compaction() {
        let mut buffer = PropertyBuffer::default();
        assert!(buffer.reserve_address_space(DENSE_GROWTH_THRESHOLD * 2));
        exercise_native_header(buffer);
    }

    #[test]
    fn heap_fallback_preserves_values_and_rejects_capacity_overflow() {
        let mut buffer = PropertyBuffer {
            allocation: Allocation::Heap,
            ..PropertyBuffer::default()
        };
        assert!(buffer.resize(4));
        buffer[0] = Value::number(-0.0);
        buffer[1] = Value::heap(7);
        let expected = buffer.to_vec();
        assert!(!buffer.resize(usize::MAX));
        assert_eq!(&*buffer, expected.as_slice());
        assert!(buffer.resize(DENSE_GROWTH_THRESHOLD + 1));
        assert_eq!(buffer[0].to_bits(), expected[0].to_bits());
        assert_eq!(buffer[1], expected[1]);
        buffer.truncate(2);
        buffer.shrink_to(2);
        assert_eq!(&*buffer, &expected[..2]);
        buffer.clear();
        assert!(buffer.resize(4));
        assert!(buffer.iter().all(|value| value.is_undefined()));
    }

    #[test]
    #[cfg(unix)]
    fn reserved_base_survives_growth_discard_regrowth_and_reset() {
        let mut buffer = PropertyBuffer::default();
        assert!(buffer.reserve_address_space(DENSE_GROWTH_THRESHOLD * 2));
        assert!(buffer.resize(4));
        assert!(matches!(buffer.allocation, Allocation::Reserved { .. }));
        let base = buffer.as_ptr();
        buffer[0] = Value::number(-0.0);
        buffer[1] = Value::heap(7);
        let expected = buffer.to_vec();
        assert!(buffer.resize(DENSE_GROWTH_THRESHOLD + 1));
        assert_eq!(buffer.as_ptr(), base);
        let grown = buffer.capacity();
        buffer.truncate(2);
        buffer.shrink_to(2);
        assert!(buffer.capacity() < grown);
        assert_eq!(buffer.as_ptr(), base);
        assert_eq!(buffer[0].to_bits(), expected[0].to_bits());
        assert_eq!(buffer[1], expected[1]);
        assert!(buffer.resize(grown));
        assert_eq!(buffer.as_ptr(), base);
        assert!(buffer[2..].iter().all(|value| value.is_undefined()));
        buffer.clear();
        buffer.shrink_to(0);
        assert_eq!(buffer.capacity(), 0);
        assert!(buffer.resize(4));
        assert_eq!(buffer.as_ptr(), base);
        assert!(buffer.iter().all(|value| value.is_undefined()));
    }

    #[test]
    #[cfg(unix)]
    fn reservation_exhaustion_keeps_the_live_prefix_and_its_address() {
        let mut buffer = PropertyBuffer::default();
        assert!(buffer.reserve_address_space(1));
        let Allocation::Reserved { bytes, .. } = buffer.allocation else {
            unreachable!()
        };
        let limit = bytes / size_of::<Value>();
        assert!(buffer.resize(limit));
        buffer[0] = Value::heap(9);
        let base = buffer.as_ptr();
        assert!(!buffer.resize(limit + 1));
        assert_eq!(buffer.as_ptr(), base);
        assert_eq!(buffer.len(), limit);
        assert_eq!(buffer[0], Value::heap(9));
    }

    #[test]
    #[cfg(unix)]
    fn exhausted_reservation_uses_dictionaries_and_reuses_released_ranges() {
        use super::super::ValueArena;
        let mut buffer = PropertyBuffer::default();
        assert!(buffer.reserve_address_space(1));
        let Allocation::Reserved { bytes, .. } = buffer.allocation else {
            unreachable!()
        };
        let length = bytes / size_of::<Value>();
        let mut arena = ValueArena {
            values: buffer,
            ..ValueArena::default()
        };
        arena.register_shape(1, length);
        let values = vec![Value::heap(17); length];
        let first = arena.with_values(1, &values);
        assert!(!first.is_dictionary());
        let second = arena.with_values(1, &values);
        assert!(second.is_dictionary());
        assert_eq!(arena.get(first, length - 1), Some(Value::heap(17)));
        assert_eq!(arena.get(second, length - 1), Some(Value::heap(17)));
        arena.release(first);
        let reused = arena.with_values(1, &values);
        assert!(!reused.is_dictionary());
        assert_eq!(reused.start(), first.start());
    }
}
