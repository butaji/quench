//! An array's dense elements: one copy-on-write allocation that holds a header (owner count,
//! length, capacity) followed by the values. Arrays that copy a constant literal or a slice
//! share one backing until either writes, so an element read is one load from the backing,
//! and an empty array owns no allocation at all.
use crate::value::Value;
use std::alloc::{Layout, alloc, dealloc, handle_alloc_error, realloc};
use std::ptr::NonNull;

#[repr(C, align(8))]
struct Header {
    /// Owners of this backing; zero only in the shared empty header, which no owner counts.
    owners: u32,
    len: u32,
    capacity: u32,
}

/// The values start right after the header, at their own alignment.
const VALUES_OFFSET: usize = size_of::<Header>();
const _: () = assert!(VALUES_OFFSET % align_of::<Value>() == 0);
const _: () = assert!(align_of::<Header>() >= align_of::<Value>());

/// The header every empty backing shares: never written, never freed.
static EMPTY: Header = Header {
    owners: 0,
    len: 0,
    capacity: 0,
};

/// The smallest capacity a growing backing takes, as `Vec` grows from empty.
const MIN_GROWN_CAPACITY: usize = 4;

#[repr(transparent)]
pub(crate) struct ArrayElements {
    header: NonNull<Header>,
}

/// Offsets for the native read-only array view. These are derived beside the fields they
/// describe so native code does not maintain a second copy of the storage layout.
#[cfg(test)]
pub(super) const NATIVE_HEADER_POINTER_OFFSET: usize =
    std::mem::offset_of!(ArrayElements, header);
#[cfg(test)]
pub(super) const NATIVE_HEADER_LENGTH_OFFSET: usize = std::mem::offset_of!(Header, len);
#[cfg(test)]
pub(super) const NATIVE_HEADER_VALUES_OFFSET: usize = VALUES_OFFSET;

#[cfg(test)]
const _: () = assert!(NATIVE_HEADER_POINTER_OFFSET == 0);
#[cfg(test)]
const _: () = assert!(NATIVE_HEADER_LENGTH_OFFSET + size_of::<u32>() <= size_of::<Header>());
#[cfg(test)]
const _: () = assert!(NATIVE_HEADER_VALUES_OFFSET >= size_of::<Header>());

/// An `ArrayElements` that no other array shares (`ArrayElements::make_mut`): the only view
/// through which values change.
#[repr(transparent)]
pub(crate) struct UniqueElements(ArrayElements);

impl ArrayElements {
    /// No values, in the shared empty header.
    pub(crate) fn new() -> Self {
        Self {
            header: NonNull::from(&EMPTY),
        }
    }

    pub(crate) fn from_slice(values: &[Value]) -> Self {
        if values.is_empty() {
            return Self::new();
        }
        let elements = Self::allocate(values.len(), values.len());
        // SAFETY: the new allocation holds `values.len()` slots after the header.
        unsafe {
            std::ptr::copy_nonoverlapping(values.as_ptr(), elements.values(), values.len());
        }
        elements
    }

    /// `len` copies of `value`, allocated at their final size with no intermediate buffer.
    pub(crate) fn filled(len: usize, value: Value) -> Self {
        if len == 0 {
            return Self::new();
        }
        let elements = Self::allocate(len, len);
        // SAFETY: the new allocation holds `len` slots after the header.
        unsafe {
            for slot in 0..len {
                elements.values().add(slot).write(value);
            }
        }
        elements
    }

    fn layout(capacity: usize) -> Layout {
        Layout::from_size_align(
            VALUES_OFFSET + capacity.checked_mul(size_of::<Value>()).expect("array backing overflow"),
            align_of::<Header>(),
        )
        .expect("array backing overflow")
    }

    /// A backing of one owner with room for `capacity` values, the first `len` uninitialized.
    fn allocate(capacity: usize, len: usize) -> Self {
        debug_assert!(capacity > 0 && len <= capacity);
        let capacity_word = u32::try_from(capacity).expect("array backing capacity overflow");
        let layout = Self::layout(capacity);
        // SAFETY: the layout has a non-zero size (the header).
        let pointer = unsafe { alloc(layout) }.cast::<Header>();
        let Some(header) = NonNull::new(pointer) else {
            handle_alloc_error(layout);
        };
        // SAFETY: the allocation is fresh and sized for a header.
        unsafe {
            header.as_ptr().write(Header {
                owners: 1,
                len: len as u32,
                capacity: capacity_word,
            });
        }
        Self { header }
    }

    #[inline(always)]
    fn header(&self) -> &Header {
        // SAFETY: the header is live while any owner holds the backing.
        unsafe { self.header.as_ref() }
    }

    #[inline(always)]
    fn values(&self) -> *mut Value {
        // SAFETY: the values start right after the header in the same allocation; for the
        // shared empty header the pointer is aligned and never dereferenced.
        unsafe { self.header.as_ptr().cast::<u8>().add(VALUES_OFFSET).cast::<Value>() }
    }

    #[inline(always)]
    pub(crate) fn len(&self) -> usize {
        self.header().len as usize
    }

    #[inline(always)]
    pub(crate) fn capacity(&self) -> usize {
        self.header().capacity as usize
    }

    /// Whether this array alone owns its backing, so writes reach no other array.
    #[inline(always)]
    fn is_unique(&self) -> bool {
        self.header().owners == 1
    }

    pub(crate) fn as_slice(&self) -> &[Value] {
        self
    }

    /// The backing's identity, which arrays that share it have in common.
    pub(crate) fn as_ptr(&self) -> *const () {
        self.header.as_ptr().cast()
    }

    /// The owners of this backing; zero for the shared empty one.
    pub(crate) fn owner_count(&self) -> usize {
        self.header().owners as usize
    }

    /// This array's values for writing: its own backing, copied first when shared (or when it
    /// is the shared empty header).
    #[inline(always)]
    pub(crate) fn make_mut(&mut self) -> &mut UniqueElements {
        if !self.is_unique() && self.capacity() != 0 {
            self.detach();
        }
        // SAFETY: `UniqueElements` is a transparent wrapper; the backing is now this array's
        // own, or the empty header, which only growth replaces.
        unsafe { &mut *(self as *mut Self).cast::<UniqueElements>() }
    }

    #[cold]
    #[inline(never)]
    fn detach(&mut self) {
        *self = Self::from_slice(self);
    }
}

impl Default for ArrayElements {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> From<[Value; N]> for ArrayElements {
    fn from(values: [Value; N]) -> Self {
        Self::from_slice(&values)
    }
}

impl From<Vec<Value>> for ArrayElements {
    fn from(values: Vec<Value>) -> Self {
        Self::from_slice(&values)
    }
}

impl Clone for ArrayElements {
    fn clone(&self) -> Self {
        if self.capacity() != 0 {
            let owners = self.header().owners.checked_add(1).expect("array backing owners overflow");
            // SAFETY: arrays are single-threaded; the header is live and only owners write it.
            unsafe { (*self.header.as_ptr()).owners = owners };
        }
        Self {
            header: self.header,
        }
    }
}

impl Drop for ArrayElements {
    fn drop(&mut self) {
        let capacity = self.capacity();
        if capacity == 0 {
            return;
        }
        let owners = self.header().owners - 1;
        if owners == 0 {
            // SAFETY: the last owner frees the allocation `allocate` made for this capacity;
            // values need no drop.
            unsafe { dealloc(self.header.as_ptr().cast(), Self::layout(capacity)) };
        } else {
            // SAFETY: as in `clone`.
            unsafe { (*self.header.as_ptr()).owners = owners };
        }
    }
}

impl std::ops::Deref for ArrayElements {
    type Target = [Value];

    #[inline(always)]
    fn deref(&self) -> &[Value] {
        // SAFETY: the first `len` values are initialized.
        unsafe { std::slice::from_raw_parts(self.values(), self.len()) }
    }
}

impl std::fmt::Debug for ArrayElements {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_list().entries(self.iter()).finish()
    }
}

impl PartialEq for ArrayElements {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl UniqueElements {
    fn set_len(&mut self, len: usize) {
        debug_assert!(len <= self.0.capacity());
        if self.0.capacity() != 0 {
            // SAFETY: a unique backing with a capacity is this array's own allocation.
            unsafe { (*self.0.header.as_ptr()).len = len as u32 };
        }
    }

    /// Room for `additional` more values, growing as `Vec` does.
    pub(crate) fn reserve(&mut self, additional: usize) {
        let required = self.0.len().checked_add(additional).expect("array backing overflow");
        let capacity = self.0.capacity();
        if required <= capacity {
            return;
        }
        let grown = required.max(capacity * 2).max(MIN_GROWN_CAPACITY);
        self.grow_to(grown);
    }

    fn grow_to(&mut self, capacity: usize) {
        let capacity_word = u32::try_from(capacity).expect("array backing capacity overflow");
        let old = self.0.capacity();
        if old == 0 {
            // The shared empty header: a fresh backing of its own.
            self.0 = ArrayElements::allocate(capacity, 0);
            return;
        }
        let layout = ArrayElements::layout(capacity);
        // SAFETY: this unique backing was allocated with the layout of its old capacity.
        let pointer = unsafe {
            realloc(self.0.header.as_ptr().cast(), ArrayElements::layout(old), layout.size())
        }
        .cast::<Header>();
        let Some(header) = NonNull::new(pointer) else {
            handle_alloc_error(layout);
        };
        self.0.header = header;
        // SAFETY: the reallocated header is this array's own.
        unsafe { (*header.as_ptr()).capacity = capacity_word };
    }

    pub(crate) fn push(&mut self, value: Value) {
        let len = self.0.len();
        self.reserve(1);
        // SAFETY: `reserve` made room for one more value.
        unsafe { self.0.values().add(len).write(value) };
        self.set_len(len + 1);
    }

    pub(crate) fn pop(&mut self) -> Option<Value> {
        let len = self.0.len().checked_sub(1)?;
        let value = self[len];
        self.set_len(len);
        Some(value)
    }

    pub(crate) fn truncate(&mut self, len: usize) {
        if len < self.0.len() {
            self.set_len(len);
        }
    }

    pub(crate) fn resize(&mut self, len: usize, value: Value) {
        let current = self.0.len();
        if len <= current {
            self.truncate(len);
            return;
        }
        self.reserve(len - current);
        // SAFETY: `reserve` made room for the values up to `len`.
        unsafe {
            for slot in current..len {
                self.0.values().add(slot).write(value);
            }
        }
        self.set_len(len);
    }

    pub(crate) fn extend_from_slice(&mut self, values: &[Value]) {
        let len = self.0.len();
        self.reserve(values.len());
        // SAFETY: `reserve` made room; `values` cannot alias this unique backing's spare room.
        unsafe { std::ptr::copy_nonoverlapping(values.as_ptr(), self.0.values().add(len), values.len()) };
        self.set_len(len + values.len());
    }

    pub(crate) fn remove(&mut self, index: usize) -> Value {
        let len = self.0.len();
        assert!(index < len, "array removal index out of bounds");
        let value = self[index];
        // SAFETY: the tail shifts down within the initialized values.
        unsafe {
            let at = self.0.values().add(index);
            std::ptr::copy(at.add(1), at, len - index - 1);
        }
        self.set_len(len - 1);
        value
    }

    /// Replaces the values in `range` with `values`, as `Vec::splice` does.
    pub(crate) fn splice(&mut self, range: std::ops::Range<usize>, values: &[Value]) {
        let len = self.0.len();
        assert!(range.start <= range.end && range.end <= len, "array splice out of bounds");
        let removed = range.end - range.start;
        if values.len() > removed {
            self.reserve(values.len() - removed);
        }
        let new_len = len - removed + values.len();
        // SAFETY: the tail moves within the reserved allocation, then `values` fills the gap.
        unsafe {
            let base = self.0.values();
            std::ptr::copy(base.add(range.end), base.add(range.start + values.len()), len - range.end);
            std::ptr::copy_nonoverlapping(values.as_ptr(), base.add(range.start), values.len());
        }
        self.set_len(new_len);
    }

    pub(crate) fn capacity(&self) -> usize {
        self.0.capacity()
    }
}

impl std::ops::Deref for UniqueElements {
    type Target = [Value];

    #[inline(always)]
    fn deref(&self) -> &[Value] {
        &self.0
    }
}

impl std::ops::DerefMut for UniqueElements {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut [Value] {
        // SAFETY: the backing is this array's own (or empty), and the first `len` values are
        // initialized.
        unsafe { std::slice::from_raw_parts_mut(self.0.values(), self.0.len()) }
    }
}

impl Extend<Value> for UniqueElements {
    fn extend<I: IntoIterator<Item = Value>>(&mut self, values: I) {
        let values = values.into_iter();
        self.reserve(values.size_hint().0);
        for value in values {
            self.push(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_layout_matches_header_and_dense_backing() {
        assert_eq!(
            NATIVE_HEADER_POINTER_OFFSET,
            std::mem::offset_of!(ArrayElements, header)
        );
        assert_eq!(
            NATIVE_HEADER_LENGTH_OFFSET,
            std::mem::offset_of!(Header, len)
        );
        assert_eq!(NATIVE_HEADER_VALUES_OFFSET, std::mem::size_of::<Header>());
        assert_eq!(
            NATIVE_HEADER_VALUES_OFFSET % std::mem::align_of::<Value>(),
            0
        );
        assert!(
            NATIVE_HEADER_LENGTH_OFFSET + std::mem::size_of::<u32>()
                <= NATIVE_HEADER_VALUES_OFFSET
        );

        let empty = ArrayElements::new();
        let empty_values = unsafe {
            empty
                .header
                .as_ptr()
                .cast::<u8>()
                .add(NATIVE_HEADER_VALUES_OFFSET)
                .cast::<Value>()
                .cast_const()
        };
        assert_eq!(empty_values, (&*empty).as_ptr());
        assert_eq!(empty.len(), 0);

        let values = [Value::integer(7), Value::integer(9)];
        let dense = ArrayElements::from(values);
        let current_values = unsafe {
            dense
                .header
                .as_ptr()
                .cast::<u8>()
                .add(NATIVE_HEADER_VALUES_OFFSET)
                .cast::<Value>()
                .cast_const()
        };
        assert_eq!(current_values, (&*dense).as_ptr());
        // The header pointer belongs to this ArrayElements value; re-reading it observes the
        // new header/backing after copy-on-write mutation.
        let mut shared = dense.clone();
        shared.make_mut().push(Value::integer(11));
        let current_values = unsafe {
            shared
                .header
                .as_ptr()
                .cast::<u8>()
                .add(NATIVE_HEADER_VALUES_OFFSET)
                .cast::<Value>()
                .cast_const()
        };
        assert_eq!(current_values, (&*shared).as_ptr());
        assert_eq!(shared.len(), 3);
    }

    #[test]
    fn shared_backings_copy_on_write_and_empty_arrays_allocate_nothing() {
        let empty = ArrayElements::new();
        assert_eq!(empty.capacity(), 0);
        let mut first = ArrayElements::from(vec![Value::integer(1), Value::integer(2)]);
        let mut second = first.clone();
        assert_eq!(first.as_ptr(), second.as_ptr());
        assert_eq!(first.owner_count(), 2);
        second.make_mut().push(Value::integer(3));
        assert_ne!(first.as_ptr(), second.as_ptr());
        assert_eq!(&*first, &[Value::integer(1), Value::integer(2)]);
        assert_eq!(&*second, &[Value::integer(1), Value::integer(2), Value::integer(3)]);
        first.make_mut()[0] = Value::integer(9);
        assert_eq!(first[0], Value::integer(9));

        let mut grown = ArrayElements::new();
        let unique = grown.make_mut();
        unique.extend((0..10).map(Value::integer));
        assert_eq!(unique.remove(0), Value::integer(0));
        unique.splice(1..3, &[Value::TRUE]);
        unique.truncate(4);
        assert_eq!(&*grown, &[Value::integer(1), Value::TRUE, Value::integer(4), Value::integer(5)]);
        let unique = grown.make_mut();
        assert_eq!(unique.pop(), Some(Value::integer(5)));
        unique.resize(6, Value::NULL);
        assert_eq!(&grown[3..], &[Value::NULL; 3]);
        drop(empty);
    }
}
