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
    fn select(&mut self, cond: Self::Cond, then: Self::Word, otherwise: Self::Word) -> Self::Word;
    /// Continues where `cond` holds; `None` where it never can, so nothing runs past it.
    fn guard(&mut self, cond: Self::Cond, miss: Self::Miss) -> Option<()>;
    /// The property arena's base, whose values a flat object's run starts at its position.
    fn property_base(&mut self) -> Self::Word;
}

/// Where an object of a proved exact shape keeps its named values: the payload word that packs
/// them, read at once or at each read, or the address of its arena run.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Slots<W> {
    Packed(W),
    PackedRecord(W),
    Arena(W),
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
/// here, for a caller no store reaches; else each read loads them again. `None` for values
/// behind extras, which the VM's own read takes.
#[inline(always)]
pub(crate) fn layout<S: Stage>(stage: &mut S, record: S::Word, exact: u32, storage_len: usize, snapshot: bool) -> Option<Layout<S::Word>> {
    let slots = match PlainPropertyStorage::of_exact(exact)? {
        PlainPropertyStorage::PackedInline if snapshot => Slots::Packed(stage.load_u64(record, OBJECT.payload_offset as i32)),
        PlainPropertyStorage::PackedInline => Slots::PackedRecord(record),
        PlainPropertyStorage::Arena => {
            // A flat object's payload word is its run's position alone (`Object::plain_storage_start`).
            let start = stage.load_u64(record, OBJECT.payload_offset as i32);
            let offset = stage.mul_imm(start, OBJECT.property_value_size as u64);
            let base = stage.property_base();
            Slots::Arena(stage.add(base, offset))
        }
    };
    Some(Layout { slots, storage_len })
}

/// Value `slot` of an object of `layout`, which holds it.
#[inline(always)]
pub(crate) fn read_slot<S: Stage>(stage: &mut S, layout: Layout<S::Word>, slot: u16) -> S::Word {
    debug_assert!(usize::from(slot) < layout.storage_len);
    let payload = match layout.slots {
        Slots::Arena(base) => return stage.load_u64(base, i32::from(slot) * OBJECT.property_value_size as i32),
        Slots::Packed(payload) => payload,
        Slots::PackedRecord(record) => stage.load_u64(record, OBJECT.payload_offset as i32),
    };
    debug_assert!(usize::from(slot) < INLINE_PROPERTY_COUNT);
    let half = stage.shr_imm(payload, u32::from(slot) * u32::BITS);
    let narrow = stage.and_imm(half, u64::from(u32::MAX));
    decode_narrow(stage, narrow)
}

/// The address of value `slot` of an object of `layout` in the arena, where a flat store
/// writes it; `None` for packed values, which a store narrows itself.
#[inline(always)]
pub(crate) fn arena_slot_address<S: Stage>(stage: &mut S, layout: Layout<S::Word>, slot: u16) -> Option<S::Word> {
    debug_assert!(usize::from(slot) < layout.storage_len);
    let Slots::Arena(base) = layout.slots else { return None };
    Some(stage.add_imm(base, u64::from(slot) * OBJECT.property_value_size as u64))
}

/// The value a narrow word (`Value::to_narrow`), zero-extended, encodes (`Value::from_narrow`):
/// an integer's code shifted down under the int tag, or a reference's upper word by its code
/// beside its index. Both are computed and one selected, so the decode takes no branch.
#[inline(always)]
pub(crate) fn decode_narrow<S: Stage>(stage: &mut S, narrow: S::Word) -> S::Word {
    let shifted = stage.sar32_imm(narrow, NarrowLayout::CODE_SHIFT);
    let integer = stage.or_imm(shifted, ValueLayout::INT_TAG);
    let code = stage.shr_imm(narrow, NarrowLayout::CODE_SHIFT);
    let code = stage.and_imm(code, u64::from(NarrowLayout::CODE_MASK));
    let offset = stage.mul_imm(code, std::mem::size_of::<u64>() as u64);
    let table = stage.constant(NarrowLayout::upper_words() as u64);
    let entry = stage.add(table, offset);
    let upper = stage.load_table(entry);
    let lower = stage.shr_imm(narrow, NarrowLayout::INDEX_SHIFT);
    let reference = stage.or(upper, lower);
    let flag = stage.and_imm(narrow, u64::from(NarrowLayout::INTEGER_FLAG));
    let is_reference = stage.eq_imm(flag, 0);
    stage.select(is_reference, reference, integer)
}

/// The interpreter's stage: words are the machine's own, and a failed guard returns to the
/// caller's slow path.
pub(crate) struct Concrete {
    pub(crate) property_base: u64,
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
    fn select(&mut self, cond: bool, then: u64, otherwise: u64) -> u64 { if cond { then } else { otherwise } }
    #[inline(always)]
    fn guard(&mut self, cond: bool, _: ()) -> Option<()> { cond.then_some(()) }
    #[inline(always)]
    fn property_base(&mut self) -> u64 { self.property_base }
}

/// The own data value `slot` of the object `object` where its exact shape is `exact`, as every
/// tier reads it (`exact_shape_is`, `layout`, `read_slot`); `None` where the shape differs or
/// its values sit behind extras, which the caller's general read takes.
#[inline(always)]
pub(crate) fn own_field(heap: &crate::heap::Heap, object: &crate::heap::Object, exact: u32, slot: u16, storage_len: usize) -> Option<crate::Value> {
    let mut stage = Concrete { property_base: heap.native_property_values().0 as u64 };
    let record = object as *const crate::heap::Object as u64;
    let matched = exact_shape_is(&mut stage, record, exact);
    stage.guard(matched, ())?;
    let layout = layout(&mut stage, record, exact, storage_len, false)?;
    Some(crate::Value::from_bits(read_slot(&mut stage, layout, slot)))
}
