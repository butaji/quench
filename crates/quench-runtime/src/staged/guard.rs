//! The tier guards: the proofs a compiled row checks before it runs, each one definition over
//! `Stage` and the frame's answers. A guard has no opcode of its own: a tier row builds its
//! `Operation` from its holes and runs the family step over its places (`steps.rs`). Where the
//! proof fails the guard yields `None` and the row leaves through its own miss edge.

use super::register_frame::Frame;
use super::stage::Stage;
use super::value::tag_languages;

/// `identity`: `source` holds exactly the word `expected` (bit identity, no conversion).
#[inline(always)]
pub fn identity<S: Stage>(stage: &mut S, source: S::Word, expected: u64, miss: S::Miss) -> Option<()> {
    let same = stage.eq_imm(source, expected);
    stage.guard(same, miss)
}

/// `live`: the code's invalidation word is zero, so none of the facts the code holds has changed
/// where it is set (`code_dependencies::Invalidation`). The frame reads the word.
#[inline(always)]
pub fn live<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, miss: S::Miss) -> Option<()> {
    let word = frame.invalidation(stage)?;
    let zero = stage.eq_imm(word, 0_u64);
    stage.guard(zero, miss)
}

/// `realm`: the active realm's global object word is the one `expected` the code folded its facts
/// of (`Vm::realm_globals_offset`). The frame reads the word.
#[inline(always)]
pub fn realm<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, expected: u64, miss: S::Miss) -> Option<()> {
    let globals = frame.realm_globals(stage)?;
    let same = stage.eq_imm(globals, expected);
    stage.guard(same, miss)
}

/// `declared`: the language bits the tag of `source` alone decides (`tag_languages`) meet the
/// declared `mask`. A word no tag decides sets no bit, so it fails.
#[inline(always)]
pub fn declared<S: Stage>(stage: &mut S, source: S::Word, mask: u64, miss: S::Miss) -> Option<()> {
    let languages = tag_languages(stage, source);
    let admitted = stage.and_imm(languages, mask);
    let held = stage.ne_imm(admitted, 0_u64);
    stage.guard(held, miss)
}

/// A guard over a direct object's record (`prototype`, `shape`, and the record arms of the iteration reads): a
/// direct object is a live class cell and has no object record, so such a guard always leaves at `miss`. The
/// shape-keyed reads of a live cell take its class key instead (`class`, `entry::class_matches`).
#[inline(always)]
pub fn no_record<S: Stage>(stage: &mut S, miss: S::Miss) -> Option<()> {
    let never = stage.cond_of(false);
    stage.guard(never, miss)
}

/// `class`: `source` is a direct object whose live class cell's class is `class` (`ic::guard_class`). The class is one
/// compare and fixes the exact shape and the prototype link together, so no record and no link test runs. Any word
/// whose cell's class is not `class` leaves.
#[inline(always)]
pub fn class<S: Stage>(stage: &mut S, source: S::Word, class: u32, miss: S::Miss) -> Option<super::Shaped<S::Word>> {
    let direct = super::value::is_direct_object(stage, source);
    stage.guard(direct, miss)?;
    let cell = super::object::cell_of(stage, source);
    let loaded = super::object::cell_class(stage, cell);
    let same = stage.eq_imm(loaded, u64::from(class));
    stage.guard(same, miss)?;
    Some(super::Shaped::of_cell(cell))
}

/// `stores`: the run's dispatch word `specialized` says its stores specialize (`slot_store::store_admitted`). A region's entry
/// checks it once; the stores it admits without a test of their own hold until a call (`base_region::proofs`).
#[inline(always)]
pub fn stores<S: Stage>(stage: &mut S, specialized: S::Word, miss: S::Miss) -> Option<()> {
    super::slot_store::store_admitted(stage, specialized, miss)
}

/// `back_edge`: a taken backward edge polls for an interrupt or a due collection first (the
/// frame's `safepoint`), then counts in the loop's counter (the frame's `count_edge`). A failed poll
/// counts nothing.
#[inline(always)]
pub fn back_edge<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, proof: S::Proof) -> Option<()> {
    frame.safepoint(stage, proof)?;
    frame.count_edge(stage)
}
