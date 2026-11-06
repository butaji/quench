//! Operation semantics written once, over a binding-time domain (`Stage`).
//!
//! Futamura's first projection made literal, by Ershov's mixed computation: each operation
//! below is one Rust function generic over the stage that runs it. The interpreter runs it
//! on machine words (`Concrete`); the specializer runs it on IR values, which emits the
//! residual code (`compiled::object_slots::Emit`), for the JIT and, under a build-time
//! division, AOT. What the function holds as a plain Rust value (an exact shape word, a
//! slot) is static: the interpreter decides on it as it runs, the specializer folds it, so a
//! branch on it leaves only the taken arm in native code. An improvement to an operation here
//! is one for every tier.

mod numbers;

pub(crate) use numbers::{AnyOperands, IntOperands, Number, NumberOperands, NumberOperator, Numbers, Source, binary, binary_values, relation};

use crate::heap::{PlainPropertyStorage, NATIVE_OBJECT_LAYOUT as OBJECT};
use crate::value::{NarrowLayout, ValueLayout};
use crate::value_vec::INLINE_PROPERTY_COUNT;

/// A binding-time domain: the machine words an operation computes on and the guards that
/// leave it. Words are 64 bits; a 32-bit load zero-extends.
pub(crate) trait Stage {
    type Word: Copy;
    type Cond: Copy;
    /// Where a failed guard goes: the interpreter's slow path, or a native exit.
    type Miss: Copy;
    fn constant(&mut self, bits: u64) -> Self::Word;
    fn load_u32(&mut self, base: Self::Word, offset: i32) -> Self::Word;
    fn load_u64(&mut self, base: Self::Word, offset: i32) -> Self::Word;
    fn store_u32(&mut self, base: Self::Word, offset: i32, value: Self::Word);
    fn store_u64(&mut self, base: Self::Word, offset: i32, value: Self::Word);
    /// A load from a table that never changes while the process runs.
    fn load_table(&mut self, address: Self::Word) -> Self::Word;
    fn add(&mut self, a: Self::Word, b: Self::Word) -> Self::Word;
    fn or(&mut self, a: Self::Word, b: Self::Word) -> Self::Word;
    fn add_imm(&mut self, a: Self::Word, imm: u64) -> Self::Word;
    fn mul_imm(&mut self, a: Self::Word, imm: u64) -> Self::Word;
    fn and_imm(&mut self, a: Self::Word, imm: u64) -> Self::Word;
    fn or_imm(&mut self, a: Self::Word, imm: u64) -> Self::Word;
    fn shr_imm(&mut self, a: Self::Word, shift: u32) -> Self::Word;
    /// The low 32 bits shifted right arithmetically, zero-extended to the word.
    fn sar32_imm(&mut self, a: Self::Word, shift: u32) -> Self::Word;
    fn eq_imm(&mut self, a: Self::Word, imm: u64) -> Self::Cond;
    fn ne_imm(&mut self, a: Self::Word, imm: u64) -> Self::Cond;
    /// Whether `a` is above `imm`, both unsigned.
    fn above_imm(&mut self, a: Self::Word, imm: u64) -> Self::Cond;
    /// Whether `a` is below `b`, both unsigned.
    fn below(&mut self, a: Self::Word, b: Self::Word) -> Self::Cond;
    /// Whether `a` is below `imm`, both unsigned.
    fn below_imm(&mut self, a: Self::Word, imm: u64) -> Self::Cond;
    /// The word `then` computes where `cond` holds, else `otherwise`'s: only the arm taken runs.
    /// `None` where the arm taken refuses (an emitting stage: either does).
    fn choose(&mut self, cond: Self::Cond, then: impl FnOnce(&mut Self) -> Option<Self::Word>, otherwise: impl FnOnce(&mut Self) -> Option<Self::Word>) -> Option<Self::Word>;
    /// Whether `a` or `b` holds.
    fn either(&mut self, a: Self::Cond, b: Self::Cond) -> Self::Cond;
    /// Continues where `cond` holds; `None` where it never can, so nothing runs past it.
    fn guard(&mut self, cond: Self::Cond, miss: Self::Miss) -> Option<()>;
    /// The base address of `table`.
    fn table(&mut self, table: Table) -> Self::Word;
}

/// The process tables an operation addresses from a base: the property arena's values, which
/// a flat object's run starts at its position, and the object and array record reservations,
/// which a record id indexes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Table {
    PropertyValues,
    ObjectRecords,
    ArrayRecords,
}

impl Table {
    pub(crate) const COUNT: usize = 3;
}

/// Where an object of a proved exact shape keeps its named values: the payload word that packs
/// them, read at once or at each read, the address of its arena run, or of the words its
/// extras displaced them into.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Slots<W> {
    Packed(W),
    PackedRecord(W),
    Arena(W),
    Displaced(W),
}

/// The layout a proved exact shape gives an object record (`layout`), and how many values its
/// shape holds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Layout<W> {
    pub(crate) slots: Slots<W>,
    pub(crate) storage_len: usize,
}

/// Whether the object record at `record` has the exact shape word `exact`
/// (`Object::exact_shape`), which proves its storage form with its shape.
#[inline(always)]
pub(crate) fn exact_shape_is<S: Stage>(stage: &mut S, record: S::Word, exact: u32) -> S::Cond {
    let shape = stage.load_u32(record, OBJECT.auxiliary_offset as i32);
    stage.eq_imm(shape, u64::from(exact))
}

/// The layout of the object record at `record`, whose exact shape the caller proved `exact`:
/// the form that word names fixes where the values live (`PlainPropertyStorage::of_exact`), so
/// it reads the payload word once and tests nothing. `snapshot` keeps the packed values as read
/// here, for a caller no store reaches; else each read loads them again.
#[inline(always)]
pub(crate) fn layout<S: Stage>(stage: &mut S, record: S::Word, exact: u32, storage_len: usize, snapshot: bool) -> Layout<S::Word> {
    let arena_run = |stage: &mut S, start: S::Word| {
        let offset = stage.mul_imm(start, OBJECT.property_value_size as u64);
        let base = stage.table(Table::PropertyValues);
        Slots::Arena(stage.add(base, offset))
    };
    let slots = match PlainPropertyStorage::of_exact(exact) {
        PlainPropertyStorage::PackedInline if snapshot => Slots::Packed(stage.load_u64(record, OBJECT.payload_offset as i32)),
        PlainPropertyStorage::PackedInline => Slots::PackedRecord(record),
        // A flat object's payload word is its run's position alone (`Object::plain_storage_start`).
        PlainPropertyStorage::Arena => {
            let start = stage.load_u64(record, OBJECT.payload_offset as i32);
            arena_run(stage, start)
        }
        // An object with extras points its payload word at them, which hold its inline values
        // or its run's start (`Object::extras_storage_start`).
        PlainPropertyStorage::ExtrasInline => {
            let extras = stage.load_u64(record, OBJECT.payload_offset as i32);
            Slots::Displaced(stage.add_imm(extras, OBJECT.extras_inline_offset as u64))
        }
        PlainPropertyStorage::ExtrasArena => {
            let extras = stage.load_u64(record, OBJECT.payload_offset as i32);
            let start = stage.load_u32(extras, OBJECT.extras_arena_start_offset as i32);
            arena_run(stage, start)
        }
    };
    Layout { slots, storage_len }
}

/// The record of the immediate prototype of the object record at `record`, where it is an
/// object record (`ProtoLink::object_record_id`); `miss` for any other prototype.
#[inline(always)]
pub(crate) fn prototype_record<S: Stage>(stage: &mut S, record: S::Word, miss: S::Miss) -> Option<S::Word> {
    let meta = stage.load_u32(record, OBJECT.meta_offset as i32);
    let link = stage.and_imm(meta, u64::from(OBJECT.proto_mask));
    // One unsigned test bounds the link to the object records' range from both sides.
    let id = stage.add_imm(link, u64::from(OBJECT.proto_record_base).wrapping_neg());
    let object = stage.below_imm(id, u64::from(OBJECT.proto_record_end - OBJECT.proto_record_base));
    stage.guard(object, miss)?;
    let offset = stage.mul_imm(id, OBJECT.record_size as u64);
    let records = stage.table(Table::ObjectRecords);
    Some(stage.add(records, offset))
}

/// Value `slot` of the immediate prototype of the object record at `record`, where that
/// prototype is an object record of the exact shape `holder` holding `storage_len` values;
/// `miss` for any other prototype. The read of the prototype's own value is `layout`'s.
#[inline(always)]
pub(crate) fn holder_slot<S: Stage>(stage: &mut S, record: S::Word, holder: u32, slot: u16, storage_len: usize, miss: S::Miss) -> Option<S::Word> {
    let prototype = prototype_record(stage, record, miss)?;
    let matched = exact_shape_is(stage, prototype, holder);
    stage.guard(matched, miss)?;
    let layout = layout(stage, prototype, holder, storage_len, false);
    Some(read_slot(stage, layout, slot))
}

/// One proved case of a field site: an object of the exact shape `exact` holds the value at
/// `slot`, its own or, for a `holder`, its immediate prototype's of that exact shape, among
/// `storage_len` values.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct FieldCase {
    pub(crate) exact: u32,
    pub(crate) holder: Option<u32>,
    pub(crate) slot: u16,
    pub(crate) storage_len: usize,
}

/// The value `case` reads from the object record at `record`, whose exact shape is `case`'s.
#[inline(always)]
fn case_read<S: Stage>(stage: &mut S, record: S::Word, case: FieldCase, miss: S::Miss) -> Option<S::Word> {
    match case.holder {
        None => {
            let layout = layout(stage, record, case.exact, case.storage_len, false);
            Some(read_slot(stage, layout, case.slot))
        }
        Some(holder) => holder_slot(stage, record, holder, case.slot, case.storage_len, miss),
    }
}

/// The value a field site's proved `cases` read from the object record at `record`, whose
/// exact shape word is `shape`; `miss` for any other shape. Own values at one slot in one
/// storage form are a layout family: one membership test and one read serve them, bounded by
/// the shortest storage, which still holds the slot. Other cases switch on the shape.
#[inline(always)]
pub(crate) fn field_read<S: Stage>(stage: &mut S, record: S::Word, shape: S::Word, cases: &[FieldCase], miss: S::Miss) -> Option<S::Word> {
    let (first, rest) = cases.split_first()?;
    let family = |case: &FieldCase| case.holder.is_none() && case.slot == first.slot
        && PlainPropertyStorage::of_exact(case.exact) == PlainPropertyStorage::of_exact(first.exact);
    if cases.iter().all(family) {
        let mut member = stage.eq_imm(shape, u64::from(first.exact));
        for case in rest {
            let same = stage.eq_imm(shape, u64::from(case.exact));
            member = stage.either(member, same);
        }
        stage.guard(member, miss)?;
        let shortest = cases.iter().copied().min_by_key(|case| case.storage_len)?;
        return case_read(stage, record, shortest, miss);
    }
    let matched = stage.eq_imm(shape, u64::from(first.exact));
    if rest.is_empty() {
        stage.guard(matched, miss)?;
        return case_read(stage, record, *first, miss);
    }
    stage.choose(matched, |stage| case_read(stage, record, *first, miss), |stage| field_read(stage, record, shape, rest, miss))
}

/// Value `slot` of an object of `layout`, which holds it.
#[inline(always)]
pub(crate) fn read_slot<S: Stage>(stage: &mut S, layout: Layout<S::Word>, slot: u16) -> S::Word {
    debug_assert!(usize::from(slot) < layout.storage_len);
    let payload = match layout.slots {
        Slots::Arena(base) | Slots::Displaced(base) => return stage.load_u64(base, i32::from(slot) * OBJECT.property_value_size as i32),
        Slots::Packed(payload) => payload,
        Slots::PackedRecord(record) => stage.load_u64(record, OBJECT.payload_offset as i32),
    };
    debug_assert!(usize::from(slot) < INLINE_PROPERTY_COUNT);
    let half = stage.shr_imm(payload, u32::from(slot) * u32::BITS);
    let narrow = stage.and_imm(half, u64::from(u32::MAX));
    decode_narrow(stage, narrow)
}

/// Stores `value` at `slot` of an object of `layout`, as every tier stores an own field of a
/// proved exact shape: an arena slot takes the word itself; a packed slot takes an integer's
/// narrow word (`Value::to_narrow`) where it fits the narrow range, and an integer needs no
/// write barrier. `int` says the value is known an integer, so its tag is not tested. `None`
/// for any other packed value (a reference's narrow form, or a wide value), which the caller's
/// general store takes, the object unchanged; `miss` where a test of the value fails.
#[inline(always)]
pub(crate) fn store_slot<S: Stage>(stage: &mut S, layout: Layout<S::Word>, slot: u16, value: S::Word, int: bool, miss: S::Miss) -> Option<()> {
    debug_assert!(usize::from(slot) < layout.storage_len);
    let record = match layout.slots {
        Slots::Arena(base) => {
            stage.store_u64(base, i32::from(slot) * OBJECT.property_value_size as i32, value);
            return Some(());
        }
        // A snapshot of packed values serves only a caller no store reaches; values extras
        // displaced take the heap's store, which keeps their barrier.
        Slots::Packed(_) | Slots::Displaced(_) => return None,
        Slots::PackedRecord(record) => record,
    };
    if !int {
        let upper = stage.and_imm(value, !u64::from(u32::MAX));
        let integer = stage.eq_imm(upper, ValueLayout::INT_TAG);
        stage.guard(integer, miss)?;
    }
    // The int's low word, biased by half the narrow range, lies below the whole range.
    let half = 1_u64 << (NarrowLayout::INTEGER_BITS - 1);
    let low = stage.and_imm(value, u64::from(u32::MAX));
    let biased = stage.add_imm(low, half);
    let biased = stage.and_imm(biased, u64::from(u32::MAX));
    let fits = stage.below_imm(biased, half << 1);
    stage.guard(fits, miss)?;
    let shifted = stage.mul_imm(low, 1 << NarrowLayout::CODE_SHIFT);
    let narrow = stage.or_imm(shifted, u64::from(NarrowLayout::INTEGER_FLAG));
    let narrow = stage.and_imm(narrow, u64::from(u32::MAX));
    let offset = OBJECT.payload_offset + usize::from(slot) * std::mem::size_of::<u32>();
    stage.store_u32(record, offset as i32, narrow);
    Some(())
}

/// The value a narrow word (`Value::to_narrow`), zero-extended, encodes (`Value::from_narrow`):
/// an integer's code shifted down under the int tag, or a reference's upper word by its code
/// beside its index; only the arm the word's integer flag takes runs.
#[inline(always)]
pub(crate) fn decode_narrow<S: Stage>(stage: &mut S, narrow: S::Word) -> S::Word {
    let flag = stage.and_imm(narrow, u64::from(NarrowLayout::INTEGER_FLAG));
    let is_reference = stage.eq_imm(flag, 0);
    stage.choose(is_reference, |stage| {
        let code = stage.shr_imm(narrow, NarrowLayout::CODE_SHIFT);
        let code = stage.and_imm(code, u64::from(NarrowLayout::CODE_MASK));
        let offset = stage.mul_imm(code, std::mem::size_of::<u64>() as u64);
        let table = stage.constant(NarrowLayout::upper_words() as u64);
        let entry = stage.add(table, offset);
        let upper = stage.load_table(entry);
        let lower = stage.shr_imm(narrow, NarrowLayout::INDEX_SHIFT);
        Some(stage.or(upper, lower))
    }, |stage| {
        let shifted = stage.sar32_imm(narrow, NarrowLayout::CODE_SHIFT);
        Some(stage.or_imm(shifted, ValueLayout::INT_TAG))
    }).expect("both arms of a narrow decode are total")
}

/// The record of the direct array `bits`; `miss` for any other value, or before any array
/// record is reserved.
#[inline(always)]
pub(crate) fn array_record<S: Stage>(stage: &mut S, bits: S::Word, miss: S::Miss) -> Option<S::Word> {
    use crate::heap::NATIVE_ARRAY_LAYOUT as ARRAY;
    let family = stage.and_imm(bits, ValueLayout::HEAP_FAMILY_MASK);
    let array = stage.eq_imm(family, ValueLayout::DIRECT_ARRAY_TAG);
    stage.guard(array, miss)?;
    let records = stage.table(Table::ArrayRecords);
    let reserved = stage.ne_imm(records, 0);
    stage.guard(reserved, miss)?;
    // A direct array's low word is its record's id.
    let id = stage.and_imm(bits, u64::from(u32::MAX));
    let offset = stage.mul_imm(id, ARRAY.record_size as u64);
    Some(stage.add(records, offset))
}

/// The present element `index` (unsigned) of the dense elements of the array record at
/// `record`, through its current backing; `miss` past their end or at a hole.
#[inline(always)]
pub(crate) fn dense_element<S: Stage>(stage: &mut S, record: S::Word, index: S::Word, miss: S::Miss) -> Option<S::Word> {
    use crate::heap::NATIVE_ARRAY_LAYOUT as ARRAY;
    let header = stage.load_u64(record, (ARRAY.elements_offset + ARRAY.elements_header_offset) as i32);
    let length = stage.load_u32(header, ARRAY.header_length_offset as i32);
    let within = stage.below(index, length);
    stage.guard(within, miss)?;
    let offset = stage.mul_imm(index, std::mem::size_of::<u64>() as u64);
    let values = stage.add_imm(header, ARRAY.header_values_offset as u64);
    let address = stage.add(values, offset);
    let value = stage.load_u64(address, 0);
    let present = stage.ne_imm(value, crate::Value::DELETED.to_bits());
    stage.guard(present, miss)?;
    Some(value)
}

/// Whether a binding's word is a value the program reads, not an internal marker (an
/// uninitialized binding's, a hole's).
#[inline(always)]
pub(crate) fn bound<S: Stage>(stage: &mut S, value: S::Word) -> S::Cond {
    let tag = stage.and_imm(value, ValueLayout::TAG_MASK);
    stage.ne_imm(tag, ValueLayout::INTERNAL_TAG)
}

/// `bound` of a value the interpreter holds.
#[inline(always)]
pub(crate) fn is_bound(value: crate::Value) -> bool {
    let mut stage = Concrete::DETACHED;
    bound(&mut stage, value.to_bits())
}

/// Capture `slot` of an environment whose dense view holds `len` values at `values`
/// (`EnvironmentSlots::native_dense_view`); `miss` where the view does not hold it: past its
/// end, or not `bound`.
#[inline(always)]
pub(crate) fn dense_capture<S: Stage>(stage: &mut S, values: S::Word, len: S::Word, slot: u16, miss: S::Miss) -> Option<S::Word> {
    let held = stage.above_imm(len, u64::from(slot));
    stage.guard(held, miss)?;
    let value = stage.load_u64(values, i32::from(slot) * std::mem::size_of::<u64>() as i32);
    let bound = bound(stage, value);
    stage.guard(bound, miss)?;
    Some(value)
}

/// The interpreter's stage: words are the machine's own, and a failed guard returns to the
/// caller's slow path. A table's base is read where an operation addresses it.
pub(crate) struct Concrete {
    heap: *const crate::heap::Heap,
    arena: *const crate::value_vec::ValueArena,
}

impl Concrete {
    /// The stage of an operation that touches no object storage (a number operation).
    pub(crate) const DETACHED: Self = Self { heap: std::ptr::null(), arena: std::ptr::null() };

    /// The stage over `heap`'s storage.
    #[inline(always)]
    pub(crate) fn of(heap: &crate::heap::Heap) -> Self {
        Self { heap, arena: heap.property_arena() }
    }

    /// The stage over the property arena `arena` alone.
    #[inline(always)]
    pub(crate) fn over_arena(arena: *const crate::value_vec::ValueArena) -> Self {
        Self { arena, ..Self::DETACHED }
    }
}

impl Stage for Concrete {
    type Word = u64;
    type Cond = bool;
    type Miss = ();
    #[inline(always)]
    fn constant(&mut self, bits: u64) -> u64 { bits }
    #[inline(always)]
    fn load_u32(&mut self, base: u64, offset: i32) -> u64 {
        // SAFETY: an operation loads only the layout offsets of a live record it was given.
        u64::from(unsafe { ((base as usize).wrapping_add_signed(offset as isize) as *const u32).read() })
    }
    #[inline(always)]
    fn load_u64(&mut self, base: u64, offset: i32) -> u64 {
        // SAFETY: as `load_u32`, of a word-aligned field or value.
        unsafe { ((base as usize).wrapping_add_signed(offset as isize) as *const u64).read() }
    }
    #[inline(always)]
    fn load_table(&mut self, address: u64) -> u64 { self.load_u64(address, 0) }
    #[inline(always)]
    fn store_u32(&mut self, base: u64, offset: i32, value: u64) {
        // SAFETY: as `load_u32`, into a record or value the caller owns exclusively.
        unsafe { ((base as usize).wrapping_add_signed(offset as isize) as *mut u32).write(value as u32) }
    }
    #[inline(always)]
    fn store_u64(&mut self, base: u64, offset: i32, value: u64) {
        // SAFETY: as `store_u32`.
        unsafe { ((base as usize).wrapping_add_signed(offset as isize) as *mut u64).write(value) }
    }
    #[inline(always)]
    fn add(&mut self, a: u64, b: u64) -> u64 { a.wrapping_add(b) }
    #[inline(always)]
    fn or(&mut self, a: u64, b: u64) -> u64 { a | b }
    #[inline(always)]
    fn add_imm(&mut self, a: u64, imm: u64) -> u64 { a.wrapping_add(imm) }
    #[inline(always)]
    fn mul_imm(&mut self, a: u64, imm: u64) -> u64 { a.wrapping_mul(imm) }
    #[inline(always)]
    fn and_imm(&mut self, a: u64, imm: u64) -> u64 { a & imm }
    #[inline(always)]
    fn or_imm(&mut self, a: u64, imm: u64) -> u64 { a | imm }
    #[inline(always)]
    fn shr_imm(&mut self, a: u64, shift: u32) -> u64 { a >> shift }
    #[inline(always)]
    fn sar32_imm(&mut self, a: u64, shift: u32) -> u64 { u64::from(((a as u32 as i32) >> shift) as u32) }
    #[inline(always)]
    fn eq_imm(&mut self, a: u64, imm: u64) -> bool { a == imm }
    #[inline(always)]
    fn below_imm(&mut self, a: u64, imm: u64) -> bool { a < imm }
    #[inline(always)]
    fn ne_imm(&mut self, a: u64, imm: u64) -> bool { a != imm }
    #[inline(always)]
    fn above_imm(&mut self, a: u64, imm: u64) -> bool { a > imm }
    #[inline(always)]
    fn below(&mut self, a: u64, b: u64) -> bool { a < b }
    #[inline(always)]
    fn choose(&mut self, cond: bool, then: impl FnOnce(&mut Self) -> Option<u64>, otherwise: impl FnOnce(&mut Self) -> Option<u64>) -> Option<u64> {
        if cond { then(self) } else { otherwise(self) }
    }
    #[inline(always)]
    fn either(&mut self, a: bool, b: bool) -> bool { a || b }
    #[inline(always)]
    fn guard(&mut self, cond: bool, _: ()) -> Option<()> { cond.then_some(()) }
    #[inline(always)]
    fn table(&mut self, table: Table) -> u64 {
        // SAFETY: the stage lives within the operation, while its heap and arena outlive it.
        unsafe {
            match table {
                Table::PropertyValues => (*self.arena).native_values().0 as u64,
                Table::ObjectRecords => (*self.heap).object_record_base().map_or(0, |base| base as u64),
                Table::ArrayRecords => (*self.heap).array_record_base().map_or(0, |base| base as u64),
            }
        }
    }
}

/// The own data value `slot` of the object `object` where its exact shape is `exact`, as every
/// tier reads it (`exact_shape_is`, `layout`, `read_slot`); `None` where the shape differs.
#[inline(always)]
pub(crate) fn own_field(heap: &crate::heap::Heap, object: &crate::heap::Object, exact: u32, slot: u16, storage_len: usize) -> Option<crate::Value> {
    let mut stage = Concrete::of(heap);
    let matched = exact_shape_is(&mut stage, object as *const crate::heap::Object as u64, exact);
    stage.guard(matched, ())?;
    Some(own_slot(heap, object, exact, slot, storage_len))
}

/// `own_field` of an object whose exact shape the caller proved `exact`.
#[inline(always)]
pub(crate) fn own_slot(heap: &crate::heap::Heap, object: &crate::heap::Object, exact: u32, slot: u16, storage_len: usize) -> crate::Value {
    let mut stage = Concrete::of(heap);
    let record = object as *const crate::heap::Object as u64;
    let layout = layout(&mut stage, record, exact, storage_len, false);
    crate::Value::from_bits(read_slot(&mut stage, layout, slot))
}

/// The data value `slot` that the immediate prototype of `object` holds, as every tier reads
/// it (`holder_slot`), where `object`'s exact shape is `exact` and the prototype is an object
/// record of the exact shape `holder`; `None` otherwise.
#[inline(always)]
pub(crate) fn inherited_field(heap: &crate::heap::Heap, object: &crate::heap::Object, exact: u32, holder: u32, slot: u16) -> Option<crate::Value> {
    let mut stage = Concrete::of(heap);
    let record = object as *const crate::heap::Object as u64;
    let matched = exact_shape_is(&mut stage, record, exact);
    stage.guard(matched, ())?;
    holder_slot(&mut stage, record, holder, slot, usize::from(slot) + 1, ()).map(crate::Value::from_bits)
}

/// `store_slot` of `value` at `slot` of `object`, of the exact shape `exact`, into the property
/// arena `arena`, as the interpreter stores a proved own field; `false` where the shared store
/// does not take it (`store_slot`), the object unchanged.
#[inline(always)]
pub(crate) fn store_own_field(arena: &mut crate::value_vec::ValueArena, object: &mut crate::heap::Object, exact: u32, slot: u16, value: crate::Value) -> bool {
    let mut stage = Concrete::over_arena(arena);
    let record = object as *mut crate::heap::Object as u64;
    let layout = layout(&mut stage, record, exact, usize::from(slot) + 1, false);
    store_slot(&mut stage, layout, slot, value.to_bits(), false, ()).is_some()
}

/// A stage with a register file: the frame an instruction's semantics reads and writes. The
/// interpreter's is the activation's registers; the specializer's is its register state, whose
/// facts (a value known, a representation proved) it keeps and folds on.
pub(crate) trait Machine: Numbers {
    /// The value of `register`, as a tagged word.
    fn register(&mut self, register: crate::bytecode::Register) -> Self::Word;
    /// Writes the tagged word `value` to `register`.
    fn set_register(&mut self, register: crate::bytecode::Register, value: Self::Word);
    /// Writes `value`, which the semantics knows, to `register`: the interpreter stores its
    /// bits, the specializer takes it as a fact and folds on it.
    fn set_static(&mut self, register: crate::bytecode::Register, value: crate::Value);
    /// ToBoolean of `register`'s value where no Number fact decides it (`to_boolean`); `miss`
    /// where the machine does not decide it here.
    fn value_truthy(&mut self, register: crate::bytecode::Register, miss: Self::Miss) -> Option<Self::Cond>;
    /// Copies `source` to `destination`, with everything known of it.
    fn copy_register(&mut self, destination: crate::bytecode::Register, source: crate::bytecode::Register) {
        let value = self.register(source);
        self.set_register(destination, value);
    }
}

/// A quickened record's guess about its number operation's operands: nothing (any values),
/// int32s, or Numbers. The semantics specializes to it as the specializer does to a proof, and
/// misses where it is wrong.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Guess {
    Values,
    Int32s,
    Numbers,
}

/// The interpreter's register file, as words: the activation's registers and whatever the
/// interpreter keeps of them outside memory (`dispatch_fast`'s accumulator).
pub(crate) trait RegisterFile {
    /// What the interpreter that holds the file guessed of a number operation's operands when
    /// it quickened the record, which the semantics then specializes to.
    const GUESS: Guess = Guess::Values;
    fn get(&mut self, register: crate::bytecode::Register) -> u64;
    fn set(&mut self, register: crate::bytecode::Register, value: u64);
}

/// The interpreter's machine: the concrete stage over a register file.
pub(crate) struct ConcreteFrame<R> {
    pub(crate) stage: Concrete,
    pub(crate) registers: R,
}

impl<R: RegisterFile> ConcreteFrame<R> {
    #[inline(always)]
    fn value(&mut self, register: crate::bytecode::Register) -> crate::Value { crate::Value::from_bits(self.registers.get(register)) }
}

macro_rules! delegate_concrete {
    ($($name:ident($($arg:ident: $ty:ty),*) $(-> $ret:ty)?;)*) => {
        $(#[inline(always)] fn $name(&mut self, $($arg: $ty),*) $(-> $ret)? { self.stage.$name($($arg),*) })*
    };
}

impl<R: RegisterFile> Stage for ConcreteFrame<R> {
    type Word = u64;
    type Cond = bool;
    type Miss = ();
    delegate_concrete! {
        constant(bits: u64) -> u64;
        load_u32(base: u64, offset: i32) -> u64;
        load_u64(base: u64, offset: i32) -> u64;
        store_u32(base: u64, offset: i32, value: u64);
        store_u64(base: u64, offset: i32, value: u64);
        load_table(address: u64) -> u64;
        add(a: u64, b: u64) -> u64;
        or(a: u64, b: u64) -> u64;
        add_imm(a: u64, imm: u64) -> u64;
        mul_imm(a: u64, imm: u64) -> u64;
        and_imm(a: u64, imm: u64) -> u64;
        or_imm(a: u64, imm: u64) -> u64;
        shr_imm(a: u64, shift: u32) -> u64;
        sar32_imm(a: u64, shift: u32) -> u64;
        eq_imm(a: u64, imm: u64) -> bool;
        below_imm(a: u64, imm: u64) -> bool;
        ne_imm(a: u64, imm: u64) -> bool;
        above_imm(a: u64, imm: u64) -> bool;
        below(a: u64, b: u64) -> bool;
        guard(cond: bool, miss: ()) -> Option<()>;
        table(table: Table) -> u64;
    }
    #[inline(always)]
    fn choose(&mut self, cond: bool, then: impl FnOnce(&mut Self) -> Option<u64>, otherwise: impl FnOnce(&mut Self) -> Option<u64>) -> Option<u64> {
        if cond { then(self) } else { otherwise(self) }
    }
    #[inline(always)]
    fn either(&mut self, a: bool, b: bool) -> bool { a || b }
}

impl<R: RegisterFile> Machine for ConcreteFrame<R> {
    /// ToBoolean of an immediate (`Value::truthiness`); a heap value's is the VM's to read.
    #[inline(always)]
    fn value_truthy(&mut self, register: crate::bytecode::Register, _: ()) -> Option<bool> { self.value(register).truthiness() }
    #[inline(always)]
    fn register(&mut self, register: crate::bytecode::Register) -> u64 { self.registers.get(register) }
    #[inline(always)]
    fn set_register(&mut self, register: crate::bytecode::Register, value: u64) { self.registers.set(register, value) }
    #[inline(always)]
    fn set_static(&mut self, register: crate::bytecode::Register, value: crate::Value) { self.registers.set(register, value.to_bits()) }
}

/// An instruction's meaning as data: what `step` interprets, decoded once from whichever
/// record carries it (a bytecode instruction, a lane record). Operands the instruction names
/// statically (registers, constants) are plain values, which every stage folds.
#[derive(Clone, Copy)]
pub(crate) enum Operation {
    /// `destination = source`.
    Move { destination: crate::bytecode::Register, source: crate::bytecode::Register },
    /// `destination = constant`, a value that needs no heap cell (`facts::immediate`).
    LoadConst { destination: crate::bytecode::Register, constant: crate::Value },
    /// `destination = left operator right` on Numbers.
    Binary { operator: NumberOperator, destination: crate::bytecode::Register, left: Source, right: Source },
    /// Continues at `target`.
    Jump { target: u32 },
    /// Continues at `target` where ToBoolean of `register` is `when`.
    JumpIf { register: crate::bytecode::Register, when: bool, target: u32 },
    /// Continues at `target` unless Number `relation` holds of `left` and `right`.
    JumpUnless { relation: super::operations::NumberRelation, left: Source, right: Source, target: u32 },
}

/// Where control goes after `step`, `C` the machine's condition.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Flow<C> {
    /// The next instruction.
    Next,
    /// The instruction at the target.
    Jump(u32),
    /// The instruction at `target` where `taken` holds, else the next.
    Branch { taken: C, target: u32 },
}

/// ToBoolean of `register` (ECMA-262 7.1.2): a proved int32 is true unless zero, a held
/// double unless zero or NaN (`operations::number_truthiness`); the machine decides any other.
#[inline(always)]
fn to_boolean<M: Machine>(machine: &mut M, register: crate::bytecode::Register, miss: M::Miss) -> Option<M::Cond> {
    use super::operations::NumberRelation;
    if machine.known_int(register) {
        let int = machine.int_register(register, miss)?;
        let zero = machine.int_constant(0);
        return Some(machine.int_relation(NumberRelation::NotEqual, int, zero));
    }
    if !machine.known_float(register) {
        return machine.value_truthy(register, miss);
    }
    struct Truthiness<'m, M>(&'m mut M);
    impl<M: Numbers> super::operations::NumberTruthinessStage<M::Float> for Truthiness<'_, M> {
        type Predicate = M::Cond;
        fn zero(&mut self) -> M::Float { self.0.float_constant(0.0) }
        fn equal(&mut self, left: M::Float, right: M::Float) -> M::Cond { self.0.float_relation(NumberRelation::Equal, left, right) }
        fn not_equal(&mut self, left: M::Float, right: M::Float) -> M::Cond { self.0.float_relation(NumberRelation::NotEqual, left, right) }
        fn and(&mut self, left: M::Cond, right: M::Cond) -> M::Cond { self.0.and_cond(left, right) }
    }
    let number = machine.float_register(register, miss)?;
    Some(super::operations::number_truthiness(&mut Truthiness(machine), number))
}

/// The interpreter: each operation's semantics, over any machine (`Machine`). The lane runs
/// it on the activation's registers; the specializer runs it on its register state, which
/// emits the residual (Futamura's first projection), and `step` for the emitting machine is
/// the compiler (the second).
/// `miss` where the operation's operands are not of the kind the machine's semantics takes.
#[inline(always)]
pub(crate) fn step<M: Machine>(machine: &mut M, operation: Operation, miss: M::Miss) -> Option<Flow<M::Cond>> {
    match operation {
        Operation::Move { destination, source } => machine.copy_register(destination, source),
        Operation::LoadConst { destination, constant } => machine.set_static(destination, constant),
        Operation::Binary { operator, destination, left, right } => binary(machine, operator, destination, left, right, miss)?,
        Operation::Jump { target } => return Some(Flow::Jump(target)),
        Operation::JumpIf { register, when, target } => {
            let truthy = to_boolean(machine, register, miss)?;
            let taken = if when { truthy } else { machine.not_cond(truthy) };
            return Some(Flow::Branch { taken, target });
        }
        Operation::JumpUnless { relation: operator, left, right, target } => {
            let holds = relation(machine, operator, left, right, miss)?;
            let taken = machine.not_cond(holds);
            return Some(Flow::Branch { taken, target });
        }
    }
    Some(Flow::Next)
}
