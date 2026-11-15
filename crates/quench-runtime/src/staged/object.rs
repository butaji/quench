//! Object storage layouts as staged reads: where a record of a proved exact shape keeps its
//! named values, and how a narrow value decodes. The interpreter reads them on `Concrete`; a
//! specializer emits them.

use super::{Concrete, Stage, Table};
use super::region::Region;
use crate::heap::{NATIVE_OBJECT_LAYOUT as OBJECT, PlainPropertyStorage};
use crate::value::{NarrowLayout, ValueLayout};

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
    #[allow(dead_code)]
    Snapshot,
    Reload,
}

/// Packed narrow values: the payload word as read once, or the record that holds it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Packed<W> {
    #[allow(dead_code)]
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
    /// The address of word zero of a narrow run (`value_vec::NARROW_FORM`): value `i` is the
    /// narrow word at byte `4 * i` (little-endian halves, the lower first). Only a stage that
    /// decides the form at run time (the interpreter) reads one; a specializer's residual
    /// leaves at its miss instead.
    Narrow(W),
}

/// The layout of one object record and how many values its shape holds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Layout<W, C> {
    pub(crate) slots: Slots<W, C>,
    pub(crate) storage_len: usize,
}

/// The address of the table entry of the record's layout (`heap::layouts`), which holds its
/// header words: the layout id is the one word of them the record keeps.
#[inline(always)]
pub(crate) fn layout_entry<S: Stage>(stage: &mut S, at: super::region::Access, record: S::Word) -> S::Word {
    let id = stage.load_u32(at, record, OBJECT.layout_offset as i32);
    let base = stage.table(super::Table::Layouts);
    let offset = stage.mul_imm(id, 8u64);
    stage.add(base, offset)
}

/// The record's header word at `offset` in its layout's entry (`OBJECT.meta_offset`,
/// `OBJECT.auxiliary_offset`), its layout id read as `at`.
#[inline(always)]
pub(crate) fn header_u32<S: Stage>(stage: &mut S, at: super::region::Access, record: S::Word, offset: usize) -> S::Word {
    let entry = layout_entry(stage, at, record);
    stage.load_u32(Region::HeapMeta.readonly(), entry, offset as i32)
}

/// The record's inline-storage flag (`Object::meta`), true where its values are inline.
#[inline(always)]
pub(crate) fn inline_storage<S: Stage>(stage: &mut S, record: S::Word) -> S::Cond {
    let meta = header_u32(stage, Region::Records.mutable(), record, OBJECT.meta_offset);
    let inline = stage.and_imm(meta, u64::from(OBJECT.inline_storage_mask));
    stage.ne_imm(inline, 0u32)
}

/// Whether an object's payload word points at its extras (`Object::payload`).
#[inline(always)]
pub(crate) fn has_extras<S: Stage>(stage: &mut S, payload: S::Word) -> S::Cond {
    let tag = stage.and_imm(payload, OBJECT.extras_tag_mask);
    stage.eq_imm(tag, OBJECT.extras_tag)
}

/// Whether an object's payload word holds plain storage, no extras: `has_extras` negated.
#[inline(always)]
pub(crate) fn plain_payload<S: Stage>(stage: &mut S, payload: S::Word) -> S::Cond {
    let tag = stage.and_imm(payload, OBJECT.extras_tag_mask);
    stage.ne_imm(tag, OBJECT.extras_tag)
}

/// The extras an object's payload word points at, once `has_extras` holds.
#[inline(always)]
pub(crate) fn extras_pointer<S: Stage>(stage: &mut S, payload: S::Word) -> S::Word {
    stage.and_imm(payload, !OBJECT.extras_tag_mask)
}

/// The record's exact shape as every cache records it (`Object::exact_shape`): its auxiliary
/// word without the storage-form bits (`storage_form`).
#[inline(always)]
pub(crate) fn exact_shape<S: Stage>(stage: &mut S, record: S::Word) -> S::Word {
    let word = header_u32(stage, Region::ShapeWord.mutable(), record, OBJECT.auxiliary_offset);
    recorded_shape(stage, word)
}

/// `exact_shape` of an auxiliary word already loaded.
#[inline(always)]
pub(crate) fn recorded_shape<S: Stage>(stage: &mut S, word: S::Word) -> S::Word {
    stage.and_imm(word, u64::from(!(OBJECT.inline_form | OBJECT.extras_form | OBJECT.narrow_form)))
}

/// Whether the storage-form bit `bit` (`OBJECT.inline_form`, `OBJECT.extras_form`) is set in
/// the auxiliary word `word` loaded from a record. Every writer of the header's inline flag or
/// the payload's extras tag keeps these bits current (`Object::storage_form`), so a record's
/// word answers `inline_storage` and `has_extras` without loading the header or the payload.
#[inline(always)]
pub(crate) fn storage_form<S: Stage>(stage: &mut S, word: S::Word, bit: u32) -> S::Cond {
    let set = stage.and_imm(word, u64::from(bit));
    stage.ne_imm(set, 0u32)
}

/// The record's encoded prototype link (`Object::proto_link`).
#[inline(always)]
pub(crate) fn proto_link<S: Stage>(stage: &mut S, record: S::Word) -> S::Word {
    let meta = header_u32(stage, Region::Records.mutable(), record, OBJECT.meta_offset);
    stage.and_imm(meta, u64::from(OBJECT.proto_mask))
}

/// The id of the object record `link` names, or `miss` where it names none
/// (`ProtoLink::object_record_id`). The range starts at a multiple of its power-of-two span,
/// so a link in it is the base in its high bits, one mask and compare, and holds the id in
/// its low bits, a mask that addresses the record without the subtracted base.
#[inline(always)]
pub(crate) fn proto_object_record_id<S: Stage>(stage: &mut S, link: S::Word, miss: S::Miss) -> Option<S::Word> {
    const SPAN: u32 = OBJECT.proto_record_end - OBJECT.proto_record_base;
    const _: () = assert!(SPAN.is_power_of_two() && OBJECT.proto_record_base.is_multiple_of(SPAN));
    let high = stage.and_imm(link, !u64::from(SPAN - 1));
    let inside = stage.eq_imm(high, u64::from(OBJECT.proto_record_base));
    stage.guard(inside, miss)?;
    Some(stage.and_imm(link, u64::from(SPAN - 1)))
}

/// The address of object record `id` in the record table at `records`.
#[inline(always)]
pub(crate) fn object_record_at<S: Stage>(stage: &mut S, records: S::Word, id: S::Word) -> S::Word {
    let offset = stage.mul_imm(id, OBJECT.record_size as u64);
    stage.add(records, offset)
}

/// The id and address of the object record the direct object reference `bits` names, in the
/// record table at `records`; `miss` for any other word, or where the VM has no table (zero).
/// The Cranelift projection of the same read is `ObjectRecord::prove`.
#[inline(always)]
#[allow(dead_code)] // read by the stencil build (`quench-bjit/stencils/ic_rows.rs`), which includes this file
pub(crate) fn direct_object_record<S: Stage>(stage: &mut S, bits: S::Word, records: S::Word, miss: S::Miss)
    -> Option<(S::Word, S::Word)> {
    let object = super::value::is_direct_object(stage, bits);
    stage.guard(object, miss)?;
    let present = stage.ne_imm(records, 0u32);
    stage.guard(present, miss)?;
    let id = stage.and_imm(bits, u64::from(u32::MAX));
    Some((id, object_record_at(stage, records, id)))
}

/// The address of the first arena value of the run starting at `start`.
#[inline(always)]
pub(crate) fn arena_run<S: Stage>(stage: &mut S, start: S::Word) -> S::Word {
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
    let payload = stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32);
    let extras = has_extras(stage, payload);
    layout_of(stage, record, payload, inline, extras, form, storage_len, access, miss)
}

/// `own_layout` of the record at `record` whose auxiliary word `word` the caller loaded to prove
/// its exact shape: the word's storage-form bits (`storage_form`) stand for the header's inline
/// flag and the payload's extras tag, so neither is loaded to be tested.
#[inline(always)]
pub(crate) fn formed_own_layout<S: Stage>(stage: &mut S, record: S::Word, word: S::Word, form: Form,
    storage_len: usize, access: PackedAccess, miss: S::Miss) -> Option<Layout<S::Word, S::Cond>> {
    // A stage that decides the form takes the arena run, the common form, on one test.
    let bits = stage.and_imm(word, u64::from(OBJECT.inline_form | OBJECT.extras_form | OBJECT.narrow_form));
    let arena = stage.eq_imm(bits, 0u32);
    if stage.known(arena) == Some(true) && form != Form::Packed {
        let payload = stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32);
        return Some(Layout { slots: Slots::Flat(arena_run(stage, payload)), storage_len });
    }
    let extras = storage_form(stage, word, OBJECT.extras_form);
    let inline = storage_form(stage, word, OBJECT.inline_form);
    let payload = stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32);
    layout_of(stage, record, payload, inline, extras, form, storage_len, access, miss)
}

/// `own_layout` past its loads: `payload` is the record's payload word, `inline` and `extras`
/// its inline flag and extras tag.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn layout_of<S: Stage>(stage: &mut S, record: S::Word, payload: S::Word, inline: S::Cond, extras: S::Cond, form: Form,
    storage_len: usize, access: PackedAccess, miss: S::Miss) -> Option<Layout<S::Word, S::Cond>> {
    if stage.known(extras) == Some(true) {
        return Some(Layout { slots: extras_slots(stage, payload, inline), storage_len });
    }
    // The extras test already decided it: one compare of the masked tag, not a second one.
    let plain = stage.not(extras);
    stage.guard(plain, miss)?;
    let packed = match access {
        PackedAccess::Snapshot => Packed::Snapshot(payload),
        PackedAccess::Reload => Packed::Record(record),
    };
    Some(Layout { slots: plain_slots(stage, payload, packed, inline, form, miss)?, storage_len })
}

/// The layout of the object record at `record` whose `storage_len` values the arena holds behind
/// its extras (`Object::extras_storage_start`), as a global object by name does, `inline` being
/// its `inline_storage`: a record without extras, or with its values inline, leaves at `miss`.
#[inline(always)]
pub(crate) fn arena_extras_layout<S: Stage>(stage: &mut S, record: S::Word, inline: S::Cond, storage_len: usize,
    miss: S::Miss) -> Option<Layout<S::Word, S::Cond>> {
    let payload = stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32);
    let extras = has_extras(stage, payload);
    stage.guard(extras, miss)?;
    let flat = stage.not(inline);
    stage.guard(flat, miss)?;
    Some(Layout { slots: extras_slots(stage, payload, inline), storage_len })
}

/// The slots of an object whose payload word points at its extras, which hold the inline
/// values wide or the run's start (`Object::extras_storage_start`).
fn extras_slots<S: Stage>(stage: &mut S, payload: S::Word, inline: S::Cond) -> Slots<S::Word, S::Cond> {
    let pointer = extras_pointer(stage, payload);
    if stage.known(inline) == Some(true) {
        Slots::Flat(stage.add_imm(pointer, OBJECT.extras_inline_values_offset as u64))
    } else {
        let start = stage.load_u32(Region::Payload.mutable(), pointer, OBJECT.extras_arena_start_offset as i32);
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
                // A narrow run on the flat side leaves: only the interpreter reads one, and its
                // stage decides the flag.
                let narrow = narrow_run(stage, payload);
                let wide = stage.not(narrow);
                let admitted = stage.any(inline, wide);
                stage.guard(admitted, miss)?;
                // The run is addressed on the flat side of the flag only.
                let flat = stage.choose(inline, |stage| stage.constant(0), |stage| arena_run(stage, payload));
                return Some(Slots::Either { packed, inline, flat });
            }
        },
    };
    if !flat {
        return Some(Slots::Packed(packed));
    }
    // A flat plain object's run may be narrow (`NARROW_RUN_TAG` beside its start): the
    // interpreter reads it, a specializer leaves.
    let narrow = narrow_run(stage, payload);
    if stage.known(narrow) == Some(true) {
        return Some(Slots::Narrow(arena_run(stage, payload)));
    }
    let wide = stage.not(narrow);
    stage.guard(wide, miss)?;
    Some(Slots::Flat(arena_run(stage, payload)))
}

/// Whether a plain object's payload word, outside inline storage, names a narrow run.
#[inline(always)]
pub(crate) fn narrow_run<S: Stage>(stage: &mut S, payload: S::Word) -> S::Cond {
    let tag = stage.and_imm(payload, OBJECT.narrow_run_tag);
    stage.ne_imm(tag, 0u32)
}

/// Value `slot` of an object of `layout`, which holds it.
#[inline(always)]
pub(crate) fn read_slot<S: Stage>(stage: &mut S, layout: Layout<S::Word, S::Cond>, slot: u16) -> S::Word {
    debug_assert!(usize::from(slot) < layout.storage_len);
    match layout.slots {
        Slots::Flat(base) => flat_read(stage, base, slot),
        Slots::Packed(packed) => packed_read(stage, packed, slot),
        Slots::Either { packed, inline, flat } => stage.choose(inline,
            |stage| packed_read(stage, packed, slot), |stage| flat_read(stage, flat, slot)),
        Slots::Narrow(base) => narrow_read(stage, base, slot),
    }
}

/// Narrow value `slot` of the run whose word zero is at `base`, decoded.
#[inline(always)]
fn narrow_read<S: Stage>(stage: &mut S, base: S::Word, slot: u16) -> S::Word {
    const _: () = assert!(cfg!(target_endian = "little"), "narrow slot i is the u32 at byte 4 * i");
    let offset = stage.site(u64::from(slot) * u64::from(u32::BITS / 8));
    let address = stage.add_imm(base, offset);
    let narrow = stage.load_u32(Region::ObjectSlots.mutable(), address, 0);
    decode_narrow(stage, narrow)
}

/// Flat value `slot`: the slot is a per-site operand, so its byte offset is a hole.
#[inline(always)]
fn flat_read<S: Stage>(stage: &mut S, base: S::Word, slot: u16) -> S::Word {
    let offset = stage.site(u64::from(slot) * OBJECT.property_value_size as u64);
    stage.load_site(Region::ObjectSlots.mutable(), base, offset)
}

/// Packed value `slot`: its half of the payload word, decoded.
#[inline(always)]
fn packed_read<S: Stage>(stage: &mut S, packed: Packed<S::Word>, slot: u16) -> S::Word {
    debug_assert!(usize::from(slot) < OBJECT.inline_property_count);
    let payload = match packed {
        Packed::Snapshot(payload) => payload,
        Packed::Record(record) => stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32),
    };
    let shift = stage.site(u64::from(slot) * u64::from(u32::BITS));
    let half = stage.shr_imm(payload, shift);
    let narrow = stage.and_imm(half, u64::from(u32::MAX));
    decode_narrow(stage, narrow)
}

/// The value a narrow word (`Value::to_narrow`), zero-extended, encodes (`Value::from_narrow`):
/// an integer's code shifted down under the int tag, or a reference's upper word by its code
/// beside its index.
#[inline(always)]
pub(crate) fn decode_narrow<S: Stage>(stage: &mut S, narrow: S::Word) -> S::Word {
    let upper_words = stage.constant(NarrowLayout::upper_words() as u64);
    decode_narrow_through(stage, narrow, upper_words)
}

/// `decode_narrow` with the reference codes' upper words at `upper_words`, a copy of
/// `NarrowLayout::upper_words` (an ahead-of-time image links its own).
#[inline(always)]
pub(crate) fn decode_narrow_through<S: Stage>(stage: &mut S, narrow: S::Word, upper_words: S::Word) -> S::Word {
    let flag = stage.and_imm(narrow, u64::from(NarrowLayout::INTEGER_FLAG));
    let integer = stage.ne_imm(flag, 0u32);
    stage.choose(integer, |stage| {
        let shifted = stage.sar32_imm(narrow, NarrowLayout::CODE_SHIFT);
        stage.or_imm(shifted, ValueLayout::INT_TAG)
    }, |stage| {
        let code = stage.shr_imm(narrow, NarrowLayout::CODE_SHIFT);
        let code = stage.and_imm(code, u64::from(NarrowLayout::CODE_MASK));
        let offset = stage.mul_imm(code, std::mem::size_of::<u64>() as u64);
        let entry = stage.add(upper_words, offset);
        let upper = stage.load_table(Region::HeapMeta.readonly(), entry);
        let lower = stage.shr_imm(narrow, NarrowLayout::INDEX_SHIFT);
        stage.or(upper, lower)
    })
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
    let word = header_u32(&mut stage, Region::ShapeWord.mutable(), record, OBJECT.auxiliary_offset);
    let layout = formed_own_layout(&mut stage, record, word, Form::Either, slot + 1, PackedAccess::Reload, ())
        .expect("the interpreter knows every storage form");
    crate::Value::from_bits(read_slot(&mut stage, layout, slot as u16))
}
