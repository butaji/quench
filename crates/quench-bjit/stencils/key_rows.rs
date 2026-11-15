//! The key and iteration rows: the fast arms of `RequireIteratorResult`, `ToPropertyKey`,
//! `typeof`, `instanceof` and `for-in`, the staged rows (`staged/{iteration,value,compare}.rs`)
//! run on `StencilStage`.

use crate::layout::{JS_STEP_ARRAY_RECORD_BASE, JS_STEP_EPOCH, JS_STEP_OBJECT_RECORD_BASE};
use crate::numeric::{HOLE_A, HOLE_B, Out, S};
use crate::words::hole64;
use crate::places::{DEST, LEFT, Places, Rep};
use crate::stage::StencilStage;
use crate::value::ValueLayout;
use crate::staged::object;
use crate::staged::{Frame, Stage, Step, iteration, value};
use crate::codes::{MEM, SITE_OBJECT_AT, HELD_LINK_AT, KEY_ROW, OBJECT_ROW, TYPEOF_INDEX_ROW, TYPEOF_STRING_ROW, TYPEOF_IS_ROW, TYPEOF_ISNOT_ROW, INSTANCEOF_ROW};
use crate::heap_rows::{finish_fast, word_at};

/// `RequireIteratorResult`'s fast arm: the operand (hole 1) is a direct object or array record
/// (`iteration::require_iterator_result`).
#[inline(always)]
fn object_row(s: S) -> Out {
    let mut frame = Places::<MEM, MEM, MEM>::new(s);
    let done = matches!(iteration::require_iterator_result(&mut StencilStage::from_hole(0), &mut frame, LEFT, ()), Step::Value(_));
    finish_fast(frame, done)
}

/// `ToPropertyKey`'s fast arm: a direct string operand (hole 1) is copied to the destination
/// (hole 0, `iteration::to_property_key`).
#[inline(always)]
fn key_row(s: S) -> Out {
    let mut frame = Places::<MEM, MEM, MEM>::new(s);
    let done = matches!(iteration::to_property_key(&mut StencilStage::from_hole(0), &mut frame, DEST, LEFT, ()), Step::Value(_));
    finish_fast(frame, done)
}

/// The tag-decided `typeof` index of the operand (hole 1) as an int in the destination (hole 0), or a
/// heap value, which leaves (`value::immediate_typeof_index`).
#[inline(always)]
fn typeof_index_row(s: S) -> Out {
    let mut frame = Places::<MEM, MEM, MEM>::new(s);
    let mut stage = StencilStage::from_hole(0);
    let word = frame.tagged(&mut stage, LEFT);
    let done = value::immediate_typeof_index(&mut stage, word, ()).map(|index| {
        let boxed = stage.or_imm(index, ValueLayout::INT_TAG);
        frame.write(&mut stage, DEST, boxed, Rep::Tagged);
    }).is_some();
    finish_fast(frame, done)
}

/// The `typeof` result string of the operand (hole 1) from the table at hole 2, in the destination (hole 0), or a
/// heap value, which leaves (`value::immediate_typeof_string`).
#[inline(always)]
fn typeof_string_row(s: S) -> Out {
    let mut frame = Places::<MEM, MEM, MEM>::new(s);
    let mut stage = StencilStage::from_hole(0);
    let word = frame.tagged(&mut stage, LEFT);
    let done = value::immediate_typeof_string(&mut stage, word, hole64::<HOLE_B>(), ()).map(|name| frame.write(&mut stage, DEST, name, Rep::Tagged)).is_some();
    finish_fast(frame, done)
}

/// The fast arm of `instanceof` of a held constructor: the value operand (the frame offset above the destination's in
/// hole 0) walked along its prototype chain for the link above `HELD_LINK_AT` in hole 1 (`object::instanceof_by_words`),
/// where the constructor operand (the frame offset in hole 1's low half) holds exactly the identity of hole 2. The boolean
/// is written to the destination (hole 0), or the row leaves into the helper step.
#[inline(always)]
fn instanceof_row(s: S, payload: usize) -> Out {
    let (value, constructor) = (hole64::<0>() >> SITE_OBJECT_AT, hole64::<HOLE_A>());
    let mut frame = Places::<MEM, MEM, MEM>::new(s);
    let mut stage = StencilStage::from_hole(0);
    let held = word_at(frame.s.frame, (constructor & u64::from(u32::MAX)) as usize) == hole64::<HOLE_B>();
    let records = word_at(payload, JS_STEP_OBJECT_RECORD_BASE);
    let words = (word_at(frame.s.frame, value as usize), constructor >> HELD_LINK_AT);
    let done = held && object::instanceof_by_words(&mut stage, records, words.0, words.1, ()).map(|holds| {
        let (boolean, fact) = frame.boolean(&mut stage, holds);
        frame.write(&mut stage, DEST, boolean, fact);
    }).is_some();
    finish_fast(frame, done)
}

/// Whether the tag-decided `typeof` index of the operand (hole 1) is the name at index hole 2 (or is
/// not, where `NEGATED`), as a boolean in the destination (hole 0).
#[inline(always)]
fn typeof_is_row<const NEGATED: bool>(s: S) -> Out {
    let mut frame = Places::<MEM, MEM, MEM>::new(s);
    let mut stage = StencilStage::from_hole(HOLE_B);
    let word = frame.tagged(&mut stage, LEFT);
    let done = value::immediate_typeof_index(&mut stage, word, ()).map(|index| {
        let tested = stage.site(0);
        let named = stage.eq_imm(index, tested);
        let holds = if NEGATED { stage.not(named) } else { named };
        let (boolean, fact) = frame.boolean(&mut stage, holds);
        frame.write(&mut stage, DEST, boolean, fact);
    }).is_some();
    finish_fast(frame, done)
}

/// `ForInNext`'s fast arm: the for-in state (hole 1) yields its next key to the destination (hole 0)
/// where the state's memo proves the snapshot current (`iteration::for_in_next`), which moves the state's
/// cursor; a hit continues at TAKEN.
#[inline(always)]
fn for_in_row(s: S, payload: usize) -> Out {
    let mut frame = Places::<MEM, MEM, MEM>::new(s);
    let mut stage = StencilStage::from_hole(0);
    let state = frame.tagged(&mut stage, LEFT);
    let tables = (word_at(payload, JS_STEP_ARRAY_RECORD_BASE), word_at(payload, JS_STEP_OBJECT_RECORD_BASE));
    let key = iteration::for_in_next(&mut stage, tables, state, word_at(payload, JS_STEP_EPOCH), ());
    if let Some(key) = key {
        frame.write(&mut stage, DEST, key, Rep::Tagged);
    }
    finish_fast(frame, key.is_some())
}

/// The key or iteration row of kind `KIND`.
#[inline(always)]
pub(crate) fn run<const KIND: u8>(s: S, payload: usize) -> Out {
    match KIND {
        OBJECT_ROW => object_row(s),
        KEY_ROW => key_row(s),
        TYPEOF_INDEX_ROW => typeof_index_row(s),
        TYPEOF_STRING_ROW => typeof_string_row(s),
        TYPEOF_IS_ROW => typeof_is_row::<false>(s),
        TYPEOF_ISNOT_ROW => typeof_is_row::<true>(s),
        INSTANCEOF_ROW => instanceof_row(s, payload),
        _ => for_in_row(s, payload),
    }
}
