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
use quench_alloc::GRANULE_BYTES;
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

pub(crate) use quarantine::{begin_quarantine, concurrent_heap_values, end_quarantine, quarantined_bytes};
use quarantine::{quarantining, release};

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
/// The byte offset from a backing's first value to its `u32` length, which native appends advance.
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

/// The `Header::capacity` flag of a backing whose header moved past dropped front values.
pub(super) const BIASED: u32 = 1 << 31;

/// The `Header::capacity` flag of a wide backing made from a narrow one (`widened`): its
/// array was read hot or written since it narrowed, so no collection narrows it again
/// (`may_narrow`), and an array narrows and widens at most once. Front removals may drop it.
const KEEP_WIDE: u32 = 1 << 30;

/// The `Header::len` flag of a narrow backing: its values are 4-byte narrow forms
/// (`Value::to_narrow`) from `VALUES_OFFSET`, read through `ArrayElements::get`. A
/// replacement by a value with a narrow form stays narrow (`replace_in_place`); any other
/// write widens it first (`make_mut`). Native element reads test the flag where they load
/// the length (`staged::elements::array_backing`) and miss, except the interpreter's, which
/// decodes (`concrete_record_element`); a length read as an int32 fails its sign test. Only
/// a collection makes a narrow backing (`leave_nursery`, `narrow_in_place`), never one a
/// running view holds.
pub(crate) const NARROW_LEN: u32 = 1 << 31;

/// The `Header::len` flag of a double backing: every value word is the IEEE-754 bits of a
/// Number, whole or not (3 as 3.0's bits, never the int32 tag), and a hole is
/// `Value::DELETED`'s word, which arithmetic never produces; `Header::others` then counts
/// holes, and the backing holds no heap reference. Reads re-canonicalize (`Value::number`),
/// so values are unchanged. A Number stored into a double backing it alone owns stays double
/// (`replace_in_place`, `push_double`); any other write makes it generic first (`make_mut`),
/// for good (`KEEP_WIDE`). An all-int32 backing becomes double where it takes its first
/// other Number. Dense capacities stay below this bit (`grow_to`), and no backing is both
/// narrow and double. Format: research/nojit/n2-n4/double-kind-format.txt. Native readers
/// miss on it with the narrow flag (`ENCODED_LEN`), and only `QUENCH_DEV=double-elements`
/// makes one until compiled tiers guard it.
pub(crate) const DOUBLE_LEN: u32 = 1 << 30;

/// Either flag of a backing whose values are not 8-byte `Value`s in place.
pub(crate) const ENCODED_LEN: u32 = NARROW_LEN | DOUBLE_LEN;

/// Whether backings may take the double kind (`DOUBLE_LEN`).
#[inline(always)]
fn double_elements() -> bool {
    #[cfg(test)]
    if TEST_DOUBLES.get() {
        return true;
    }
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| crate::trace::Dev::DoubleElements.on())
}

#[cfg(test)]
thread_local! {
    /// The double kind on for this test thread alone (`double_elements`).
    static TEST_DOUBLES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The value a double backing's `word` holds.
#[inline(always)]
fn decode_double(word: u64) -> Value {
    if word == Value::DELETED.to_bits() { Value::DELETED } else { Value::number(f64::from_bits(word)) }
}

/// The word a double backing holds for the Number `number`: its bits, NaN canonical.
#[inline(always)]
fn double_word(number: f64) -> u64 {
    if number.is_nan() { f64::NAN.to_bits() } else { number.to_bits() }
}

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

/// Packed to four bytes: an array record (`ArrayObject`) holds it in its 4-aligned payload word.
#[repr(C, packed(4))]
pub(crate) struct ArrayElements {
    header: NonNull<Header>,
}

/// Offsets for the native read-only array view. These are derived beside the fields they
/// describe so native code does not maintain a second copy of the storage layout.
pub(super) const NATIVE_HEADER_POINTER_OFFSET: usize =
    std::mem::offset_of!(ArrayElements, header);
pub(super) const NATIVE_HEADER_LENGTH_OFFSET: usize = std::mem::offset_of!(Header, len);
pub(super) const NATIVE_HEADER_OWNERS_OFFSET: usize = std::mem::offset_of!(Header, owners);
/// The byte offset of the `u32` capacity word, which only the backing's allocator and growth change.
pub(super) const NATIVE_HEADER_CAPACITY_OFFSET: usize = std::mem::offset_of!(Header, capacity);
/// The capacity word's bits that count values: those left once the `BIASED` and `KEEP_WIDE`
/// flags are dropped.
pub(super) const NATIVE_HEADER_CAPACITY_MASK: u32 = !(BIASED | KEEP_WIDE);
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
        // The header is written last, so the values are addressed past it, not through it
        // (`values` reads the header's form).
        // SAFETY: the run starts with room for a header and holds `len` values after it.
        let values = unsafe { run.as_ptr().cast::<u8>().add(VALUES_OFFSET).cast::<Value>() };
        let mut counted = 0;
        for slot in 0..len {
            let written = value(slot);
            counted += other(written);
            // SAFETY: the run holds `len` values after the header.
            unsafe { values.add(slot).write(written) };
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
    /// Replaces a wide backing this array alone owns with a narrow copy (`NARROW_LEN`) where
    /// every value has a narrow form and the copy is smaller, as a full collection does for
    /// the arrays it keeps: the bytes it saves, or zero.
    /// Replaces a narrow backing with a wide copy of one owner, as a hot array needs for the
    /// native reads that do not decode narrow values: the same values.
    #[cold]
    pub(crate) fn widen(&mut self) {
        if self.is_encoded() {
            *self = self.widened();
        }
    }

    pub(crate) fn may_narrow(&self) -> bool {
        self.header().owners == 1 && self.len() != 0 && self.header().capacity & (KEEP_WIDE | BIASED) == 0 && !self.is_encoded()
    }

    pub(crate) fn narrow_in_place(&mut self) -> usize {
        if self.header().owners != 1 || self.is_encoded() || self.bias() != 0 {
            return 0;
        }
        let Some(narrow) = self.narrowed(self.allocated_bytes()) else {
            // Not every value has a narrow form, or the copy saves no block: no collection
            // tries again (a later write may change that; the array then stays wide).
            // SAFETY: a backing of one owner with a length is this array's allocation.
            unsafe { (*self.header.as_ptr()).capacity |= KEEP_WIDE };
            return 0;
        };
        let saved = self.allocated_bytes() - narrow.allocated_bytes();
        *self = narrow;
        saved
    }

    pub(crate) fn leave_nursery(&mut self) {
        if self.is_young() {
            self.detach();
        }
    }

    /// `leave_nursery` of the backing of an array a collection keeps: a narrow copy where it
    /// has one (`narrowed`), as unwritten young literals mostly stay unwritten.
    pub(crate) fn survive_nursery(&mut self) {
        if self.is_young() {
            match self.narrowed(Self::layout(self.len()).size()) {
                Some(narrow) => *self = narrow,
                None => self.detach(),
            }
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
        let capacity_word = u32::try_from(capacity).ok().filter(|word| word & (BIASED | KEEP_WIDE) == 0).expect("array backing capacity overflow");
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
        unsafe { &*{ self.header }.as_ptr() }
    }

    #[inline(always)]
    fn values(&self) -> *mut Value {
        // Every wide reader and writer comes through here: a narrow backing's words are not
        // values, so reaching one here means a caller skipped `widen` or `wide_values`.
        debug_assert!(!self.is_narrow(), "a narrow array backing read as wide values");
        // SAFETY: the values start right after the header in the same allocation; for the
        // shared empty header the pointer is aligned and never dereferenced.
        unsafe { self.header.as_ptr().cast::<u8>().add(VALUES_OFFSET).cast::<Value>() }
    }

    #[inline(always)]
    pub(crate) fn len(&self) -> usize {
        (self.header().len & !ENCODED_LEN) as usize
    }

    #[inline(always)]
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether this backing holds narrow values (`NARROW_LEN`).
    #[inline(always)]
    pub(crate) fn is_narrow(&self) -> bool {
        self.header().len & NARROW_LEN != 0
    }

    /// Whether this backing holds raw doubles (`DOUBLE_LEN`).
    #[inline(always)]
    pub(crate) fn is_double(&self) -> bool {
        self.header().len & DOUBLE_LEN != 0
    }

    /// Whether this backing's values are not `Value`s in place: narrow or double.
    #[inline(always)]
    pub(crate) fn is_encoded(&self) -> bool {
        self.header().len & ENCODED_LEN != 0
    }

    /// A double backing's words, as doubles (holes are `Value::DELETED`'s NaN word); `None`
    /// for any other backing.
    #[inline]
    pub(crate) fn double_values(&self) -> Option<&[f64]> {
        // SAFETY: a double backing's first `len` words are initialized f64 bits.
        self.is_double().then(|| unsafe { std::slice::from_raw_parts(self.double_words().cast::<f64>(), self.len()) })
    }

    #[inline(always)]
    fn double_words(&self) -> *mut u64 {
        // SAFETY: as in `values`: the words follow the header.
        unsafe { self.header.as_ptr().cast::<u8>().add(VALUES_OFFSET).cast::<u64>() }
    }

    #[inline(always)]
    fn narrow_values(&self) -> *const u32 {
        // SAFETY: as in `values`.
        unsafe { self.header.as_ptr().cast::<u8>().add(VALUES_OFFSET).cast::<u32>() }
    }

    /// The value at `index`, below the length.
    #[inline(always)]
    fn read(&self, index: usize) -> Value {
        debug_assert!(index < self.len());
        // SAFETY: the first `len` values are initialized, in the backing's form.
        unsafe {
            if !self.is_encoded() {
                self.values().add(index).read()
            } else if self.is_narrow() {
                Value::from_narrow(self.narrow_values().add(index).read())
            } else {
                decode_double(self.double_words().add(index).read())
            }
        }
    }

    /// The value at `index`, or `None` past the length.
    #[inline(always)]
    pub(crate) fn get(&self, index: usize) -> Option<Value> {
        (index < self.len()).then(|| self.read(index))
    }

    #[inline]
    pub(crate) fn first(&self) -> Option<Value> {
        self.get(0)
    }

    #[inline]
    pub(crate) fn last(&self) -> Option<Value> {
        self.len().checked_sub(1).map(|index| self.read(index))
    }

    /// The values in order.
    #[inline]
    pub(crate) fn iter(&self) -> impl DoubleEndedIterator<Item = Value> + ExactSizeIterator + Clone + '_ {
        (0..self.len()).map(move |index| self.read(index))
    }

    /// The values of `range` in order.
    #[inline]
    pub(crate) fn range(&self, range: std::ops::Range<usize>) -> impl DoubleEndedIterator<Item = Value> + ExactSizeIterator + '_ {
        assert!(range.start <= range.end && range.end <= self.len(), "array range out of bounds");
        range.map(move |index| self.read(index))
    }

    pub(crate) fn to_vec(&self) -> Vec<Value> {
        self.iter().collect()
    }

    /// The values of a wide backing as a slice; `None` for a narrow one.
    #[inline(always)]
    pub(crate) fn wide_values(&self) -> Option<&[Value]> {
        // SAFETY: the first `len` values of a wide backing are initialized.
        (!self.is_encoded()).then(|| unsafe { std::slice::from_raw_parts(self.values(), self.len()) })
    }

    /// `values_cow`.
    #[inline]
    pub(crate) fn as_slice(&self) -> std::borrow::Cow<'_, [Value]> {
        self.values_cow()
    }

    /// The values as a slice: borrowed from a wide backing, decoded from a narrow one.
    #[inline]
    pub(crate) fn values_cow(&self) -> std::borrow::Cow<'_, [Value]> {
        if self.is_encoded() {
            std::borrow::Cow::Owned(self.to_vec())
        } else {
            // SAFETY: the first `len` values of a wide backing are initialized.
            std::borrow::Cow::Borrowed(unsafe { std::slice::from_raw_parts(self.values(), self.len()) })
        }
    }

}

/// An array record keeps its elements' header pointer as a raw word where it has no extras
/// (`ArrayObject`): the word an `ArrayElements` is, owned by whoever holds it.
const _: () = assert!(size_of::<ArrayElements>() == size_of::<u64>());

impl ArrayElements {
    /// The header pointer as a raw word, which takes over this owner (`from_raw`). Its upper
    /// 16 bits are clear, so it is never an object's extras word (`EXTRAS_TAG`).
    #[inline(always)]
    pub(super) fn into_raw(self) -> u64 {
        let raw = self.header.as_ptr() as usize as u64;
        std::mem::forget(self);
        assert!(raw >> 48 == 0, "array elements outside 48-bit space");
        raw
    }

    /// The owner `into_raw` gave up.
    ///
    /// # Safety
    /// `raw` came from `into_raw` and no other `ArrayElements` holds that ownership.
    #[inline(always)]
    pub(super) unsafe fn from_raw(raw: u64) -> Self {
        // SAFETY: `into_raw` words are header pointers, never null.
        Self { header: unsafe { NonNull::new_unchecked(raw as usize as *mut Header) } }
    }

    /// The elements a raw word (`into_raw`) owns, borrowed in place.
    ///
    /// # Safety
    /// `raw` holds an `into_raw` word for the borrow's whole lifetime.
    #[inline(always)]
    pub(super) unsafe fn from_raw_ref<'a>(raw: *const u64) -> &'a Self {
        // SAFETY: `ArrayElements` is a pointer packed to the alignment of a record's payload word.
        unsafe { &*raw.cast::<Self>() }
    }

    /// `from_raw_ref`, mutably: a replacement written through it is a raw word again.
    ///
    /// # Safety
    /// As `from_raw_ref`.
    #[inline(always)]
    pub(super) unsafe fn from_raw_mut<'a>(raw: *mut u64) -> &'a mut Self {
        // SAFETY: as in `from_raw_ref`; the caller holds the word uniquely.
        unsafe { &mut *raw.cast::<Self>() }
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
            (0, _) => return self.widened(),
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
            unsafe { release(start, layout) };
        } else {
            // SAFETY: as in `clone`.
            unsafe { (*self.header.as_ptr()).owners = owners };
        }
    }
}


/// Tests read wide backings as slices; the runtime reads through `get`, `iter` and
/// `values_cow`, which also read narrow ones.
#[cfg(test)]
impl std::ops::Deref for ArrayElements {
    type Target = [Value];

    fn deref(&self) -> &[Value] {
        self.wide_values().expect("a test slices a wide backing")
    }
}

impl std::fmt::Debug for ArrayElements {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_list().entries(self.iter()).finish()
    }
}

impl PartialEq for ArrayElements {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().eq(other.iter())
    }
}


mod encoded;
mod quarantine;
mod front_room;
mod unique;
#[cfg(test)]
mod tests;
