//! Object storage layouts as staged reads: where a record of a proved exact shape keeps its
//! named values, and how a narrow value decodes. The interpreter reads them on `Concrete`; a
//! specializer emits them.

use super::{Concrete, Stage, Table};
use super::region::Region;
use crate::heap::{NATIVE_OBJECT_LAYOUT as OBJECT, PlainPropertyStorage};
use crate::typed_ir::Representation;
use crate::value::{NarrowLayout, ValueLayout};

/// Bytes in one half of a packed payload word, the stride of a packed slot.
pub(crate) const HALF_BYTES: u64 = std::mem::size_of::<u32>() as u64;

/// What a site proved about where an object of an exact shape keeps its values. It is a
/// static input: a specializer reads no word to learn it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Form {
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
    #[inline(always)]
    pub fn of(storage: Option<PlainPropertyStorage>, storage_len: usize) -> Self {
        match storage {
            Some(PlainPropertyStorage::PackedInline) => Self::Packed,
            Some(PlainPropertyStorage::Arena) => Self::Arena,
            None if storage_len <= OBJECT.inline_property_count => Self::Either,
            None => Self::Arena,
        }
    }
}

#[cfg_attr(feature = "test-support", quench_macros::test_visible)]
/// How packed values are read: from the record at each read, since a store may reach them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PackedAccess {
    Reload,
}

/// Packed narrow values: the record that holds them.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Packed<W> {
    Record(W),
}

/// Where an object of a proved exact shape keeps its named values.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Slots<W, C> {
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
pub struct Layout<W, C> {
    pub slots: Slots<W, C>,
    pub storage_len: usize,
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
pub fn inline_storage<S: Stage>(stage: &mut S, record: S::Word) -> S::Cond {
    let meta = header_u32(stage, Region::ShapeWord.mutable(), record, OBJECT.meta_offset);
    let inline = stage.and_imm(meta, u64::from(OBJECT.inline_storage_mask));
    stage.ne_imm(inline, 0u32)
}

/// Whether an object's payload word points at its extras (`Object::payload`).
#[inline(always)]
pub fn has_extras<S: Stage>(stage: &mut S, payload: S::Word) -> S::Cond {
    let tag = stage.and_imm(payload, OBJECT.extras_tag_mask);
    stage.eq_imm(tag, OBJECT.extras_tag)
}

/// Whether an object's payload word holds plain storage, no extras: `has_extras` negated.
#[inline(always)]
pub fn plain_payload<S: Stage>(stage: &mut S, payload: S::Word) -> S::Cond {
    let tag = stage.and_imm(payload, OBJECT.extras_tag_mask);
    stage.ne_imm(tag, OBJECT.extras_tag)
}

/// The extras an object's payload word points at, once `has_extras` holds.
#[inline(always)]
pub fn extras_pointer<S: Stage>(stage: &mut S, payload: S::Word) -> S::Word {
    stage.and_imm(payload, !OBJECT.extras_tag_mask)
}

/// The record's exact shape as every cache records it (`Object::exact_shape`): its auxiliary
/// word without the storage-form bits (`storage_form`).
#[inline(always)]
pub(crate) fn exact_shape<S: Stage>(stage: &mut S, record: S::Word) -> S::Word {
    let word = aux_word(stage, record);
    recorded_shape(stage, word)
}

/// The record's whole auxiliary word: its shape and storage-form bits (`recorded_shape`, `storage_form`).
#[inline(always)]
pub(crate) fn aux_word<S: Stage>(stage: &mut S, record: S::Word) -> S::Word {
    header_u32(stage, Region::ShapeWord.mutable(), record, OBJECT.auxiliary_offset)
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
pub fn proto_link<S: Stage>(stage: &mut S, record: S::Word) -> S::Word {
    let meta = header_u32(stage, Region::ShapeWord.mutable(), record, OBJECT.meta_offset);
    stage.and_imm(meta, u64::from(OBJECT.proto_mask))
}

/// The span of the object record range of prototype links. The range starts at a multiple of its
/// power-of-two span, so a link in it is the base in its high bits, one mask and compare, and
/// holds the id in its low bits, a mask that addresses the record without the subtracted base.
const RECORD_LINKS: u32 = OBJECT.proto_record_end - OBJECT.proto_record_base;
const _: () = assert!(RECORD_LINKS.is_power_of_two() && OBJECT.proto_record_base.is_multiple_of(RECORD_LINKS));

/// The link of a `null` prototype: the all-ones link field (`ProtoLink::NULL`).
const NULL_LINK: u32 = OBJECT.proto_mask;

/// Whether `link` names an object record (`ProtoLink::object_record_id`).
#[inline(always)]
pub(crate) fn proto_names_record<S: Stage>(stage: &mut S, link: S::Word) -> S::Cond {
    let high = stage.and_imm(link, !u64::from(RECORD_LINKS - 1));
    stage.eq_imm(high, u64::from(OBJECT.proto_record_base))
}

/// The id of the object record `link` names, which `proto_names_record` proved it names.
#[inline(always)]
pub(crate) fn proto_record_id<S: Stage>(stage: &mut S, link: S::Word) -> S::Word {
    stage.and_imm(link, u64::from(RECORD_LINKS - 1))
}

/// The id of the object record `link` names, or `miss` where it names none.
#[inline(always)]
pub fn proto_object_record_id<S: Stage>(stage: &mut S, link: S::Word, miss: S::Miss) -> Option<S::Word> {
    let inside = proto_names_record(stage, link);
    stage.guard(inside, miss)?;
    Some(proto_record_id(stage, link))
}

/// A prototype walk's answer as a word: the target is not on the chain, is on it, or a link names
/// no object record (a proxy, an array, a function) or the links ran out, which only the VM decides.
const WALK_FAILS: u64 = 0;
const WALK_HOLDS: u64 = 1;
const WALK_UNDECIDED: u64 = 2;

/// Whether `target`, an encoded prototype link (`ProtoLink::bits`), lies on the prototype chain of
/// the object record `object` (`records` is the object record table's base), as `(decided, holds)`.
/// At most `links` links past the first are followed, each a nested conditional, so a stage that
/// knows its conditions runs the one arm taken and a specializer emits one block per link.
#[inline(always)]
pub fn walk_prototypes<S: Stage>(stage: &mut S, records: S::Word, object: S::Word, target: S::Word,
    links: u32) -> (S::Cond, S::Cond) {
    let answer = walk_from(stage, records, object, target, links);
    let undecided = stage.eq_imm(answer, WALK_UNDECIDED);
    let decided = stage.not(undecided);
    (decided, stage.eq_imm(answer, WALK_HOLDS))
}

/// The answer of the walk from `record`, with `links` more links to follow.
#[inline(always)]
fn walk_from<S: Stage>(stage: &mut S, records: S::Word, record: S::Word, target: S::Word, links: u32) -> S::Word {
    let link = proto_link(stage, record);
    let gap = stage.sub(link, target);
    let reached = stage.eq_imm(gap, 0u64);
    stage.choose(reached, #[inline(always)] |stage| stage.constant(WALK_HOLDS), #[inline(always)] |stage| {
        let null = stage.eq_imm(link, u64::from(NULL_LINK));
        stage.choose(null, #[inline(always)] |stage| stage.constant(WALK_FAILS), #[inline(always)] |stage| walk_on(stage, records, link, target, links))
    })
}

/// The walk past `link`, which is neither the target nor `null`: into the object record it names.
#[inline(always)]
fn walk_on<S: Stage>(stage: &mut S, records: S::Word, link: S::Word, target: S::Word, links: u32) -> S::Word {
    let Some(rest) = links.checked_sub(1) else { return stage.constant(WALK_UNDECIDED) };
    let names = proto_names_record(stage, link);
    stage.choose(names, #[inline(always)] |stage| {
        let id = proto_record_id(stage, link);
        let next = object_record_at(stage, records, id);
        walk_from(stage, records, next, target, rest)
    }, #[inline(always)] |stage| stage.constant(WALK_UNDECIDED))
}

/// `walk_prototypes` as a loop, for a stage whose words and conditions are the machine's own
/// (the interpreter, a stencil).
#[inline(always)]
pub(crate) fn walk_prototypes_loop<S: Stage<Word = u64, Cond = bool>>(stage: &mut S, records: u64, object: u64,
    target: u64, links: u32) -> (bool, bool) {
    let mut record = object;
    for _ in 0..=links {
        let link = proto_link(stage, record);
        if link == target { return (true, true); }
        if link == u64::from(NULL_LINK) { return (true, false); }
        if !proto_names_record(stage, link) { break; }
        let id = proto_record_id(stage, link);
        record = object_record_at(stage, records, id);
    }
    (false, false)
}

/// The longest prototype chain an `instanceof` walk follows by words: an instance, its class chain and
/// `Object.prototype` end well within it, and a longer chain is the VM's to decide.
pub(crate) const INSTANCEOF_WALK_LINKS: u32 = 16;

/// Whether `value instanceof` a constructor whose `prototype` is the encoded link `target` holds, by the
/// words of `value` alone (`records` is the object record table's base; that the constructor is steady
/// and its `prototype` is `target` is the caller's proof): a primitive, an immediate or a string, is no
/// instance, as the algorithm decides it before it reads `prototype`; a direct object's record is walked
/// (`Stage::walk_prototypes`). Any other cell (an array, a function, a proxy), a chain the walk does not
/// decide, or an absent record table leaves at `miss`.
#[inline(always)]
pub(crate) fn instanceof_by_words<S: Stage>(stage: &mut S, records: S::Word, value: S::Word, target: S::Word,
    miss: S::Miss) -> Option<S::Cond> {
    let present = stage.ne_imm(records, 0u32);
    stage.guard(present, miss)?;
    let (decided, holds) = instance_decision(stage, records, value, target);
    stage.guard(decided, miss)?;
    Some(holds)
}

/// The walk's decision for `value` as `(decided, holds)`: whether the words decide `instanceof`, and
/// where they do, whether it holds.
#[inline(always)]
pub(crate) fn instance_decision<S: Stage>(stage: &mut S, records: S::Word, value: S::Word, target: S::Word) -> (S::Cond, S::Cond) {
    let answer = instance_answer(stage, records, value, target);
    let undecided = stage.eq_imm(answer, WALK_UNDECIDED);
    (stage.not(undecided), stage.eq_imm(answer, WALK_HOLDS))
}

/// The walk's answer for `value`: a direct object's chain, `fails` for a primitive, else undecided.
#[inline(always)]
fn instance_answer<S: Stage>(stage: &mut S, records: S::Word, value: S::Word, target: S::Word) -> S::Word {
    let direct = super::value::is_direct_object(stage, value);
    stage.choose(direct, #[inline(always)] |stage| {
        let id = super::value::direct_object_id(stage, value);
        let record = object_record_at(stage, records, id);
        let (decided, holds) = stage.walk_prototypes(records, record, target, INSTANCEOF_WALK_LINKS);
        let [fails, holding, undecided] = [WALK_FAILS, WALK_HOLDS, WALK_UNDECIDED].map(|answer| stage.constant(answer));
        let answer = stage.select(holds, holding, fails);
        stage.select(decided, answer, undecided)
    }, #[inline(always)] |stage| {
        let tag = stage.and_imm(value, ValueLayout::TAG_MASK);
        let heap = stage.eq_imm(tag, ValueLayout::HEAP_TAG);
        let immediate = stage.not(heap);
        let string = super::value::is_direct_string(stage, value);
        let primitive = stage.any(immediate, string);
        let [fails, undecided] = [WALK_FAILS, WALK_UNDECIDED].map(|answer| stage.constant(answer));
        stage.select(primitive, fails, undecided)
    })
}

/// The address of object record `id` in the record table at `records`.
#[inline(always)]
pub fn object_record_at<S: Stage>(stage: &mut S, records: S::Word, id: S::Word) -> S::Word {
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

/// The arena run a field slot binds: the address of value `slot` of the run of `length` values at
/// `values`, and how many values remain from it (none where the slot lies outside the run). An
/// address formed outside the run grants no permission to dereference it.
#[inline(always)]
pub fn bind_arena_slot<S: Stage>(stage: &mut S, values: S::Word, length: S::Word, slot: S::Word) -> (S::Word, S::Word) {
    let fits = stage.ult(slot, length);
    let difference = stage.sub(length, slot);
    let none = stage.constant(0);
    let remaining = stage.select(fits, difference, none);
    let offset = stage.mul_imm(slot, OBJECT.property_value_size as u64);
    (stage.add(values, offset), remaining)
}

/// The address of value `position` of the run of `length` values at `values`; `None` (leaving at
/// `miss`) at or past its end.
#[inline(always)]
pub fn arena_slot_address<S: Stage>(stage: &mut S, values: S::Word, length: S::Word, position: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    let within = stage.ult(position, length);
    stage.guard(within, miss)?;
    let offset = stage.mul_imm(position, OBJECT.property_value_size as u64);
    Some(stage.add(values, offset))
}

#[cfg_attr(feature = "test-support", quench_macros::test_visible)]
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

/// The layout of the record at `record` whose whole auxiliary word a guard proved equal to a
/// word with storage-form bits `bits` (`Object::hint_shape`): the form is a static input, so
/// neither the inline flag, the extras tag nor a narrow run is tested. Values behind extras (a
/// watched prototype's, a global object's) are the extras' wide inline values or the run whose
/// start the extras hold, as `extras_slots` reads them. `None` for any other bits.
#[inline(always)]
pub(crate) fn proved_own_layout<S: Stage>(stage: &mut S, shaped: super::Shaped<S::Word>, bits: u32, storage_len: usize)
    -> Option<Layout<S::Word, S::Cond>> {
    let slots = match bits {
        bits if bits == OBJECT.inline_form => Slots::Packed(Packed::Record(shaped.record())),
        0 => {
            let payload = stage.load_u64(Region::Payload.mutable(), shaped.record(), OBJECT.payload_offset as i32);
            Slots::Flat(arena_run(stage, payload))
        }
        bits if bits == OBJECT.narrow_form => {
            let payload = stage.load_u64(Region::Payload.mutable(), shaped.record(), OBJECT.payload_offset as i32);
            Slots::Narrow(arena_run(stage, payload))
        }
        bits if bits == OBJECT.extras_form || bits == OBJECT.extras_form | OBJECT.inline_form => {
            let payload = stage.load_u64(Region::Payload.mutable(), shaped.record(), OBJECT.payload_offset as i32);
            let pointer = extras_pointer(stage, payload);
            if bits == OBJECT.extras_form {
                let start = stage.load_u32(Region::Payload.mutable(), pointer, OBJECT.extras_arena_start_offset as i32);
                Slots::Flat(arena_run(stage, start))
            } else {
                Slots::Flat(stage.add_imm(pointer, OBJECT.extras_inline_values_offset as u64))
            }
        }
        _ => return None,
    };
    Some(Layout { slots, storage_len })
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
        PackedAccess::Reload => Packed::Record(record),
    };
    Some(Layout { slots: plain_slots(stage, payload, packed, inline, form, miss)?, storage_len })
}

/// The layout of the object record at `record` whose `storage_len` values the arena holds behind
/// its extras (`Object::extras_storage_start`), as a global object by name does, `inline` being
/// its `inline_storage`: a record without extras, or with its values inline, leaves at `miss`.
#[inline(always)]
pub fn arena_extras_layout<S: Stage>(stage: &mut S, record: S::Word, inline: S::Cond, storage_len: usize,
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
#[inline(always)]
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
#[inline(always)]
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
                let flat = stage.choose(inline, #[inline(always)] |stage| stage.constant(0), #[inline(always)] |stage| arena_run(stage, payload));
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

/// The address of value zero of the wide run a plain object's `payload` word names outside
/// inline storage: `None` (leaving at `miss`) for a narrow run, whose words are halves.
#[inline(always)]
pub(crate) fn flat_run<S: Stage>(stage: &mut S, payload: S::Word, miss: S::Miss) -> Option<S::Word> {
    let narrow = narrow_run(stage, payload);
    let wide = stage.not(narrow);
    stage.guard(wide, miss)?;
    Some(arena_run(stage, payload))
}

/// Whether a plain object's payload word, outside inline storage, names a narrow run.
#[inline(always)]
pub(crate) fn narrow_run<S: Stage>(stage: &mut S, payload: S::Word) -> S::Cond {
    let tag = stage.and_imm(payload, OBJECT.narrow_run_tag);
    stage.ne_imm(tag, 0u32)
}

#[cfg_attr(feature = "test-support", quench_macros::test_visible)]
/// Value `slot` of an object of `layout`, which holds it, as the word its owner's `rep` encodes:
/// a `Value`'s bits for `Representation::Tagged` and `Representation::Int32`, the IEEE double for `Representation::Float64`
/// (`value_word` makes a `Value` of either). Only an arena slot holds a raw double: a packed or
/// narrow slot holds a narrow value whatever its owner's type, and reads as one, a `Value`.
#[inline(always)]
pub(crate) fn read_slot<S: Stage>(stage: &mut S, layout: Layout<S::Word, S::Cond>, slot: u16, rep: Representation) -> S::Word {
    debug_assert!(usize::from(slot) < layout.storage_len);
    match layout.slots {
        Slots::Flat(base) => flat_read(stage, base, slot, rep),
        Slots::Packed(packed) => packed_read(stage, packed, slot, rep),
        Slots::Either { packed: Packed::Record(record), inline, flat } => {
            // One operand for both forms (`site_offset`): a stencil numbers its holes as the row runs.
            let offset = site_offset(stage, slot);
            stage.choose(inline, #[inline(always)] |stage| {
                let half = stage.shr_imm(offset, HALF_SHIFT);
                let address = stage.add(record, half);
                let narrow = stage.load_u32(Region::Payload.mutable(), address, OBJECT.payload_offset as i32);
                decode_narrow(stage, narrow)
            }, #[inline(always)] |stage| {
                let address = stage.add(flat, offset);
                match rep {
                    Representation::Float64 => stage.load_f64(Region::ObjectSlots.mutable(), address, 0),
                    Representation::Int32 | Representation::Int64 | Representation::Tagged => stage.load_u64(Region::ObjectSlots.mutable(), address, 0),
                }
            })
        }
        Slots::Narrow(base) => narrow_read(stage, base, slot, rep),
    }
}

/// The `Value` bits of the word `read_slot` read at `slot` of `layout` in `rep`: a raw double
/// of an arena run boxed (`Stage::box_number`), any other word as it is.
#[inline(always)]
pub(crate) fn read_slot_value<S: Stage>(stage: &mut S, layout: Layout<S::Word, S::Cond>, slot: u16, rep: Representation) -> S::Word {
    match (rep, layout.slots) {
        (Representation::Float64, Slots::Flat(_)) => { let number = read_slot(stage, layout, slot, rep); stage.box_number(super::Float64::of_row(number)) }
        (Representation::Float64, Slots::Either { packed, inline, flat }) => stage.choose(inline,
            #[inline(always)] |stage| packed_read(stage, packed, slot, Representation::Tagged),
            #[inline(always)] |stage| { let number = flat_read(stage, flat, slot, rep); stage.box_number(super::Float64::of_row(number)) }),
        _ => read_slot(stage, layout, slot, Representation::Tagged),
    }
}

/// `read_slot` at a run-time `index` (a for-in key's listed slot): value `index` of an object
/// of `layout`, which the caller proved holds it (`index < layout.storage_len`), as the word
/// its owner's `rep` encodes.
#[inline(always)]
pub(crate) fn read_slot_at<S: Stage>(stage: &mut S, layout: Layout<S::Word, S::Cond>, index: S::Word, rep: Representation) -> S::Word {
    match layout.slots {
        Slots::Flat(base) => flat_read_at(stage, base, index, rep),
        Slots::Packed(packed) => packed_read_at(stage, packed, index, rep),
        Slots::Either { packed, inline, flat } => stage.choose(inline,
            #[inline(always)] |stage| packed_read_at(stage, packed, index, rep), #[inline(always)] |stage| flat_read_at(stage, flat, index, rep)),
        Slots::Narrow(base) => {
            debug_assert!(rep != Representation::Float64, "a narrow run holds narrow values, never a raw double");
            let offset = stage.mul_imm(index, u64::from(u32::BITS / 8));
            let address = stage.add(base, offset);
            let narrow = stage.load_u32(Region::ObjectSlots.mutable(), address, 0);
            decode_narrow(stage, narrow)
        }
    }
}

/// Flat value `index` of the run at `base`, `index` a run-time word.
#[inline(always)]
fn flat_read_at<S: Stage>(stage: &mut S, base: S::Word, index: S::Word, rep: Representation) -> S::Word {
    let offset = stage.mul_imm(index, OBJECT.property_value_size as u64);
    let address = stage.add(base, offset);
    match rep {
        Representation::Float64 => stage.load_f64(Region::ObjectSlots.mutable(), address, 0),
        Representation::Int32 | Representation::Int64 | Representation::Tagged => stage.load_u64(Region::ObjectSlots.mutable(), address, 0),
    }
}

/// Packed value `index` (0 or 1, a run-time word): its half of the payload word, decoded.
#[inline(always)]
fn packed_read_at<S: Stage>(stage: &mut S, packed: Packed<S::Word>, index: S::Word, rep: Representation) -> S::Word {
    const _: () = assert!(OBJECT.inline_property_count == 2, "a packed index selects one of two halves");
    debug_assert!(rep != Representation::Float64, "a packed slot holds a narrow value, never a raw double");
    let payload = match packed {
        Packed::Record(record) => stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32),
    };
    let upper = stage.shr_imm(payload, u64::from(u32::BITS));
    let second = stage.ne_imm(index, 0u32);
    let half = stage.select(second, upper, payload);
    let narrow = stage.and_imm(half, u64::from(u32::MAX));
    decode_narrow(stage, narrow)
}

/// Narrow value `slot` of the run whose word zero is at `base`, decoded.
#[inline(always)]
fn narrow_read<S: Stage>(stage: &mut S, base: S::Word, slot: u16, _rep: Representation) -> S::Word {
    const _: () = assert!(cfg!(target_endian = "little"), "narrow slot i is the u32 at byte 4 * i");
    let offset = stage.site(u64::from(slot) * u64::from(u32::BITS / 8));
    let address = stage.add_imm(base, offset);
    let narrow = stage.load_u32(Region::ObjectSlots.mutable(), address, 0);
    decode_narrow(stage, narrow)
}

/// The shift from a slot's byte offset in a flat run to its half's in the packed payload word.
const HALF_SHIFT: u64 = (OBJECT.property_value_size as u64 / HALF_BYTES).trailing_zeros() as u64;

/// The byte offset of `slot` in a flat run, a per-site operand, as a word: the one operand a read of
/// either form takes, the packed half's offset being it shifted (`HALF_SHIFT`).
#[inline(always)]
fn site_offset<S: Stage>(stage: &mut S, slot: u16) -> S::Word {
    let offset = stage.site(u64::from(slot) * OBJECT.property_value_size as u64);
    let zero = stage.constant(0);
    stage.add_imm(zero, offset)
}

/// Flat value `slot`: the slot is a per-site operand, so its byte offset is a hole.
#[inline(always)]
fn flat_read<S: Stage>(stage: &mut S, base: S::Word, slot: u16, rep: Representation) -> S::Word {
    let offset = stage.site(u64::from(slot) * OBJECT.property_value_size as u64);
    match rep {
        Representation::Float64 => {
            let address = stage.add_imm(base, offset);
            stage.load_f64(Region::ObjectSlots.mutable(), address, 0)
        }
        Representation::Int32 | Representation::Int64 | Representation::Tagged => stage.load_site(Region::ObjectSlots.mutable(), base, offset),
    }
}

/// Packed value `slot`: its half of the payload word, decoded.
#[inline(always)]
fn packed_read<S: Stage>(stage: &mut S, packed: Packed<S::Word>, slot: u16, _rep: Representation) -> S::Word {
    debug_assert!(usize::from(slot) < OBJECT.inline_property_count);
    let payload = match packed {
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
pub fn decode_narrow_through<S: Stage>(stage: &mut S, narrow: S::Word, upper_words: S::Word) -> S::Word {
    let flag = stage.and_imm(narrow, u64::from(NarrowLayout::INTEGER_FLAG));
    let integer = stage.ne_imm(flag, 0u32);
    stage.choose(integer, #[inline(always)] |stage| {
        let shifted = stage.sar32_imm(narrow, NarrowLayout::CODE_SHIFT);
        stage.or_imm(shifted, ValueLayout::INT_TAG)
    }, #[inline(always)] |stage| {
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
pub unsafe fn concrete_own_read(object: &crate::heap::Object, property_values: *const crate::Value, slot: usize) -> crate::Value {
    let mut stage = Concrete::new(property_values);
    let record = object as *const crate::heap::Object as u64;
    let word = header_u32(&mut stage, Region::ShapeWord.mutable(), record, OBJECT.auxiliary_offset);
    let layout = formed_own_layout(&mut stage, record, word, Form::Either, slot + 1, PackedAccess::Reload, ())
        .expect("the interpreter knows every storage form");
    crate::value_vec::decode_staged_word(read_slot(&mut stage, layout, slot as u16, Representation::Tagged))
}
