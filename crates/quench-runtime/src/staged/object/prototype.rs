//! Prototype links and chain walks as staged reads: the link of a record, whether a link
//! names an object record, and the walks that decide `instanceof` and prototype membership by words.

use super::*;

/// The record's encoded prototype link (`Object::proto_link`).
#[inline(always)]
pub(crate) fn proto_link<S: Stage>(stage: &mut S, record: S::Word) -> S::Word {
    let meta = header_u32(stage, Region::Records.mutable(), record, OBJECT.meta_offset);
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
#[inline(always)]
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
#[inline(always)]
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
    let direct = super::super::value::is_direct_object(stage, value);
    stage.choose(direct, |stage| {
        let id = super::super::value::direct_object_id(stage, value);
        let record = object_record_at(stage, records, id);
        let (decided, holds) = stage.walk_prototypes(records, record, target, INSTANCEOF_WALK_LINKS);
        let [fails, holding, undecided] = [WALK_FAILS, WALK_HOLDS, WALK_UNDECIDED].map(|answer| stage.constant(answer));
        let answer = stage.select(holds, holding, fails);
        stage.select(decided, answer, undecided)
    }, |stage| {
        let tag = stage.and_imm(value, ValueLayout::TAG_MASK);
        let heap = stage.eq_imm(tag, ValueLayout::HEAP_TAG);
        let immediate = stage.not(heap);
        let string = super::super::value::is_direct_string(stage, value);
        let primitive = stage.any(immediate, string);
        let [fails, undecided] = [WALK_FAILS, WALK_UNDECIDED].map(|answer| stage.constant(answer));
        stage.select(primitive, fails, undecided)
    })
}
