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
    /// How many values below `len` are not int32s, holes (`Value::DELETED`) included: the
    /// fold of every write, each adding what it writes and removing what it overwrites, so a
    /// reader proves every value an int32 with one load instead of a scan. At least the
    /// true count once native code stored unknown values (`OTHERS_UNKNOWN`).
    others: u32,
}

/// The `others` count native code stores when it may write values it does not count: above
/// any length, so the backing stays `Tagged`, and saturating arithmetic keeps it so.
pub(crate) const OTHERS_UNKNOWN: u32 = u32::MAX;

/// Whether `value` counts among a backing's others.
#[inline(always)]
fn other(value: Value) -> u32 {
    u32::from(value.as_int().is_none())
}

fn others(values: &[Value]) -> u32 {
    values.iter().map(|value| other(*value)).sum()
}

/// A backing's count of other values once a write added `added` and overwrote `removed`;
/// `OTHERS_UNKNOWN` saturates and stays.
#[inline(always)]
fn recounted(others: u32, added: u32, removed: u32) -> u32 {
    others.saturating_add(added).saturating_sub(removed)
}

/// The values start right after the header, at their own alignment.
const VALUES_OFFSET: usize = size_of::<Header>();
const _: () = assert!(VALUES_OFFSET.is_multiple_of(align_of::<Value>()));
const _: () = assert!(align_of::<Header>() >= align_of::<Value>());
/// The byte offset from a backing's first value to its `u32` length: native code that
/// appends within capacity advances the length there.
pub(crate) const LENGTH_FROM_VALUES: i32 = std::mem::offset_of!(Header, len) as i32 - VALUES_OFFSET as i32;
/// The byte offset from a backing's first value to its `u32` count of other values
/// (`Header::others`), which native code reads at entry, and sets to `OTHERS_UNKNOWN` where
/// it may store uncounted values.
pub(crate) const OTHERS_FROM_VALUES: i32 = std::mem::offset_of!(Header, others) as i32 - VALUES_OFFSET as i32;

/// The header every empty backing shares: never written, never freed.
static EMPTY: Header = Header {
    owners: 0,
    len: 0,
    capacity: 0,
    others: 0,
};

/// The smallest capacity a growing backing takes, as `Vec` grows from empty.
const MIN_GROWN_CAPACITY: usize = 4;

/// A full backing grows by its capacity shifted right by this, so by half (as V8 grows
/// fast elements), not doubled as `Vec`: appending stays amortized constant, and the slack
/// a grown backing holds drops from up to its length to half of it. V8-v7 Crypto, whose
/// bignum digit arrays grow by append: median RSS memory-first `--jit` 2844 -> 2728 KiB,
/// throughput 8316 -> 7292 KiB, with Score within run noise across the suite.
const GROWTH_SHIFT: u32 = 1;

#[repr(transparent)]
pub(crate) struct ArrayElements {
    header: NonNull<Header>,
}

/// Offsets for the native read-only array view. These are derived beside the fields they
/// describe so native code does not maintain a second copy of the storage layout.
pub(super) const NATIVE_HEADER_POINTER_OFFSET: usize =
    std::mem::offset_of!(ArrayElements, header);
pub(super) const NATIVE_HEADER_LENGTH_OFFSET: usize = std::mem::offset_of!(Header, len);
pub(super) const NATIVE_HEADER_OWNERS_OFFSET: usize = std::mem::offset_of!(Header, owners);
pub(super) const NATIVE_HEADER_VALUES_OFFSET: usize = VALUES_OFFSET;

const _: () = assert!(NATIVE_HEADER_POINTER_OFFSET == 0);
const _: () = assert!(NATIVE_HEADER_LENGTH_OFFSET + size_of::<u32>() <= size_of::<Header>());
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
        Self::copied(values, others(values))
    }

    /// A backing of `values`, `others` of which are not int32s.
    fn copied(values: &[Value], others: u32) -> Self {
        let elements = Self::allocate(values.len(), values.len());
        // SAFETY: the new allocation holds `values.len()` slots after the header.
        unsafe {
            std::ptr::copy_nonoverlapping(values.as_ptr(), elements.values(), values.len());
            (*elements.header.as_ptr()).others = others;
        }
        elements
    }

    /// `len` copies of `value`, allocated at their final size with no intermediate buffer.
    pub(crate) fn filled(len: usize, value: Value) -> Self {
        Self::from_fn(len, |_| value)
    }

    /// The `len` values `value` gives for each index in order, written straight into a
    /// backing allocated at its final size, with no intermediate buffer.
    pub(crate) fn from_fn(len: usize, mut value: impl FnMut(usize) -> Value) -> Self {
        if len == 0 {
            return Self::new();
        }
        let mut elements = Self::allocate(len, len);
        let mut counted = 0;
        for slot in 0..len {
            let written = value(slot);
            counted += other(written);
            // SAFETY: the new allocation holds `len` slots after the header.
            unsafe { elements.values().add(slot).write(written) };
        }
        elements.make_mut().set_others(counted);
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
                others: 0,
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

    /// Whether every value is an int32 (so none is a hole).
    pub(crate) fn holds_only_int32(&self) -> bool {
        self.header().others == 0
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

    /// The first value of the backing this array alone owns, which a replacement writes in
    /// place (`ArrayElements::replace_through`); `None` when shared (or the shared empty
    /// header), where a write must copy first (`make_mut`). Taking it changes no backing.
    #[inline(always)]
    pub(crate) fn unique_values(&mut self) -> Option<NonNull<Value>> {
        // SAFETY: a unique backing's values follow its header.
        self.is_unique().then(|| unsafe { NonNull::new_unchecked(self.values()) })
    }

    /// Replaces the value at `index`, below the length, in place where this array alone owns
    /// its backing (`UniqueElements::set`, which keeps the count of other values); `false`
    /// changes nothing where the backing is shared. Changes no backing.
    #[inline(always)]
    pub(crate) fn replace_in_place(&mut self, index: usize, value: Value) -> bool {
        if !self.is_unique() {
            return false;
        }
        // SAFETY: `UniqueElements` is a transparent wrapper; the backing is this array's own.
        unsafe { &mut *(self as *mut Self).cast::<UniqueElements>() }.set(index, value);
        true
    }

    /// `UniqueElements::set` through the first value a unique backing gave
    /// (`unique_values`): the count of other values in its header keeps the write.
    ///
    /// # Safety
    /// `values` is the first value of a backing its array still owns alone, and `index` is
    /// below its length.
    #[inline(always)]
    pub(crate) unsafe fn replace_through(values: NonNull<Value>, index: usize, value: Value) {
        // SAFETY: the caller's contract; the header precedes the values in the allocation.
        unsafe {
            let header = values.as_ptr().cast::<u8>().sub(VALUES_OFFSET).cast::<Header>();
            let slot = values.as_ptr().add(index);
            (*header).others = recounted((*header).others, other(value), other(*slot));
            slot.write(value);
        }
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
        *self = Self::copied(self, self.header().others);
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
            // An owner more makes in-place replacement reach another array.
            super::array_proofs::retire_array_storage_proofs();
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
    fn set_others(&mut self, others: u32) {
        if self.0.capacity() != 0 {
            // SAFETY: a unique backing with a capacity is this array's own allocation.
            unsafe { (*self.0.header.as_ptr()).others = others };
        }
    }

    /// Counts `added` other values in and `removed` out.
    #[inline(always)]
    fn recount(&mut self, added: u32, removed: u32) {
        if added != removed {
            self.set_others(recounted(self.0.header().others, added, removed));
        }
    }

    /// Writes `value` at `index`, below the length.
    #[inline(always)]
    pub(crate) fn set(&mut self, index: usize, value: Value) {
        assert!(index < self.0.len(), "array store index out of bounds");
        self.recount(other(value), other(self.0[index]));
        // SAFETY: the index is below the length of this unique backing.
        unsafe { self.0.values().add(index).write(value) };
    }

    /// `value` at every index of `range`.
    pub(crate) fn fill(&mut self, range: std::ops::Range<usize>, value: Value) {
        assert!(range.start <= range.end && range.end <= self.0.len(), "array fill out of bounds");
        self.recount(other(value) * range.len() as u32, others(&self.0[range.clone()]));
        for index in range {
            // SAFETY: the range is within the length of this unique backing.
            unsafe { self.0.values().add(index).write(value) };
        }
    }

    /// `values` at the indices from `start`.
    pub(crate) fn copy_from(&mut self, start: usize, values: &[Value]) {
        assert!(start + values.len() <= self.0.len(), "array copy out of bounds");
        self.recount(others(values), others(&self.0[start..start + values.len()]));
        // SAFETY: the range is within the length; `values` cannot alias this unique backing.
        unsafe { std::ptr::copy_nonoverlapping(values.as_ptr(), self.0.values().add(start), values.len()) };
    }

    /// The first value, for native code that writes the backing and keeps its count of
    /// other values (`OTHERS_FROM_VALUES`).
    pub(crate) fn as_mut_ptr(&mut self) -> *mut Value {
        self.0.values()
    }

    fn set_len(&mut self, len: usize) {
        debug_assert!(len <= self.0.capacity());
        if self.0.capacity() != 0 {
            // SAFETY: a unique backing with a capacity is this array's own allocation.
            unsafe { (*self.0.header.as_ptr()).len = len as u32 };
        }
    }

    /// Room for `additional` more values, growing by `GROWTH_SHIFT`.
    pub(crate) fn reserve(&mut self, additional: usize) {
        let required = self.0.len().checked_add(additional).expect("array backing overflow");
        let capacity = self.0.capacity();
        if required <= capacity {
            return;
        }
        let grown = required.max(capacity + (capacity >> GROWTH_SHIFT)).max(MIN_GROWN_CAPACITY);
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
        self.recount(other(value), 0);
        // SAFETY: `reserve` made room for one more value.
        unsafe { self.0.values().add(len).write(value) };
        self.set_len(len + 1);
    }

    pub(crate) fn pop(&mut self) -> Option<Value> {
        let len = self.0.len().checked_sub(1)?;
        let value = self[len];
        self.recount(0, other(value));
        self.set_len(len);
        Some(value)
    }

    pub(crate) fn truncate(&mut self, len: usize) {
        if len < self.0.len() {
            self.recount(0, others(&self.0[len..]));
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
        self.recount(other(value) * (len - current) as u32, 0);
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
        self.recount(others(values), 0);
        // SAFETY: `reserve` made room; `values` cannot alias this unique backing's spare room.
        unsafe { std::ptr::copy_nonoverlapping(values.as_ptr(), self.0.values().add(len), values.len()) };
        self.set_len(len + values.len());
    }

    pub(crate) fn remove(&mut self, index: usize) -> Value {
        let len = self.0.len();
        assert!(index < len, "array removal index out of bounds");
        let value = self[index];
        self.recount(0, other(value));
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
        self.recount(others(values), others(&self.0[range.clone()]));
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
        assert_eq!(empty_values, (*empty).as_ptr());
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
        assert_eq!(current_values, (*dense).as_ptr());
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
        assert_eq!(current_values, (*shared).as_ptr());
        assert_eq!(shared.len(), 3);
    }

    #[test]
    fn in_place_replacements_keep_the_others_count_exact() {
        // An array storage proof's store writes through the backing (`replace_through`),
        // as an in-place replacement does: the count stays the one authority.
        let mut elements = ArrayElements::from(vec![Value::integer(1), Value::integer(2)]);
        assert!(elements.replace_in_place(0, Value::number(0.5)));
        assert!(!elements.holds_only_int32());
        let values = elements.unique_values().expect("a unique backing");
        // SAFETY: the backing stays unique and both indices are below its length.
        unsafe { ArrayElements::replace_through(values, 0, Value::integer(3)) };
        assert!(elements.holds_only_int32());
        unsafe { ArrayElements::replace_through(values, 1, Value::NULL) };
        assert!(!elements.holds_only_int32());
        let shared = elements.clone();
        assert!(!elements.replace_in_place(1, Value::integer(4)));
        assert!(elements.unique_values().is_none());
        assert_eq!(shared[1], Value::NULL);
    }

    #[test]
    fn every_write_keeps_the_others_count_exact() {
        let ints = || ArrayElements::from(vec![Value::integer(1), Value::integer(2)]);
        assert!(ints().holds_only_int32());
        assert!(!ArrayElements::from(vec![Value::integer(1), Value::number(0.5)]).holds_only_int32());
        assert!(ArrayElements::filled(3, Value::integer(0)).holds_only_int32());
        assert!(!ArrayElements::from_fn(3, |slot| if slot == 2 { Value::NULL } else { Value::integer(0) }).holds_only_int32());
        let demotions: [(&str, fn(&mut UniqueElements)); 6] = [
            ("set", |elements| elements.set(0, Value::DELETED)),
            ("push", |elements| elements.push(Value::number(0.5))),
            ("resize", |elements| elements.resize(4, Value::UNDEFINED)),
            ("fill", |elements| elements.fill(0..1, Value::TRUE)),
            ("copy_from", |elements| elements.copy_from(1, &[Value::NULL])),
            ("splice", |elements| elements.splice(0..1, &[Value::FALSE])),
        ];
        for (write, demote) in demotions {
            let mut elements = ints();
            demote(elements.make_mut());
            assert!(!elements.holds_only_int32(), "{write}");
        }
        let mut grown = ints();
        for value in 3..64 {
            grown.make_mut().push(Value::integer(value));
        }
        grown.make_mut().set(5, Value::integer(-5));
        assert!(grown.holds_only_int32(), "int32 writes and growth keep the kind");
        let mut shared = ArrayElements::from(vec![Value::integer(1), Value::NULL]);
        let mut copy = shared.clone();
        copy.make_mut().set(1, Value::integer(2));
        assert_eq!((shared.holds_only_int32(), copy.holds_only_int32()), (false, true), "a detached copy counts its own writes");
        shared.make_mut().truncate(1);
        assert!(shared.holds_only_int32(), "removing the last other value restores Int32");
        let mut holey = ints();
        holey.make_mut().resize(4, Value::DELETED);
        assert!(!holey.holds_only_int32());
        for index in [3, 2] {
            holey.make_mut().set(index, Value::integer(0));
        }
        assert!(holey.holds_only_int32(), "filling every hole with int32s restores Int32");
        holey.make_mut().set(0, Value::number(0.5));
        holey.make_mut().pop();
        assert!(!holey.holds_only_int32());
        holey.make_mut().remove(0);
        assert!(holey.holds_only_int32());
        assert!(ArrayElements::new().holds_only_int32());
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
        first.make_mut().set(0, Value::integer(9));
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
