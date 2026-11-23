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
// Raw-pointer field access predates `dangerous_implicit_autorefs`; the sites are reads and
// writes of `Copy` fields of a live record, and converting them to `addr_of!` is its own
// change (TOD-1202). Nothing here is a new allow: the lint did not exist when they landed.
#![allow(dangerous_implicit_autorefs)]
use crate::staged::record_heads::ArrayHeaderHead;
use super::growth::{GrowthPolicy, grown_capacity};
use crate::value::Value;
use std::alloc::{Layout, alloc, dealloc, handle_alloc_error, realloc};
use std::ptr::NonNull;

/// A backing's header: its words are `ArrayHeaderHead` (`staged::record_heads`), which the
/// stencils read by offset. `capacity` takes `BIASED` when the header was moved past values a
/// front removal dropped (`UniqueElements::remove` of index 0), and `others` is the fold of
/// every write's int32-ness, which native code reads with one load (`OTHERS_UNKNOWN` once it
/// stores uncounted values). Field accesses reach the head through `Deref`.
#[repr(C, align(8))]
struct Header {
    head: ArrayHeaderHead,
}

impl std::ops::Deref for Header {
    type Target = ArrayHeaderHead;
    #[inline(always)]
    fn deref(&self) -> &ArrayHeaderHead {
        &self.head
    }
}

impl std::ops::DerefMut for Header {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut ArrayHeaderHead {
        &mut self.head
    }
}

pub use crate::staged::encodings::OTHERS_UNKNOWN;

use crate::staged::elements::{concrete_other as other, concrete_recount as recounted};

fn others(values: &[Value]) -> u32 {
    values.iter().map(|value| other(*value)).sum()
}

/// The total number of values in `parts`.
fn parts_len(parts: &[&[Value]]) -> usize {
    parts.iter().map(|part| part.len()).sum()
}

/// Copies `parts` in order to `values`, which starts room for their total length, and returns
/// their count of other values, counted part by part as each is copied.
///
/// # Safety
/// `values` starts `parts_len(parts)` slots that nothing else reads or writes during the call,
/// and no part overlaps those slots.
unsafe fn write_parts(values: *mut Value, parts: &[&[Value]]) -> u32 {
    let mut counted = 0;
    let mut offset = 0;
    for &part in parts {
        counted += others(part);
        // SAFETY: the caller's contract; the slots from `offset` on hold exactly this part.
        unsafe { std::slice::from_raw_parts_mut(values.add(offset), part.len()).copy_from_slice(part) };
        offset += part.len();
    }
    counted
}


/// Frees a backing allocation no owner holds.
///
/// # Safety
/// `start` was allocated with `layout` and nothing uses it after this call.
#[inline(always)]
pub unsafe fn release(start: *mut u8, layout: Layout) {
    // SAFETY: the caller's contract.
    unsafe { dealloc(start, layout) };
}

/// The values start right after the header, at their own alignment.
const VALUES_OFFSET: usize = size_of::<Header>();
/// The arena values a young run's header takes ahead of its values.
pub const HEADER_VALUES: usize = VALUES_OFFSET / size_of::<Value>();
const _: () = assert!(HEADER_VALUES * size_of::<Value>() == VALUES_OFFSET);
const _: () = assert!(align_of::<Header>() <= align_of::<Value>());
const _: () = assert!(VALUES_OFFSET.is_multiple_of(align_of::<Value>()));
const _: () = assert!(align_of::<Header>() >= align_of::<Value>());
/// The byte offset from a backing's first value to its `u32` length, which native appends advance.
pub const LENGTH_FROM_VALUES: i32 = (std::mem::offset_of!(Header, head) + std::mem::offset_of!(ArrayHeaderHead, len)) as i32 - VALUES_OFFSET as i32;
/// The byte offset from a backing's first value to its `u32` count of other values
/// (`Header::others`), which native code reads at entry, and sets to `OTHERS_UNKNOWN` where
/// it may store uncounted values.
pub const OTHERS_FROM_VALUES: i32 = (std::mem::offset_of!(Header, head) + std::mem::offset_of!(ArrayHeaderHead, others)) as i32 - VALUES_OFFSET as i32;

/// The header every empty backing shares: never written, never freed.
static EMPTY: Header = Header {
    head: ArrayHeaderHead { owners: 0, len: 0, capacity: 0, others: 0 },
};

// The `Header::capacity` flags (`staged::object_encoding`): `BIASED` marks a header moved past dropped
// front values; `KEEP_WIDE` marks a wide backing made from a narrow one (`widened`), which no collection
// narrows again (`may_narrow`), so an array narrows and widens at most once. Front removals may drop it.
pub(super) use crate::staged::object_encoding::{BIASED, KEEP_WIDE};

/// The `Header::len` flag of a narrow backing: its values are 4-byte narrow forms
/// (`Value::to_narrow`) from `VALUES_OFFSET`, read through `ArrayElements::get`. A
/// replacement by a value with a narrow form stays narrow (`replace_in_place`); any other
/// write widens it first (`make_mut`). Native element reads test the flag where they load
/// the length (`staged::elements::array_backing`) and miss, except the interpreter's, which
/// decodes (`concrete_record_element`); a length read as an int32 fails its sign test. Only
/// a collection makes a narrow backing (`leave_nursery`, `narrow_in_place`), never one a
/// running view holds.
pub(crate) use crate::staged::encodings::NARROW_LEN;

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
pub use crate::staged::encodings::DOUBLE_LEN;

/// Either flag of a backing whose values are not 8-byte `Value`s in place.
pub use crate::staged::encodings::ENCODED_LEN;

/// Whether backings may take the double kind (`DOUBLE_LEN`): in production unless
/// `QUENCH_DEV=no-double-elements`; in a test only where it asks (`TEST_DOUBLES`), as tests
/// slice backings as wide values.
#[cfg(not(test))]
#[inline(always)]
fn double_elements() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !crate::trace::Dev::NoDoubleElements.on())
}

/// `double_elements` in a test.
#[cfg(test)]
#[inline(always)]
fn double_elements() -> bool {
    TEST_DOUBLES.get()
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

/// The block step of the allocator backings come from (`quench_alloc`'s size classes): a
/// narrow copy saves memory only where it fills a smaller block.
const GRANULE_BYTES: usize = 16;

/// The shortest backing whose front removal moves its header instead of its values: V8
/// moves the values of short arrays too, where the copy costs less than the bookkeeping.
const TRIM_MIN_LEN: usize = 16;

/// A backing with front room compacts in place, instead of growing, while at most
/// 1/COMPACT_LIVE_DIVISOR of it is live: each compaction then follows as many front removals
/// as it moves values.
const COMPACT_LIVE_DIVISOR: usize = 2;

/// The smallest capacity a growing backing takes, as `Vec` grows from empty. The backing
/// grows by half (`GrowthPolicy::Half`, in `heap::growth`), not doubled as `Vec`.
const MIN_GROWN_CAPACITY: usize = 4;

/// Packed to four bytes: an array record (`ArrayObject`) holds it in its 4-aligned payload word.
#[repr(C, packed(4))]
pub struct ArrayElements {
    header: NonNull<Header>,
}

/// Offsets for the native read-only array view. These are derived beside the fields they
/// describe so native code does not maintain a second copy of the storage layout.
pub(super) const NATIVE_HEADER_POINTER_OFFSET: usize =
    std::mem::offset_of!(ArrayElements, header);
pub(super) const NATIVE_HEADER_LENGTH_OFFSET: usize = std::mem::offset_of!(Header, head) + std::mem::offset_of!(ArrayHeaderHead, len);
pub(super) const NATIVE_HEADER_OWNERS_OFFSET: usize = std::mem::offset_of!(Header, head) + std::mem::offset_of!(ArrayHeaderHead, owners);
/// The byte offset of the `u32` capacity word, which only the backing's allocator and growth change.
pub(super) const NATIVE_HEADER_CAPACITY_OFFSET: usize = std::mem::offset_of!(Header, head) + std::mem::offset_of!(ArrayHeaderHead, capacity);
/// The capacity word's bits that count values: those left once the `BIASED` and `KEEP_WIDE`
/// flags are dropped.
pub(super) use crate::staged::object_encoding::HEADER_CAPACITY_MASK as NATIVE_HEADER_CAPACITY_MASK;
pub(super) const NATIVE_HEADER_VALUES_OFFSET: usize = VALUES_OFFSET;

const _: () = assert!(NATIVE_HEADER_POINTER_OFFSET == 0);
const _: () = assert!(NATIVE_HEADER_LENGTH_OFFSET + size_of::<u32>() <= size_of::<Header>());
const _: () = assert!(NATIVE_HEADER_VALUES_OFFSET >= size_of::<Header>());

/// An `ArrayElements` that no other array shares (`ArrayElements::make_mut`): the only view
/// through which values change.
#[repr(transparent)]
pub struct UniqueElements(ArrayElements);

impl ArrayElements {
    /// No values, in the shared empty header.
    #[inline(always)]
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
            (*elements.header.as_ptr()).head.others = others;
        }
        elements
    }

    /// A backing of the values of `parts`, in order, copied into one allocation of their total
    /// length, their count of other values taken by `others`'s rule, holes included.
    pub(crate) fn from_parts(parts: &[&[Value]]) -> Self {
        let len = parts_len(parts);
        if len == 0 {
            return Self::new();
        }
        let elements = Self::allocate(len, len);
        // SAFETY: the new allocation holds `len` slots after the header, and no part aliases it.
        let counted = unsafe { write_parts(elements.values(), parts) };
        // SAFETY: the allocation is fresh and this array alone owns it.
        unsafe { (*elements.header.as_ptr()).head.others = counted };
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

    /// `from_fn` of a constant: `len` copies of `value` in one allocation, filled as a slice,
    /// whose count of others is `len` when `value` is no int32 and none otherwise.
    pub(crate) fn filled(len: usize, value: Value) -> Self {
        if len == 0 {
            return Self::new();
        }
        let mut elements = Self::allocate(len, len);
        // SAFETY: the new allocation holds `len` slots after the header.
        unsafe { std::slice::from_raw_parts_mut(elements.values(), len) }.fill(value);
        let counted = if other(value) == 0 { 0 } else { len as u32 };
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
                head: ArrayHeaderHead { owners: 0, len: len as u32, capacity: len as u32, others: counted },
            });
        }
        elements
    }

    /// The young run at `run` for the values of `parts`, in order, as `from_parts` gives them:
    /// admitting reads until a write or a survived collection copies it (`make_mut`,
    /// `leave_nursery`).
    ///
    /// # Safety
    /// As for `young`: `run` starts `HEADER_VALUES + len` values that nothing else holds, where
    /// `len` is the parts' total, and the array record that takes these elements is made before
    /// any collection.
    pub(crate) unsafe fn young_from_parts(run: NonNull<Value>, parts: &[&[Value]]) -> Self {
        let len = parts_len(parts);
        debug_assert!(len > 0);
        let header = run.cast::<Header>();
        // SAFETY: the run starts with room for a header and holds `len` values after it.
        let values = unsafe { run.as_ptr().cast::<u8>().add(VALUES_OFFSET).cast::<Value>() };
        // SAFETY: the run holds `len` values after the header, and no part aliases it.
        let counted = unsafe { write_parts(values, parts) };
        // SAFETY: the run starts with room for a header, which no owner counts.
        unsafe {
            header.as_ptr().write(Header {
                head: ArrayHeaderHead { owners: 0, len: len as u32, capacity: len as u32, others: counted },
            });
        }
        Self { header }
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
            unsafe { (*self.header.as_ptr()).head.capacity |= KEEP_WIDE };
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
                head: ArrayHeaderHead { owners: 1, len: len as u32, capacity: capacity_word, others: 0 },
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
    pub fn len(&self) -> usize {
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
    pub fn is_double(&self) -> bool {
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
    pub fn double_values(&self) -> Option<&[f64]> {
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
    pub fn get(&self, index: usize) -> Option<Value> {
        (index < self.len()).then(|| self.read(index))
    }

    #[inline]
    pub(crate) fn first(&self) -> Option<Value> {
        self.get(0)
    }

    /// The values in order.
    #[inline]
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = Value> + ExactSizeIterator + Clone + '_ {
        (0..self.len()).map(move |index| self.read(index))
    }

    /// The values of `range` in order.
    #[inline]
    pub fn range(&self, range: std::ops::Range<usize>) -> impl DoubleEndedIterator<Item = Value> + ExactSizeIterator + '_ {
        assert!(range.start <= range.end && range.end <= self.len(), "array range out of bounds");
        range.map(move |index| self.read(index))
    }

    pub(crate) fn to_vec(&self) -> Vec<Value> {
        self.iter().collect()
    }

    /// The values of a wide backing as a slice; `None` for a narrow one.
    #[inline(always)]
    pub fn wide_values(&self) -> Option<&[Value]> {
        // SAFETY: the first `len` values of a wide backing are initialized.
        (!self.is_encoded()).then(|| unsafe { std::slice::from_raw_parts(self.values(), self.len()) })
    }

    /// The packed words of a narrow backing (`Value::to_narrow`) as a slice; `None` for any
    /// other. A collection's trace reads them word by word: a heap test and a decode per
    /// element, with no per-element form dispatch (`read`).
    #[inline(always)]
    pub(crate) fn narrow_words(&self) -> Option<&[u32]> {
        // SAFETY: the first `len` words of a narrow backing are initialized.
        (self.is_encoded() && self.is_narrow()).then(|| unsafe { std::slice::from_raw_parts(self.narrow_values(), self.len()) })
    }

    /// `values_cow`.
    #[inline]
    pub fn as_slice(&self) -> std::borrow::Cow<'_, [Value]> {
        self.values_cow()
    }

    /// The values as a slice: borrowed from a wide backing, decoded from a narrow one.
    #[inline]
    pub fn values_cow(&self) -> std::borrow::Cow<'_, [Value]> {
        if self.is_encoded() {
            std::borrow::Cow::Owned(self.to_vec())
        } else {
            // SAFETY: the first `len` values of a wide backing are initialized.
            std::borrow::Cow::Borrowed(unsafe { std::slice::from_raw_parts(self.values(), self.len()) })
        }
    }

    /// A narrow backing (`NARROW_LEN`) of `values`, when each has a narrow form and the
    /// narrow allocation is smaller than a wide one.
    /// `wide_bytes` is what the wide form takes: the narrow one must fill a smaller block of
    /// the allocator's (`GRANULE_BYTES` steps), or it saves nothing.
    fn narrowed(&self, wide_bytes: usize) -> Option<Self> {
        let values = self.wide_values()?;
        let len = values.len();
        let layout = Self::narrow_layout(len);
        if len == 0 || layout.size().next_multiple_of(GRANULE_BYTES) >= wide_bytes.next_multiple_of(GRANULE_BYTES) {
            return None;
        }
        // SAFETY: the layout has a non-zero size (the header).
        let pointer = unsafe { alloc(layout) }.cast::<Header>();
        let Some(header) = NonNull::new(pointer) else {
            handle_alloc_error(layout);
        };
        // SAFETY: the allocation is fresh and sized for a header and `len` narrow values; one
        // without a narrow form for some value is freed unused.
        unsafe {
            let narrow = header.as_ptr().cast::<u8>().add(VALUES_OFFSET).cast::<u32>();
            for (index, value) in values.iter().enumerate() {
                let Some(word) = value.to_narrow() else {
                    dealloc(pointer.cast(), layout);
                    return None;
                };
                narrow.add(index).write(word);
            }
            header.as_ptr().write(Header {
                head: ArrayHeaderHead { owners: 1, len: len as u32 | NARROW_LEN, capacity: len as u32, others: self.header().others },
            });
        }
        Some(Self { header })
    }

    fn narrow_layout(capacity: usize) -> Layout {
        Layout::from_size_align(
            VALUES_OFFSET + capacity.checked_mul(size_of::<u32>()).expect("array backing overflow"),
            align_of::<Header>(),
        )
        .expect("array backing overflow")
    }

    #[inline(always)]
    pub fn capacity(&self) -> usize {
        (self.header().capacity & !(BIASED | KEEP_WIDE)) as usize
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
        if self.is_narrow() {
            return (self.header.as_ptr().cast(), Self::narrow_layout(self.capacity()));
        }
        let bias = self.bias();
        // SAFETY: a biased header lies `bias` values into its allocation.
        let start = unsafe { self.header.as_ptr().cast::<Value>().sub(bias) };
        (start.cast(), Self::layout(self.capacity() + bias))
    }

    /// The bytes the values take (capacity times the value width: 4 narrow, 8 wide).
    #[inline]
    pub(crate) fn value_bytes(&self) -> usize {
        self.capacity() * if self.is_narrow() { size_of::<u32>() } else { size_of::<Value>() }
    }

    /// Requested allocation extent; the shared static empty header owns no allocation.
    pub(crate) fn allocated_bytes(&self) -> usize {
        match self.capacity() {
            0 => 0,
            _ => self.allocation().1.size(),
        }
    }

    /// Whether every value is an int32 (so none is a hole, and none a heap reference).
    #[inline]
    pub(crate) fn holds_only_int32(&self) -> bool {
        // A double backing's count is of holes (`DOUBLE_LEN`).
        self.header().others == 0 && !self.is_double()
    }

    /// Whether this array alone owns its backing, so writes reach no other array.
    #[inline(always)]
    fn is_unique(&self) -> bool {
        self.header().owners == 1 && !self.is_encoded()
    }

    /// The backing's identity, which arrays that share it have in common.
    #[inline] pub(crate) fn as_ptr(&self) -> *const () {
        self.header.as_ptr().cast()
    }

    /// The owners of this backing; zero for the shared empty one and young runs.
    #[inline] pub fn owner_count(&self) -> usize {
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
            return self.replace_encoded_in_place(index, value);
        }
        if self.header().others == 0 && !value.is_tagged() && self.takes_double() {
            self.enter_double();
            return self.replace_encoded_in_place(index, value);
        }
        // SAFETY: `UniqueElements` is a transparent wrapper; the backing is this array's own.
        unsafe { &mut *(self as *mut Self).cast::<UniqueElements>() }.set(index, value);
        true
    }

    /// `replace_in_place` on an encoded backing this array alone owns: a double one takes a
    /// Number and a narrow one a value with a narrow form, and stays so. `false` changes
    /// nothing otherwise.
    #[cold]
    fn replace_encoded_in_place(&mut self, index: usize, value: Value) -> bool {
        if self.header().owners != 1 {
            return false;
        }
        if self.is_double() {
            let Some(number) = value.as_number() else {
                return false;
            };
            assert!(index < self.len(), "array store index out of bounds");
            // SAFETY: a double backing this array alone owns holds `len` words.
            unsafe {
                let word = self.double_words().add(index);
                if word.read() == Value::DELETED.to_bits() {
                    (*self.header.as_ptr()).head.others -= 1;
                }
                word.write(double_word(number));
            }
            return true;
        }
        if !self.is_narrow() {
            return false;
        }
        let Some(narrow) = value.to_narrow() else {
            return false;
        };
        assert!(index < self.len(), "array store index out of bounds");
        let old = self.read(index);
        let (added, removed) = (other(value), other(old));
        // SAFETY: a narrow backing this array alone owns holds `len` narrow values.
        unsafe {
            if added != removed {
                let header = self.header.as_ptr();
                (*header).head.others = recounted((*header).head.others, added, removed);
            }
            self.narrow_values().cast_mut().add(index).write(narrow);
        }
        true
    }

    #[cfg(test)]
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

    /// Whether this wide backing, which this array alone owns, may become double on its
    /// first non-int32 Number: never one made generic from an encoded backing (`KEEP_WIDE`).
    #[inline(always)]
    fn takes_double(&self) -> bool {
        self.header().capacity & (KEEP_WIDE | BIASED) == 0 && double_elements()
    }

    /// Turns a wide backing of int32 values alone (`holds_only_int32`), which this array
    /// owns alone, into a double one in place: each value's word becomes its double's bits.
    /// The words stop being `Value`s, so the running lane's array storage proofs, which read
    /// and write them as such, retire first (`DOUBLE_LEN`).
    #[cold]
    #[inline(never)]
    fn enter_double(&mut self) {
        debug_assert!(self.is_unique() && self.header().others == 0);
        super::array_proofs::retire_array_storage_proofs();
        let words = self.double_words();
        for index in 0..self.len() {
            // SAFETY: the backing holds `len` initialized int32 values, this array's own.
            unsafe {
                let value = Value::from_bits(words.add(index).read());
                words.add(index).write(double_word(value.as_number().unwrap_or(f64::NAN)));
            }
        }
        // SAFETY: as above; no value is a hole.
        unsafe {
            let header = self.header.as_ptr();
            (*header).head.len |= DOUBLE_LEN;
            (*header).head.others = 0;
        }
    }

    /// An empty double backing of one owner with room for its first appends, where backings
    /// take the double kind (`double_elements`); the shared empty header where they do not.
    /// A TypeScript `number[]` binding's empty literal starts with it (`Vm::make_array`).
    pub(crate) fn double_with_room() -> Self {
        if !double_elements() {
            return Self::new();
        }
        let elements = Self::allocate(MIN_GROWN_CAPACITY, 0);
        // SAFETY: a fresh allocation of one owner, empty.
        unsafe { (*elements.header.as_ptr()).head.len = DOUBLE_LEN };
        elements
    }

    /// Appends the Number `value` to a double backing this array alone owns, or to an empty
    /// or all-int32 one that `value` (not an int32) turns double, growing as `reserve` does.
    /// `false` changes nothing: the caller appends as to any backing, which makes a double
    /// one generic first. Answers the capacity it grew by through `grown`.
    pub(crate) fn push_double(&mut self, value: Value, grown: &mut usize) -> bool {
        if !self.is_double() {
            if value.is_tagged() || !double_elements() {
                return false;
            }
            if self.capacity() == 0 {
                *self = Self::double_with_room();
                *grown = MIN_GROWN_CAPACITY;
            } else if self.is_unique() && self.header().others == 0 && self.takes_double() {
                self.enter_double();
            } else {
                return false;
            }
        } else if self.header().owners != 1 {
            return false;
        }
        let Some(number) = value.as_number() else {
            return false;
        };
        let len = self.len();
        if len == self.capacity() {
            let before = self.capacity();
            // SAFETY: `UniqueElements` is a transparent wrapper; growth copies the header with
            // its flag and every word, and this array owns the backing alone.
            unsafe { &mut *(self as *mut Self).cast::<UniqueElements>() }.grow_for(len + 1);
            *grown += self.capacity() - before;
        }
        // SAFETY: below the capacity of a double backing this array alone owns.
        unsafe {
            self.double_words().add(len).write(double_word(number));
            (*self.header.as_ptr()).head.len += 1;
        }
        true
    }

    /// Room for `additional` more words in a double backing this array alone owns, grown as
    /// `UniqueElements::reserve` grows: the flag and the hole count stay, as a native
    /// append into the room then needs (`Vm::reserve_native_array_append`).
    pub fn reserve_double(&mut self, additional: usize) {
        assert!(self.is_double() && self.header().owners == 1, "double room in a backing of its own");
        let required = self.len().checked_add(additional).expect("array backing overflow");
        if required > self.capacity() {
            // SAFETY: as in `push_double`.
            unsafe { &mut *(self as *mut Self).cast::<UniqueElements>() }.grow_for(required);
        }
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

    /// `make_mut` of a backing already this array's own and wide, which copies nothing;
    /// `None`, which changes nothing, where `make_mut` would copy it.
    #[inline(always)]
    pub(crate) fn unique_mut(&mut self) -> Option<&mut UniqueElements> {
        // SAFETY: `UniqueElements` is a transparent wrapper; the backing is this array's own.
        self.is_unique().then(|| unsafe { &mut *(self as *mut Self).cast::<UniqueElements>() })
    }

    #[cold]
    #[inline(never)]
    fn detach(&mut self) {
        *self = self.widened();
    }

    /// A wide copy of this backing, of one owner.
    fn widened(&self) -> Self {
        if self.is_encoded() {
            // An empty double backing (`double_with_room`) widens to no allocation.
            if self.is_empty() {
                return Self::new();
            }
            let mut copy = Self::from_fn(self.len(), |index| self.read(index));
            if self.is_narrow() {
                copy.make_mut().set_others(self.header().others);
            }
            // SAFETY: the copy is a fresh allocation of one owner (the length is not zero).
            unsafe { (*copy.header.as_ptr()).head.capacity |= KEEP_WIDE };
            return copy;
        }
        // SAFETY: the first `len` values of a wide backing are initialized.
        Self::copied(unsafe { std::slice::from_raw_parts(self.values(), self.len()) }, self.header().others)
    }
}

/// An array record keeps its elements' header pointer as a raw word where it has no extras
/// (`ArrayObject`): the word an `ArrayElements` is, owned by whoever holds it.
const _: () = assert!(size_of::<ArrayElements>() == size_of::<u64>());

/// The raw word (`ArrayElements::into_raw`) of the shared empty header, which no array owns.
#[inline(always)]
pub(super) fn empty_header_word() -> u64 {
    std::ptr::addr_of!(EMPTY) as usize as u64
}

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
                unsafe { (*self.header.as_ptr()).head.owners = owners };
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
            unsafe { (*self.header.as_ptr()).head.owners = owners };
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

impl UniqueElements {
    #[inline] fn set_others(&mut self, others: u32) {
        if self.0.capacity() != 0 {
            // SAFETY: a unique backing with a capacity is this array's own allocation.
            unsafe { (*self.0.header.as_ptr()).head.others = others };
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
        self.recount(other(value), other(self[index]));
        // SAFETY: the index is below the length of this unique backing.
        unsafe { self.0.values().add(index).write(value) };
    }

    /// `value` at every index of `range`, counting the others it overwrites. The count of others
    /// the range holds is known without a scan when the backing counts none, or when the range is
    /// the whole backing and its count is known; then the range is a plain slice fill. Otherwise
    /// one pass counts the others it overwrites and stores `value` over them.
    pub(crate) fn fill(&mut self, range: std::ops::Range<usize>, value: Value) {
        assert!(range.start <= range.end && range.end <= self.0.len(), "array fill out of bounds");
        let added = other(value) * range.len() as u32;
        let counted = self.0.header().others;
        let known = if counted == 0 {
            Some(0)
        } else if counted != OTHERS_UNKNOWN && range.start == 0 && range.end == self.0.len() {
            Some(counted)
        } else {
            None
        };
        // SAFETY: the range is within the length of this unique backing, which owns its values.
        let slots = unsafe { std::slice::from_raw_parts_mut(self.0.values().add(range.start), range.len()) };
        let removed = match known {
            Some(removed) => {
                slots.fill(value);
                removed
            }
            None => {
                let mut removed = 0;
                for slot in slots {
                    removed += other(*slot);
                    *slot = value;
                }
                removed
            }
        };
        self.recount(added, removed);
    }

    /// Reverses the values and returns `true`, unless one is a hole (`Value::DELETED`): then nothing
    /// changes and this returns `false`. The count of other values is unchanged. A hole counts as
    /// other, so an exact count of zero proves there is none and the scan is skipped.
    pub(crate) fn reverse_unless_holed(&mut self) -> bool {
        let counted_none = self.0.header().others == 0;
        // SAFETY: a unique backing is wide; its first `len` values are initialized.
        let values = unsafe { std::slice::from_raw_parts_mut(self.0.values(), self.0.len()) };
        if !counted_none && values.iter().any(|value| value.is_deleted()) {
            return false;
        }
        values.reverse();
        true
    }

    /// `values` at the indices from `start`.
    pub(crate) fn copy_from(&mut self, start: usize, values: &[Value]) {
        assert!(start + values.len() <= self.0.len(), "array copy out of bounds");
        self.recount(others(values), others(&self[start..start + values.len()]));
        // SAFETY: the range is within the length; `values` cannot alias this unique backing.
        unsafe { std::ptr::copy_nonoverlapping(values.as_ptr(), self.0.values().add(start), values.len()) };
    }

    /// The first value, for native code that writes the backing and keeps its count of
    /// other values (`OTHERS_FROM_VALUES`).
    pub fn as_mut_ptr(&mut self) -> *mut Value {
        self.0.values()
    }

    #[inline] fn set_len(&mut self, len: usize) {
        debug_assert!(len <= self.0.capacity());
        if self.0.capacity() != 0 {
            // SAFETY: a unique backing with a capacity is this array's own allocation.
            unsafe { (*self.0.header.as_ptr()).head.len = len as u32 };
        }
    }

    /// Room for `additional` more values, growing by half (`GrowthPolicy::Half`). Inline: a backing with
    /// room, the common append, tests its capacity alone.
    #[inline(always)]
    pub fn reserve(&mut self, additional: usize) {
        let required = self.0.len().checked_add(additional).expect("array backing overflow");
        if required > self.0.capacity() {
            self.grow_for(required);
        }
    }

    #[cold]
    #[inline(never)]
    fn grow_for(&mut self, required: usize) {
        if self.0.bias() != 0 {
            self.unbias();
            if required <= self.0.capacity() && self.0.len() <= self.0.capacity() / COMPACT_LIVE_DIVISOR {
                return;
            }
        }
        let capacity = self.0.capacity();
        let grown = grown_capacity(capacity, required, GrowthPolicy::Half { minimum: MIN_GROWN_CAPACITY });
        self.grow_to(grown);
    }

    fn grow_to(&mut self, capacity: usize) {
        let capacity_word = u32::try_from(capacity).ok().filter(|word| word & (BIASED | KEEP_WIDE) == 0).expect("array backing capacity overflow");
        let old = self.0.capacity();
        if old == 0 {
            // The shared empty header: a fresh backing of its own.
            self.0 = ArrayElements::allocate(capacity, 0);
            return;
        }
        let layout = ArrayElements::layout(capacity);
        let old_layout = ArrayElements::layout(old);
        // SAFETY: this unique backing was allocated with the layout of its old capacity.
        let pointer = unsafe { realloc(self.0.header.as_ptr().cast(), old_layout, layout.size()) }
        .cast::<Header>();
        let Some(header) = NonNull::new(pointer) else {
            handle_alloc_error(layout);
        };
        self.0.header = header;
        // SAFETY: the reallocated header is this array's own, its words copied from the old.
        unsafe { (*header.as_ptr()).head.capacity = capacity_word | ((*header.as_ptr()).head.capacity & KEEP_WIDE) };
    }

    #[inline(always)]
    pub(crate) fn push(&mut self, value: Value) {
        let len = self.0.len();
        self.reserve(1);
        self.recount(other(value), 0);
        // SAFETY: `reserve` made room for one more value.
        unsafe { self.0.values().add(len).write(value) };
        self.set_len(len + 1);
    }

    #[inline(always)]
    pub(crate) fn pop(&mut self) -> Option<Value> {
        let len = self.0.len().checked_sub(1)?;
        let value = self[len];
        self.recount(0, other(value));
        self.set_len(len);
        Some(value)
    }

    pub(crate) fn truncate(&mut self, len: usize) {
        if len < self.0.len() {
            self.recount(0, others(&self[len..]));
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
        if index == 0 && len >= TRIM_MIN_LEN && self.0.capacity() != 0 {
            self.trim_front();
            return value;
        }
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
        if range.end == 0 && len >= TRIM_MIN_LEN {
            return self.prepend(values);
        }
        let removed = range.end - range.start;
        if values.len() > removed {
            self.reserve(values.len() - removed);
        }
        let new_len = len - removed + values.len();
        self.recount(others(values), others(&self[range.clone()]));
        // SAFETY: the tail moves within the reserved allocation, then `values` fills the gap.
        unsafe {
            let base = self.0.values();
            std::ptr::copy(base.add(range.end), base.add(range.start + values.len()), len - range.end);
            std::ptr::copy_nonoverlapping(values.as_ptr(), base.add(range.start), values.len());
        }
        self.set_len(new_len);
    }

    pub fn capacity(&self) -> usize {
        self.0.capacity()
    }
}

impl std::ops::Deref for UniqueElements {
    type Target = [Value];

    #[inline(always)]
    fn deref(&self) -> &[Value] {
        debug_assert!(!self.0.is_encoded());
        // SAFETY: a unique backing is wide; its first `len` values are initialized.
        unsafe { std::slice::from_raw_parts(self.0.values(), self.0.len()) }
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

mod front_room;
#[cfg(test)]
mod tests;
