//! Object storage layouts as staged reads: where a record of a proved exact shape keeps its
//! named values, and how a narrow value decodes. The interpreter reads them on `Concrete`; a
//! specializer emits them.

use super::{Concrete, Stage, Table};
use crate::heap::{NATIVE_OBJECT_LAYOUT as OBJECT, PlainPropertyStorage};
use crate::value::{
    HEAP_FAMILY_BITS, HEAP_FAMILY_SHIFT, LOWER_WORD, NARROW_HEAP_FAMILIES, NarrowLayout, TAG_BOOL, TAG_DELETED, TAG_NULL, TAG_UNDEFINED, ValueLayout,
};

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

/// The record's encoded prototype link (`Object::proto_link`).
#[inline(always)]
pub(crate) fn proto_link<S: Stage>(stage: &mut S, record: S::Word) -> S::Word {
    let meta = stage.load_u32(record, OBJECT.meta_offset as i32);
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
pub(crate) fn proto_object_record_id<S: Stage>(stage: &mut S, link: S::Word, miss: S::Miss) -> Option<S::Word> {
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
pub(crate) fn walk_prototypes<S: Stage>(stage: &mut S, records: S::Word, object: S::Word, target: S::Word,
    links: u32) -> (S::Cond, S::Cond) {
    let answer = walk_from(stage, records, object, target, links);
    let undecided = stage.eq_imm(answer, WALK_UNDECIDED);
    let decided = stage.not(undecided);
    (decided, stage.eq_imm(answer, WALK_HOLDS))
}

/// The answer of the walk from `record`, with `links` more links to follow.
fn walk_from<S: Stage>(stage: &mut S, records: S::Word, record: S::Word, target: S::Word, links: u32) -> S::Word {
    let link = proto_link(stage, record);
    let gap = stage.sub(link, target);
    let reached = stage.eq_imm(gap, 0u64);
    stage.choose(reached, |stage| stage.constant(WALK_HOLDS), |stage| {
        let null = stage.eq_imm(link, u64::from(NULL_LINK));
        stage.choose(null, |stage| stage.constant(WALK_FAILS), |stage| walk_on(stage, records, link, target, links))
    })
}

/// The walk past `link`, which is neither the target nor `null`: into the object record it names.
fn walk_on<S: Stage>(stage: &mut S, records: S::Word, link: S::Word, target: S::Word, links: u32) -> S::Word {
    let Some(rest) = links.checked_sub(1) else { return stage.constant(WALK_UNDECIDED) };
    let names = proto_names_record(stage, link);
    stage.choose(names, |stage| {
        let id = proto_record_id(stage, link);
        let next = object_record_at(stage, records, id);
        walk_from(stage, records, next, target, rest)
    }, |stage| stage.constant(WALK_UNDECIDED))
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
    let answer = instance_answer(stage, records, value, target);
    let undecided = stage.eq_imm(answer, WALK_UNDECIDED);
    let decided = stage.not(undecided);
    stage.guard(decided, miss)?;
    Some(stage.eq_imm(answer, WALK_HOLDS))
}

/// The walk's answer for `value`: a direct object's chain, `fails` for a primitive, else undecided.
#[inline(always)]
fn instance_answer<S: Stage>(stage: &mut S, records: S::Word, value: S::Word, target: S::Word) -> S::Word {
    let direct = super::value::is_direct_object(stage, value);
    stage.choose(direct, |stage| {
        let id = super::value::direct_object_id(stage, value);
        let record = object_record_at(stage, records, id);
        let (decided, holds) = stage.walk_prototypes(records, record, target, INSTANCEOF_WALK_LINKS);
        let [fails, holding, undecided] = [WALK_FAILS, WALK_HOLDS, WALK_UNDECIDED].map(|answer| stage.constant(answer));
        let answer = stage.select(holds, holding, fails);
        stage.select(decided, answer, undecided)
    }, |stage| {
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
pub(crate) fn object_record_at<S: Stage>(stage: &mut S, records: S::Word, id: S::Word) -> S::Word {
    let offset = stage.mul_imm(id, OBJECT.record_size as u64);
    stage.add(records, offset)
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
    let payload = stage.load_u64(record, OBJECT.payload_offset as i32);
    let extras = has_extras(stage, payload);
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

/// The slots of an object whose payload word points at its extras, which hold the inline
/// values wide or the run's start (`Object::extras_storage_start`).
fn extras_slots<S: Stage>(stage: &mut S, payload: S::Word, inline: S::Cond) -> Slots<S::Word, S::Cond> {
    let pointer = extras_pointer(stage, payload);
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
    match layout.slots {
        Slots::Flat(base) => flat_read(stage, base, slot),
        Slots::Packed(packed) => packed_read(stage, packed, slot),
        Slots::Either { packed, inline, flat } => stage.choose(inline,
            |stage| packed_read(stage, packed, slot), |stage| flat_read(stage, flat, slot)),
    }
}

/// Flat value `slot`: the slot is a per-site operand, so its byte offset is a hole.
#[inline(always)]
fn flat_read<S: Stage>(stage: &mut S, base: S::Word, slot: u16) -> S::Word {
    let offset = stage.site(u64::from(slot) * OBJECT.property_value_size as u64);
    stage.load_site(base, offset)
}

/// Packed value `slot`: its half of the payload word, decoded.
#[inline(always)]
fn packed_read<S: Stage>(stage: &mut S, packed: Packed<S::Word>, slot: u16) -> S::Word {
    debug_assert!(usize::from(slot) < OBJECT.inline_property_count);
    let payload = match packed {
        Packed::Snapshot(payload) => payload,
        Packed::Record(record) => stage.load_u64(record, OBJECT.payload_offset as i32),
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
        let upper = stage.load_table(entry);
        let lower = stage.shr_imm(narrow, NarrowLayout::INDEX_SHIFT);
        stage.or(upper, lower)
    })
}

/// The upper words of the narrow constants, their codes following the families' codes.
const CONSTANT_UPPER_WORDS: [u64; 4] = [TAG_UNDEFINED, TAG_NULL, TAG_BOOL, TAG_DELETED];
const FIRST_CONSTANT_CODE: u64 = NARROW_HEAP_FAMILIES as u64;
/// A bit mask's complement in the low word: the upper word of a value.
const UPPER_WORD: u64 = !(u32::MAX as u64);
/// The word a value that has no narrow form (a Number, a wide reference) yields.
const NOT_NARROW: u64 = u64::MAX;

/// The narrow form of `value` (`Value::to_narrow`) where it has one: an integer in range, a
/// reference whose index fits, or a constant; `NOT_NARROW` for any other value, which the
/// store leaves to the general path. Each form computes only its own arm.
#[inline(always)]
fn narrow_form<S: Stage>(stage: &mut S, value: S::Word) -> S::Word {
    let upper = stage.and_imm(value, UPPER_WORD);
    let is_int = stage.eq_imm(upper, ValueLayout::INT_TAG);
    stage.choose(is_int, |stage| {
        // An integer fits where its biased low word is below the span.
        let lower = stage.and_imm(value, LOWER_WORD);
        let span = 1_u64 << NarrowLayout::INTEGER_BITS;
        let biased = stage.add_imm(lower, span >> 1);
        let biased = stage.and_imm(biased, LOWER_WORD);
        let over = stage.ugt_imm(biased, span - 1);
        let fits = stage.not(over);
        let shifted = stage.mul_imm(lower, 1_u64 << NarrowLayout::CODE_SHIFT);
        let shifted = stage.and_imm(shifted, LOWER_WORD);
        let integer = stage.or_imm(shifted, u64::from(NarrowLayout::INTEGER_FLAG));
        let refused = stage.constant(NOT_NARROW);
        stage.select(fits, integer, refused)
    }, |stage| reference_form(stage, value, upper))
}

/// The word a store of `value` writes into a slot of a record whose values are packed
/// (`inline`): its narrow form, or `None` (leaving at `miss`) where it has none. A flat slot takes
/// the value whole, and its word here is zero.
#[inline(always)]
pub(crate) fn packed_form<S: Stage>(stage: &mut S, inline: S::Cond, value: S::Word, miss: S::Miss) -> Option<S::Word> {
    let narrow = stage.choose(inline, |stage| narrow_form(stage, value), |stage| stage.constant(0));
    let storable = stage.ne_imm(narrow, NOT_NARROW);
    stage.guard(storable, miss)?;
    Some(narrow)
}

/// `narrow_form` of a value that is no integer: its code is its family bits, a constant's
/// follows them, and the lower word is the index.
#[inline(always)]
fn reference_form<S: Stage>(stage: &mut S, value: S::Word, upper: S::Word) -> S::Word {
    let lower = stage.and_imm(value, LOWER_WORD);
    let family = stage.and_imm(upper, HEAP_FAMILY_BITS);
    let base = stage.and_imm(upper, !HEAP_FAMILY_BITS);
    let mut named = stage.eq_imm(base, ValueLayout::HEAP_TAG);
    let mut code = stage.shr_imm(family, HEAP_FAMILY_SHIFT);
    for (offset, word) in CONSTANT_UPPER_WORDS.into_iter().enumerate() {
        let is_constant = stage.eq_imm(upper, word);
        let constant_code = stage.constant(FIRST_CONSTANT_CODE + offset as u64);
        code = stage.select(is_constant, constant_code, code);
        named = stage.any(named, is_constant);
    }
    let index_high = stage.shr_imm(lower, NarrowLayout::INDEX_BITS);
    let index_fits = stage.eq_imm(index_high, 0u32);
    let fits = stage.pick(named, index_fits, named);
    let index = stage.mul_imm(lower, 1_u64 << NarrowLayout::INDEX_SHIFT);
    let coded = stage.mul_imm(code, 1_u64 << NarrowLayout::CODE_SHIFT);
    let reference = stage.or(index, coded);
    let refused = stage.constant(NOT_NARROW);
    stage.select(fits, reference, refused)
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
