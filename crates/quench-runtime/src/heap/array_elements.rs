//! An array's dense elements: one copy-on-write backing that holds a header (owner count,
//! length, capacity) followed by the values. Arrays that copy a constant literal or a slice
//! share one backing until either writes, so an element read is one load from the backing,
//! and an empty array owns no allocation at all.
//!
//! A backing is a heap allocation its owners count, or a young run (`ArrayElements::young`)
//! that no owner counts: a new small array's elements, bumped from the property arena's
//! nursery (`ValueArena::young_elements`) beside the young objects' runs, where a dead one
//! costs nothing to reclaim. Like a shared backing it admits reads only: the first write
//! copies it to an allocation of its own (`make_mut`), as does the collection its array
//! survives (`ArrayElements::leave_nursery`), so every young run's owner is a young array
//! record its sweep shows.
use crate::value::Value;
use std::alloc::{Layout, alloc, dealloc, handle_alloc_error, realloc};
use std::ptr::NonNull;

#[repr(C, align(8))]
struct Header {
    /// Owners of this backing; zero where no owner counts it: the shared empty header and
    /// young runs, which no drop frees.
    owners: u32,
    len: u32,
    /// Room for values after the header, with `BIASED` set when the header was moved past
    /// values a front removal dropped (`UniqueElements::remove` of index 0): the allocation
    /// then starts `bias` values earlier, a count kept in the word just before the header.
    capacity: u32,
    /// How many values below `len` are not int32s, holes (`Value::DELETED`) included: the
    /// fold of every write, each adding what it writes and removing what it overwrites, so a
    /// reader proves every value an int32 with one load instead of a scan. At least the
    /// true count once native code stored unknown values (`OTHERS_UNKNOWN`).
    others: u32,
}

/// The conservative `others` marker native code stores for uncounted writes. Incremental
/// updates preserve it until the backing's values are counted again.
pub(crate) const OTHERS_UNKNOWN: u32 = u32::MAX;


use crate::staged::elements::{concrete_other as other, concrete_recount as recounted};


fn others(values: &[Value]) -> u32 {
    values.iter().map(|value| other(*value)).sum()
}


/// The values start right after the header, at their own alignment.
const VALUES_OFFSET: usize = size_of::<Header>();
/// The arena values a young run's header takes ahead of its values.
pub(crate) const HEADER_VALUES: usize = VALUES_OFFSET / size_of::<Value>();
const _: () = assert!(HEADER_VALUES * size_of::<Value>() == VALUES_OFFSET);
const _: () = assert!(align_of::<Header>() <= align_of::<Value>());
const _: () = assert!(VALUES_OFFSET.is_multiple_of(align_of::<Value>()));
const _: () = assert!(align_of::<Header>() >= align_of::<Value>());
/// The byte offset from a backing's first value to its `u32` length: native code that
/// appends within capacity advances the length there.
pub(crate) const LENGTH_FROM_VALUES: i32 = std::mem::offset_of!(Header, len) as i32 - VALUES_OFFSET as i32;
/// The byte offset from a backing's first value to its `u32` count of other values
/// (`Header::others`), which native code reads at entry, and sets to `OTHERS_UNKNOWN` where
/// it may store uncounted values.
pub(crate) const OTHERS_FROM_VALUES: i32 = std::mem::offset_of!(Header, others) as i32 - VALUES_OFFSET as i32;
/// The byte offset from a backing's first value to its capacity word, whose
/// `DOUBLE_ELEMENTS` flag native code reads at entry.
pub(crate) const CAPACITY_FROM_VALUES: i32 = std::mem::offset_of!(Header, capacity) as i32 - VALUES_OFFSET as i32;

/// The header every empty backing shares: never written, never freed.
static EMPTY: Header = Header {
    owners: 0,
    len: 0,
    capacity: 0,
    others: 0,
};

/// The `Header::capacity` flag of a backing whose header moved past dropped front values.
/// Native code never reads the capacity word, so the flag changes no native layout.
const BIASED: u32 = 1 << 31;

/// The `Header::capacity` flag of a backing whose every value is a raw double
/// (`UniqueElements::promote_doubles`): native code reads and writes its elements as `f64`
/// words, and every other reader sees ordinary untagged Number values.
pub(crate) const DOUBLE_ELEMENTS: u32 = 1 << 30;

/// The capacity word's flags, which its value masks out.
const CAPACITY_FLAGS: u32 = BIASED | DOUBLE_ELEMENTS;

/// The shortest backing whose front removal moves its header instead of its values: V8
/// moves the values of short arrays too, where the copy costs less than the bookkeeping.
const TRIM_MIN_LEN: usize = 16;

/// A backing with front room compacts in place, instead of growing, while at most
/// 1/COMPACT_LIVE_DIVISOR of it is live: each compaction then follows as many front removals
/// as it moves values.
const COMPACT_LIVE_DIVISOR: usize = 2;

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

    /// The young run (`HEADER_VALUES` and then `len` values) at `run` for the `len` values
    /// `value` gives for each index in order, admitting reads until a write or a survived
    /// collection copies it to an allocation (`make_mut`, `leave_nursery`).
    ///
    /// # Safety
    /// `run` starts `HEADER_VALUES + len` values of the arena's nursery that nothing else
    /// holds, and the array record that takes these elements is made before any collection.
    pub(crate) unsafe fn young(run: NonNull<Value>, len: usize, mut value: impl FnMut(usize) -> Value) -> Self {
        debug_assert!(len > 0);
        let header = run.cast::<Header>();
        let elements = Self { header };
        let mut counted = 0;
        for slot in 0..len {
            let written = value(slot);
            counted += other(written);
            // SAFETY: the run holds `len` values after the header.
            unsafe { elements.values().add(slot).write(written) };
        }
        // SAFETY: the run starts with room for a header, which no owner counts.
        unsafe {
            header.as_ptr().write(Header {
                owners: 0,
                len: len as u32,
                capacity: len as u32,
                others: counted,
            });
        }
        elements
    }

    /// Whether this backing is a young run (`ArrayElements::young`).
    #[inline(always)]
    pub(crate) fn is_young(&self) -> bool {
        self.header().owners == 0 && self.capacity() != 0
    }

    /// Copies a young run to an allocation of this array's own, as the collection it
    /// survives must before the nursery takes the run back; any other backing stays.
    pub(crate) fn leave_nursery(&mut self) {
        if self.is_young() {
            self.detach();
        }
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
        let capacity_word = u32::try_from(capacity).ok().filter(|word| word & CAPACITY_FLAGS == 0).expect("array backing capacity overflow");
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
        (self.header().capacity & !CAPACITY_FLAGS) as usize
    }

    /// Whether every value is a raw double (`UniqueElements::promote_doubles`).
    #[inline(always)]
    pub(crate) fn is_doubles(&self) -> bool {
        self.header().capacity & DOUBLE_ELEMENTS != 0
    }

    /// Clears the backing's double-elements kind: its words are ordinary Number values
    /// either way, so nothing else changes.
    #[inline(always)]
    pub(crate) fn clear_doubles(&mut self) {
        if self.is_doubles() && self.is_unique() {
            // SAFETY: a unique backing is this array's own allocation.
            unsafe { (*self.header.as_ptr()).capacity &= !DOUBLE_ELEMENTS };
        }
    }

    /// The values a front removal dropped ahead of the header (`BIASED`).
    #[inline(always)]
    fn bias(&self) -> usize {
        if self.header().capacity & BIASED == 0 {
            return 0;
        }
        // SAFETY: a biased header follows at least one dropped value, whose word holds the bias.
        unsafe { self.header.as_ptr().cast::<usize>().sub(1).read() }
    }

    /// The start and layout of the allocation the header lives in.
    fn allocation(&self) -> (*mut u8, Layout) {
        let bias = self.bias();
        // SAFETY: a biased header lies `bias` values into its allocation.
        let start = unsafe { self.header.as_ptr().cast::<Value>().sub(bias) };
        (start.cast(), Self::layout(self.capacity() + bias))
    }

    /// Requested allocation extent; the shared static empty header owns no allocation.
    pub(crate) fn allocated_bytes(&self) -> usize {
        match self.capacity() {
            0 => 0,
            _ => self.allocation().1.size(),
        }
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

    /// The owners of this backing; zero for the shared empty one and young runs.
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
        unsafe { crate::staged::elements::concrete_replace_element(values.as_ptr(), index, value) }
    }

    /// This array's values for writing: its own backing, copied first when shared or young
    /// (or when it is the shared empty header).
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
        match (self.header().owners, self.capacity()) {
            (0, 0) => {}
            // A young run has one owner, its array record: a copy owns an allocation.
            (0, _) => return Self::copied(self, self.header().others),
            (owners, _) => {
                // Only losing unique ownership can falsify a writable proof. A shared
                // backing already admits reads only; another owner changes neither its
                // address nor its bounds.
                if self.is_unique() {
                    super::array_proofs::retire_array_storage_proofs();
                }
                let owners = owners.checked_add(1).expect("array backing owners overflow");
                // SAFETY: arrays are single-threaded; the header is live and only owners write it.
                unsafe { (*self.header.as_ptr()).owners = owners };
            }
        }
        Self {
            header: self.header,
        }
    }
}

impl Drop for ArrayElements {
    fn drop(&mut self) {
        // The empty header and young runs count no owner, and no drop frees them.
        let Some(owners) = self.header().owners.checked_sub(1) else {
            return;
        };
        if owners == 0 {
            // SAFETY: the last owner frees the allocation `allocate` made for this capacity;
            // values need no drop.
            let (start, layout) = self.allocation();
            unsafe { dealloc(start, layout) };
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

mod mutation;
mod front_room;
#[cfg(test)]
mod tests;
