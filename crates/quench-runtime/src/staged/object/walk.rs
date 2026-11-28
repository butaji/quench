//! Walking the prototype chain by words, and the `instanceof` decision over it.

use super::*;

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
    let (reached, null) = link_decision(stage, link, target);
    stage.choose(reached, #[inline(always)] |stage| stage.constant(WALK_HOLDS), #[inline(always)] |stage| {
        stage.choose(null, #[inline(always)] |stage| stage.constant(WALK_FAILS), #[inline(always)] |stage| walk_on(stage, records, link, target, links))
    })
}

/// The one decision a walk makes at each link, both forms' (`walk_from`, `walk_prototypes_loop`): whether
/// `link` is the target (the walk holds) and whether it is `null` (it fails); a link that is neither goes
/// on into the record it names where it names one (`next_record`), else the walk is undecided.
#[inline(always)]
fn link_decision<S: Stage>(stage: &mut S, link: S::Word, target: S::Word) -> (S::Cond, S::Cond) {
    let gap = stage.sub(link, target);
    let reached = stage.eq_imm(gap, 0u64);
    (reached, stage.eq_imm(link, u64::from(NULL_LINK)))
}

/// The object record the walk goes on into past `link`, which names one (`proto_names_record`).
#[inline(always)]
fn next_record<S: Stage>(stage: &mut S, records: S::Word, link: S::Word) -> S::Word {
    let id = proto_record_id(stage, link);
    object_record_at(stage, records, id)
}

/// The walk past `link`, which is neither the target nor `null`: into the object record it names.
#[inline(always)]
fn walk_on<S: Stage>(stage: &mut S, records: S::Word, link: S::Word, target: S::Word, links: u32) -> S::Word {
    let Some(rest) = links.checked_sub(1) else { return stage.constant(WALK_UNDECIDED) };
    let names = proto_names_record(stage, link);
    stage.choose(names, #[inline(always)] |stage| {
        let next = next_record(stage, records, link);
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
        let (reached, null) = link_decision(stage, link, target);
        if reached { return (true, true); }
        if null { return (true, false); }
        if !proto_names_record(stage, link) { break; }
        record = next_record(stage, records, link);
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
    let (decided, holds) = instance_decision(stage, records, value, target, miss)?;
    stage.guard(decided, miss)?;
    Some(holds)
}

/// The walk's decision for `value` as `(decided, holds)`: whether the words decide `instanceof`, and
/// where they do, whether it holds. A live direct object leaves at `miss` (`object_record_of`).
#[inline(always)]
pub(crate) fn instance_decision<S: Stage>(stage: &mut S, records: S::Word, value: S::Word, target: S::Word,
    miss: S::Miss) -> Option<(S::Cond, S::Cond)> {
    let answer = instance_answer(stage, records, value, target, miss)?;
    let undecided = stage.eq_imm(answer, WALK_UNDECIDED);
    Some((stage.not(undecided), stage.eq_imm(answer, WALK_HOLDS)))
}

/// The walk's answer for `value`: a direct object's chain, `fails` for a primitive, else undecided.
#[inline(always)]
fn instance_answer<S: Stage>(stage: &mut S, records: S::Word, value: S::Word, target: S::Word, miss: S::Miss) -> Option<S::Word> {
    let direct = super::super::value::is_direct_object(stage, value);
    stage.choose_leaving(direct, #[inline(always)] |stage| {
        let record = object_record_of(stage, records, value, miss)?;
        let (decided, holds) = stage.walk_prototypes(records, record, target, INSTANCEOF_WALK_LINKS);
        let [fails, holding, undecided] = [WALK_FAILS, WALK_HOLDS, WALK_UNDECIDED].map(|answer| stage.constant(answer));
        let answer = stage.select(holds, holding, fails);
        Some(stage.select(decided, answer, undecided))
    }, #[inline(always)] |stage| {
        let tag = stage.and_imm(value, ValueLayout::TAG_MASK);
        let heap = stage.eq_imm(tag, ValueLayout::HEAP_TAG);
        let immediate = stage.not(heap);
        let string = super::super::value::is_direct_string(stage, value);
        let primitive = stage.any(immediate, string);
        let [fails, undecided] = [WALK_FAILS, WALK_UNDECIDED].map(|answer| stage.constant(answer));
        Some(stage.select(primitive, fails, undecided))
    })
}
