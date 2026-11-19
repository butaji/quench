//! The tier guards: the proofs a compiled row checks before it runs, each one definition over
//! `Stage` and the frame's answers. A guard has no opcode of its own: a tier row builds its
//! `Operation` from its holes and runs the family step over its places (`steps.rs`). Where the
//! proof fails the guard yields `None` and the row leaves through its own miss edge.

use super::register_frame::Frame;
use super::stage::Stage;
use super::value::tag_languages;

/// `identity`: `source` holds exactly the word `expected` (bit identity, no conversion).
#[inline(always)]
pub(crate) fn identity<S: Stage>(stage: &mut S, source: S::Word, expected: u64, miss: S::Miss) -> Option<()> {
    let same = stage.eq_imm(source, expected);
    stage.guard(same, miss)
}

/// `live`: the code's invalidation word is zero, so none of the facts the code holds has changed
/// where it is set (`code_dependencies::Invalidation`). The frame reads the word.
#[inline(always)]
pub(crate) fn live<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, miss: S::Miss) -> Option<()> {
    let word = frame.invalidation(stage)?;
    let zero = stage.eq_imm(word, 0_u64);
    stage.guard(zero, miss)
}

/// `realm`: the active realm's global object word is the one `expected` the code folded its facts
/// of (`Vm::realm_globals_offset`). The frame reads the word.
#[inline(always)]
pub(crate) fn realm<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, expected: u64, miss: S::Miss) -> Option<()> {
    let globals = frame.realm_globals(stage)?;
    let same = stage.eq_imm(globals, expected);
    stage.guard(same, miss)
}

/// `declared`: the language bits the tag of `source` alone decides (`tag_languages`) meet the
/// declared `mask`. A word no tag decides sets no bit, so it fails.
#[inline(always)]
pub(crate) fn declared<S: Stage>(stage: &mut S, source: S::Word, mask: u64, miss: S::Miss) -> Option<()> {
    let languages = tag_languages(stage, source);
    let admitted = stage.and_imm(languages, mask);
    let held = stage.ne_imm(admitted, 0_u64);
    stage.guard(held, miss)
}

/// `back_edge`: a taken backward edge polls for an interrupt or a due collection first (the
/// frame's `safepoint`), then counts in the loop's counter (the frame's `count_edge`). A failed poll
/// counts nothing.
#[inline(always)]
pub(crate) fn back_edge<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, proof: S::Proof) -> Option<()> {
    frame.safepoint(stage, proof)?;
    frame.count_edge(stage)
}
