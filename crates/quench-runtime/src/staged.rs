//! Operation semantics written once, over a binding-time domain (`Stage`).
//!
//! This is Futamura's first projection made literal through Ershov's mixed computation. Each
//! operation below is one Rust function, generic over the stage that runs it:
//!
//! - The interpreter runs it on machine words (`Concrete`). Every condition is known there, so
//!   only the arm taken runs.
//! - A specializer runs it on IR values, which emits the residual code (`Emit` in
//!   `vm/compiled/object_slots.rs`). A condition it cannot know becomes a guard, or both arms.
//!
//! A plain Rust argument (a slot, a storage form a site proved) is static, so a branch on it
//! leaves no code. A rule written here is the rule of every tier, which is what makes the
//! native tiers the interpreter specialized, not a second implementation of it
//! (`docs/v2/specialization.md`). `staged_coverage` in this module's tests keeps hand-written
//! decodes of the same layouts from reappearing elsewhere.

use crate::heap::{NATIVE_OBJECT_LAYOUT as OBJECT, PlainPropertyStorage};
use crate::value::{NarrowLayout, ValueLayout};

/// A binding-time domain: the 64-bit words an operation computes on, its conditions, and where
/// a failed guard goes. A 32-bit load zero-extends.
pub(crate) trait Stage {
    type Word: Copy;
    type Cond: Copy;
    /// Where a failed guard leaves: nowhere for the interpreter, which knows the value; a
    /// native exit for a specializer.
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
    fn ne_imm(&mut self, a: Self::Word, imm: u64) -> Self::Cond;
    fn not(&mut self, cond: Self::Cond) -> Self::Cond;
    /// The word `then` computes where `cond` holds, else `otherwise`'s.
    fn choose(&mut self, cond: Self::Cond, then: impl FnOnce(&mut Self) -> Self::Word,
        otherwise: impl FnOnce(&mut Self) -> Self::Word) -> Self::Word;
    /// Continues where `cond` holds; `None` where it does not, so nothing runs past it.
    fn guard(&mut self, cond: Self::Cond, miss: Self::Miss) -> Option<()>;
    /// `cond`'s value where this stage knows it: the interpreter always does, a specializer
    /// never does for a value read from the heap.
    fn known(&self, cond: Self::Cond) -> Option<bool>;
    /// The base of `table`.
    fn table(&mut self, table: Table) -> Self::Word;
}

/// The process tables an operation addresses from a base.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Table {
    /// The property arena's values, where a flat object's run starts at its position.
    PropertyValues,
}

impl Table {
    pub(crate) const COUNT: usize = 1;
}

/// What a site proved about where an object of an exact shape keeps its values. It is a
/// static input: a specializer reads no word to learn it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Form {
    /// Packed narrow values in the payload word (`PlainPropertyStorage::PackedInline`).
    Packed,
    /// A flat run in the property arena (`PlainPropertyStorage::Arena`).
    Arena,
    /// Either, as the record's inline flag says at run time.
    Either,
}

impl Form {
    /// The form a site's observed storage and its shape's length leave possible: only a shape
    /// that fits the payload word can be packed.
    pub(crate) fn of(storage: Option<PlainPropertyStorage>, storage_len: usize) -> Self {
        match storage {
            Some(PlainPropertyStorage::PackedInline) => Self::Packed,
            Some(PlainPropertyStorage::Arena) => Self::Arena,
            None if storage_len <= OBJECT.inline_property_count => Self::Either,
            None => Self::Arena,
        }
    }
}

/// Whether packed values are read once, for a caller no store reaches, or at each read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PackedAccess {
    Snapshot,
    Reload,
}

/// Packed narrow values: the payload word as read once, or the record that holds it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Packed<W> {
    Snapshot(W),
    Record(W),
}

/// Where an object of a proved exact shape keeps its named values.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Slots<W, C> {
    Packed(Packed<W>),
    /// The address of value zero of a flat run of wide values: the arena's, or the inline
    /// values extras displaced.
    Flat(W),
    /// Packed or flat, as `inline` says; `flat` is the run's address where it is flat.
    Either { packed: Packed<W>, inline: C, flat: W },
}

/// The layout of one object record and how many values its shape holds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Layout<W, C> {
    pub(crate) slots: Slots<W, C>,
    pub(crate) storage_len: usize,
}

/// The record's inline-storage flag (`Object::meta`), true where its values are inline.
#[inline(always)]
pub(crate) fn inline_storage<S: Stage>(stage: &mut S, record: S::Word) -> S::Cond {
    let meta = stage.load_u32(record, OBJECT.meta_offset as i32);
    let inline = stage.and_imm(meta, u64::from(OBJECT.inline_storage_mask));
    stage.ne_imm(inline, 0)
}

/// The address of the first arena value of the run starting at `start`.
#[inline(always)]
fn arena_run<S: Stage>(stage: &mut S, start: S::Word) -> S::Word {
    let start = stage.and_imm(start, u64::from(OBJECT.storage_start_mask));
    let offset = stage.mul_imm(start, OBJECT.property_value_size as u64);
    let base = stage.table(Table::PropertyValues);
    stage.add(base, offset)
}

/// The layout of the object record at `record`, whose exact shape the caller proved and which
/// holds `storage_len` values in `form`; `inline` is its `inline_storage`.
///
/// An object whose values extras displaced is read where the stage knows it has extras (the
/// interpreter); a specializer leaves through `miss` for it, so its residual handles the two
/// common forms only. A form the site proved is guarded, not decoded.
#[inline(always)]
pub(crate) fn own_layout<S: Stage>(stage: &mut S, record: S::Word, inline: S::Cond, form: Form,
    storage_len: usize, access: PackedAccess, miss: S::Miss) -> Option<Layout<S::Word, S::Cond>> {
    let payload = stage.load_u64(record, OBJECT.payload_offset as i32);
    let tag = stage.and_imm(payload, OBJECT.extras_tag_mask);
    let extras = stage.eq_imm(tag, OBJECT.extras_tag);
    if stage.known(extras) == Some(true) {
        return Some(Layout { slots: extras_slots(stage, payload, inline), storage_len });
    }
    let plain = stage.ne_imm(tag, OBJECT.extras_tag);
    stage.guard(plain, miss)?;
    let packed = match access {
        PackedAccess::Snapshot => Packed::Snapshot(payload),
        PackedAccess::Reload => Packed::Record(record),
    };
    Some(Layout { slots: plain_slots(stage, payload, packed, inline, form, miss)?, storage_len })
}

/// The slots of an object whose payload word points at its extras, which hold the inline
/// values wide or the run's start (`Object::extras_storage_start`).
fn extras_slots<S: Stage>(stage: &mut S, payload: S::Word, inline: S::Cond) -> Slots<S::Word, S::Cond> {
    let pointer = stage.and_imm(payload, !OBJECT.extras_tag_mask);
    if stage.known(inline) == Some(true) {
        Slots::Flat(stage.add_imm(pointer, OBJECT.extras_inline_values_offset as u64))
    } else {
        let start = stage.load_u32(pointer, OBJECT.extras_arena_start_offset as i32);
        Slots::Flat(arena_run(stage, start))
    }
}

/// The slots of a plain object in `form`: packed in its payload or a run in the arena, the
/// inline flag decided by the stage, guarded, or left to the read.
fn plain_slots<S: Stage>(stage: &mut S, payload: S::Word, packed: Packed<S::Word>, inline: S::Cond,
    form: Form, miss: S::Miss) -> Option<Slots<S::Word, S::Cond>> {
    let flat = match stage.known(inline) {
        Some(inline) => !inline,
        None => match form {
            Form::Packed => { stage.guard(inline, miss)?; false }
            Form::Arena => {
                let flat = stage.not(inline);
                stage.guard(flat, miss)?;
                true
            }
            Form::Either => {
                // The run is addressed on the flat side of the flag only.
                let flat = stage.choose(inline, |stage| stage.constant(0), |stage| arena_run(stage, payload));
                return Some(Slots::Either { packed, inline, flat });
            }
        },
    };
    Some(if flat { Slots::Flat(arena_run(stage, payload)) } else { Slots::Packed(packed) })
}

/// Value `slot` of an object of `layout`, which holds it.
#[inline(always)]
pub(crate) fn read_slot<S: Stage>(stage: &mut S, layout: Layout<S::Word, S::Cond>, slot: u16) -> S::Word {
    debug_assert!(usize::from(slot) < layout.storage_len);
    let offset = i32::from(slot) * OBJECT.property_value_size as i32;
    match layout.slots {
        Slots::Flat(base) => stage.load_u64(base, offset),
        Slots::Packed(packed) => packed_read(stage, packed, slot),
        Slots::Either { packed, inline, flat } => stage.choose(inline,
            |stage| packed_read(stage, packed, slot), |stage| stage.load_u64(flat, offset)),
    }
}

/// Packed value `slot`: its half of the payload word, decoded.
#[inline(always)]
fn packed_read<S: Stage>(stage: &mut S, packed: Packed<S::Word>, slot: u16) -> S::Word {
    debug_assert!(usize::from(slot) < OBJECT.inline_property_count);
    let payload = match packed {
        Packed::Snapshot(payload) => payload,
        Packed::Record(record) => stage.load_u64(record, OBJECT.payload_offset as i32),
    };
    let half = stage.shr_imm(payload, u32::from(slot) * u32::BITS);
    let narrow = stage.and_imm(half, u64::from(u32::MAX));
    decode_narrow(stage, narrow)
}

/// The value a narrow word (`Value::to_narrow`), zero-extended, encodes (`Value::from_narrow`):
/// an integer's code shifted down under the int tag, or a reference's upper word by its code
/// beside its index.
#[inline(always)]
pub(crate) fn decode_narrow<S: Stage>(stage: &mut S, narrow: S::Word) -> S::Word {
    let flag = stage.and_imm(narrow, u64::from(NarrowLayout::INTEGER_FLAG));
    let integer = stage.ne_imm(flag, 0);
    stage.choose(integer, |stage| {
        let shifted = stage.sar32_imm(narrow, NarrowLayout::CODE_SHIFT);
        stage.or_imm(shifted, ValueLayout::INT_TAG)
    }, |stage| {
        let code = stage.shr_imm(narrow, NarrowLayout::CODE_SHIFT);
        let code = stage.and_imm(code, u64::from(NarrowLayout::CODE_MASK));
        let offset = stage.mul_imm(code, std::mem::size_of::<u64>() as u64);
        let table = stage.constant(NarrowLayout::upper_words() as u64);
        let entry = stage.add(table, offset);
        let upper = stage.load_table(entry);
        let lower = stage.shr_imm(narrow, NarrowLayout::INDEX_SHIFT);
        stage.or(upper, lower)
    })
}

/// The interpreter's stage: machine words, read from live memory. It knows every condition,
/// so a guard never fails and each `choose` runs one arm.
#[derive(Clone, Copy)]
pub(crate) struct Concrete {
    tables: [u64; Table::COUNT],
}

impl Concrete {
    pub(crate) fn new(property_values: *const crate::Value) -> Self {
        Self { tables: [property_values as u64] }
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
        // SAFETY: a staged operation loads only from a live record or table at a layout offset.
        u64::from(unsafe { ((base as *const u8).offset(offset as isize) as *const u32).read() })
    }
    #[inline(always)]
    fn load_u64(&mut self, base: u64, offset: i32) -> u64 {
        // SAFETY: as `load_u32`; records and arena values are 8-byte aligned.
        unsafe { ((base as *const u8).offset(offset as isize) as *const u64).read() }
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
    fn ne_imm(&mut self, a: u64, imm: u64) -> bool { a != imm }
    #[inline(always)]
    fn not(&mut self, cond: bool) -> bool { !cond }
    #[inline(always)]
    fn choose(&mut self, cond: bool, then: impl FnOnce(&mut Self) -> u64, otherwise: impl FnOnce(&mut Self) -> u64) -> u64 {
        if cond { then(self) } else { otherwise(self) }
    }
    #[inline(always)]
    fn guard(&mut self, cond: bool, _miss: ()) -> Option<()> { cond.then_some(()) }
    #[inline(always)]
    fn known(&self, cond: bool) -> Option<bool> { Some(cond) }
    #[inline(always)]
    fn table(&mut self, table: Table) -> u64 { self.tables[table as usize] }
}

/// The value at `slot` of the live object `object`, whose exact shape (no dictionary) holds
/// it, through the arena whose values start at `property_values`: the interpreter's reading
/// of `own_layout` and `read_slot`.
///
/// # Safety
/// `object` is a live record whose exact shape covers `slot`, and `property_values` is the
/// base of the arena that holds its run.
#[inline(always)]
pub(crate) unsafe fn concrete_own_read(object: &crate::heap::Object, property_values: *const crate::Value, slot: usize) -> crate::Value {
    let mut stage = Concrete::new(property_values);
    let record = object as *const crate::heap::Object as u64;
    let inline = inline_storage(&mut stage, record);
    let layout = own_layout(&mut stage, record, inline, Form::Either, slot + 1, PackedAccess::Reload, ())
        .expect("the interpreter knows every storage form");
    crate::Value::from_bits(read_slot(&mut stage, layout, slot as u16))
}

#[cfg(test)]
mod tests;
