//! The key and iteration rows: the fast arms of `RequireIteratorResult`, `ToPropertyKey`,
//! `typeof`, `instanceof` and `for-in`. Each row builds the staged `Operation` the interpreter
//! decodes for its instruction, over the row's places, and runs its family step on `StencilStage`
//! (`staged/{steps,generic,control,names,heap}.rs`). The frame methods those steps call on `Places`
//! live in `places_names.rs` (`typeof_entry`, `call_out_fast`).

use crate::codes::{MEM, SITE_OBJECT_AT, HELD_LINK_AT, KEY_ROW, OBJECT_ROW, TYPEOF_INDEX_ROW, TYPEOF_STRING_ROW, TYPEOF_IS_ROW, TYPEOF_ISNOT_ROW, INSTANCEOF_ROW};
use crate::heap_rows::word_at;
use crate::numeric::{HOLE_A, HOLE_B, Out, S};
use crate::places::{DEST, LEFT, Places, finish_fast};
use crate::stage::StencilStage;
use crate::staged::{Frame, Operation, Stage, Step, object, op_kind, step_generic_of, step_names_of, step_of, step_value_of};
use crate::words::hole64;

/// `RequireIteratorResult`'s fast arm: the operand (hole 1) is a direct object or array record
/// (`iteration::require_iterator_result`, `step_value`).
#[inline(always)]
fn object_row(s: S) -> Out {
    let mut frame = Places::<MEM, MEM, MEM>::new(s);
    let mut stage = StencilStage::from_hole(0);
    let done = matches!(step_value_of::<{ op_kind::REQUIRE_ITERATOR_RESULT }, _, _>(&mut stage, &mut frame, Operation::RequireIteratorResult { register: LEFT }), Step::Value(_));
    finish_fast(frame, done)
}

/// `ToPropertyKey`'s fast arm: a direct string operand (hole 1) is copied to the destination
/// (hole 0, `Operation::PropertyKey`, `generic::property_key`).
#[inline(always)]
fn key_row(s: S) -> Out {
    let mut frame = Places::<MEM, MEM, MEM>::new(s);
    let mut stage = StencilStage::from_hole(0);
    let done = matches!(step_generic_of::<{ op_kind::PROPERTY_KEY }, _, _>(&mut stage, &mut frame, Operation::PropertyKey { destination: DEST, source: LEFT }), Step::Value(_));
    finish_fast(frame, done)
}

/// The tag-decided `typeof` index of the operand (hole 1) as an int in the destination (hole 0), or a
/// heap value, which leaves (`Operation::TypeofIndex`, `control::typeof_index`).
#[inline(always)]
fn typeof_index_row(s: S) -> Out {
    let mut frame = Places::<MEM, MEM, MEM>::new(s);
    let mut stage = StencilStage::from_hole(0);
    let done = matches!(step_of::<{ op_kind::TYPEOF_INDEX }, _, _>(&mut stage, &mut frame, Operation::TypeofIndex { destination: DEST, source: LEFT }), Step::Value(_));
    finish_fast(frame, done)
}

/// The `typeof` result string of the operand (hole 1) from the table at hole 2, in the destination (hole 0), or a
/// heap value, which leaves (`Operation::TypeofString`, `control::typeof_string`; the table is `Places::typeof_entry`).
#[inline(always)]
fn typeof_string_row(s: S) -> Out {
    let mut frame = Places::<MEM, MEM, MEM>::new(s);
    let mut stage = StencilStage::from_hole(0);
    let done = matches!(step_of::<{ op_kind::TYPEOF_STRING }, _, _>(&mut stage, &mut frame, Operation::TypeofString { destination: DEST, source: LEFT }), Step::Value(_));
    finish_fast(frame, done)
}

/// The fast arm of `instanceof` of a held constructor: the value operand (the frame offset above the destination's in
/// hole 0) walked along its prototype chain for the link above `HELD_LINK_AT` in hole 1 (`object::instanceof_by_words`),
/// where the constructor operand (the frame offset in hole 1's low half) holds exactly the identity of hole 2. The boolean
/// is written to the destination (hole 0), or the row leaves into the helper step.
///
/// The operands are frame offsets packed into the holes, not registers, so `Operation::InstanceOf` (whose operands
/// are the registers `Frame::tagged` reads) cannot name them; the row runs the walk `InstanceOf` runs on the same words.
#[inline(always)]
fn instanceof_row(s: S) -> Out {
    let (value, constructor) = (hole64::<0>() >> SITE_OBJECT_AT, hole64::<HOLE_A>());
    let mut frame = Places::<MEM, MEM, MEM>::new(s);
    let mut stage = StencilStage::from_hole(0);
    let held = word_at(frame.s.frame, (constructor & u64::from(u32::MAX)) as usize) == hole64::<HOLE_B>();
    let records = frame.object_records();
    let words = (word_at(frame.s.frame, value as usize), constructor >> HELD_LINK_AT);
    let done = held && object::instanceof_by_words(&mut stage, records, words.0, words.1, ()).map(|holds| {
        let (boolean, fact) = frame.boolean(&mut stage, holds);
        frame.write(&mut stage, DEST, boolean, fact);
    }).is_some();
    finish_fast(frame, done)
}

/// Whether the tag-decided `typeof` index of the operand (hole 1) is the name at index hole 2 (or is
/// not, where `NEGATED`), as a boolean in the destination (hole 0). `Operation::TypeofTest` with the
/// index the row's hole 2 holds (`control::typeof_test`).
#[inline(always)]
fn typeof_is_row<const NEGATED: bool>(s: S) -> Out {
    let mut frame = Places::<MEM, MEM, MEM>::new(s);
    let mut stage = StencilStage::from_hole(HOLE_B);
    let index = stage.site(0);
    let done = matches!(step_of::<{ op_kind::TYPEOF_TEST }, _, _>(&mut stage, &mut frame, Operation::TypeofTest { destination: DEST, source: LEFT, index, negated: NEGATED }), Step::Value(_));
    finish_fast(frame, done)
}

/// `ForInNext`'s fast arm: the for-in state (hole 1) yields its next key to the destination (hole 0)
/// where the state's memo proves the snapshot current (`Frame::vm_step` answers `CallOut::ForInNext`
/// through `Places::call_out_fast`, `iteration::for_in_next`), which moves the state's cursor; a hit
/// continues at TAKEN. `names::for_in_next` branches on the exhausted marker, which the fast arm never yields.
#[inline(always)]
fn for_in_row(s: S) -> Out {
    let mut frame = Places::<MEM, MEM, MEM>::new(s);
    let mut stage = StencilStage::from_hole(0);
    let done = matches!(step_names_of::<{ op_kind::FOR_IN_NEXT }, _, _>(&mut stage, &mut frame, Operation::ForInNext { destination: DEST, state: LEFT }), Step::Control(()));
    finish_fast(frame, done)
}

/// The key or iteration row of kind `KIND`.
#[inline(always)]
pub(crate) fn run<const KIND: u8>(s: S, _payload: usize) -> Out {
    match KIND {
        OBJECT_ROW => object_row(s),
        KEY_ROW => key_row(s),
        TYPEOF_INDEX_ROW => typeof_index_row(s),
        TYPEOF_STRING_ROW => typeof_string_row(s),
        TYPEOF_IS_ROW => typeof_is_row::<false>(s),
        TYPEOF_ISNOT_ROW => typeof_is_row::<true>(s),
        INSTANCEOF_ROW => instanceof_row(s),
        _ => for_in_row(s),
    }
}
