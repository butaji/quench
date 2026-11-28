//! The inline-cache rows: a property read or store whose site observed the receiver's shape,
//! as the shared staged IC (`staged/ic.rs`, `staged/slot_store.rs`) run on `StencilStage`. One
//! row is one cache entry; a site of several entries is a chain of rows (`CHAINED`).
//!
//! Holes. A get: 0 is the destination frame offset with the receiver's above it (`SITE_OBJECT_AT`),
//! 1 the upper-word table a packed reference decodes through, then the
//! entry's operands in the order the staged ops name them: the receiver shape (2) and the slot's
//! byte offset (3). A store: 0 is the receiver's frame offset, 1 the value
//! (any Number place), then the atom (2), the shape (3) and the slot's byte offset (4). The shape
//! ids and slot offsets are values the stitcher patches from the site's IC record
//! (`base_rows::get_entry_holes`); nothing is read from the record at run time.
//!
//! Outcomes. A hit continues at `HOLE_NEXT`, or at `HOLE_TAKEN` where the row is `CHAINED`: the
//! stitcher points it past the rest of the site's chain. A miss of a `CHAINED` row falls to
//! `HOLE_NEXT`, the site's next entry; the last entry's miss, and a destination that cannot hold
//! the value, leave through exit 0 with nothing written, so the interpreter runs the
//! instruction. Receiver proofs are per-execution facts, proved once per row; the object record table is
//! gone (S7f), so a direct object is read through its class cell (`IC_*_CLASS_ROW`).
//!
//! Expected code of a flat own get on a frame destination: load the receiver word, test its
//! family, mask the id, scale it and add the record table base, compare the record's exact shape
//! with the patched shape (`jne` miss), load the payload word and test the extras tag and the
//! inline flag, load the arena base through the allocator header, load `[base + run + slot]`
//! and store it to the destination: about 25 instructions, four of them branches, no call.
//!
//! Re-patching (not implemented). The shape and slot offset are the only operands that change
//! when a later execution observes another receiver shape at an entry that missed, and each is
//! one 32-bit field (`Imm32`) of the copied code. A site's miss helper would write the new pair
//! through the code heap's write view (`vm/code_heap/views.rs`) into the entry's fields, found
//! from the same relocation table the stitcher patched them from, then re-run the instruction;
//! the code stays valid because the shape dependency (`OsrFacts::shape_dependencies`) already
//! covers every id a patch names. The entry's holes would move from `Planned::holes` into a
//! per-row record of field offsets for this, which the stitcher keeps beside the code.

use crate::codes::{IC_GET_CLASS_ROW, IC_GET_HELD_CLASS_ROW, IC_SET_CLASS_ROW, IC_GET_ROW, IC_PROOF_ROW, IC_SET_INT_ROW, MEM};
use crate::heap_rows::{payload, word_at};
use crate::numeric::{HOLE_A, Out, S, deliver, number_word};
use crate::places::{Places, finish_chained, site_object};
use crate::places_property::{Callout, planned_frame, receiver_read, receiver_set};
use crate::stage::StencilStage;
use crate::staged::entry::{class_held_field, class_key_cell, class_own_field, class_store};
use crate::staged::ic::IcOp;
use crate::staged::object::Form;
use crate::typed_ir::Representation;
use crate::staged::{Stage, StagedRecordSpace, Step, op_kind, step_property_of, value};
use crate::words::hole;

/// The hole the receiver's frame offset is in: for a get, the upper half of hole 0, whose lower half
/// is the destination's (hole 1 is the narrow decode's upper-word table, `NarrowLayout::upper_words`,
/// as a site row's); for a store, operand 0.
const SET_RECEIVER: u8 = 0;
/// The first hole of an entry's own operands: the two before it are the row's frame operands.
const ENTRY_HOLES: u8 = 2;

/// A shape-keyed get: `GetField` of the receiver (above the destination in hole 0) through the entry's ops, the own
/// slot (`GuardShape`, `LoadSlot`); the word goes to the destination `D`.
#[inline(always)]
fn get_row<const CHAINED: bool, const D: u8>(s: S, payload: usize) -> Out {
    let mut stage = StencilStage::with_payload(ENTRY_HOLES, payload as u64);
    // The receiver is the word above the destination's offset in hole 0, not hole 1's (`HOLE_A`).
    let receiver = site_object(&s);
    let mut places = Places::new::<D, MEM, MEM>(s);
    let ops = [IcOp::GuardShape(0), IcOp::LoadSlot { slot: 0, form: Form::Either, storage_len: 1, rep: Representation::Tagged }];
    let step = step_property_of::<{ op_kind::GET_FIELD }, _, _>(&mut stage, &mut received(planned_frame(&mut places, ops, Callout::Plan), receiver), receiver_read());
    finish_chained::<CHAINED>(places.s, step, |s, word| deliver::<D>(s, word))
}

/// `frame` with register `LEFT` reading `receiver`.
#[inline(always)]
fn received<const N: usize>(mut frame: crate::places_property::PlannedPlaces<'_, N>, receiver: u64) -> crate::places_property::PlannedPlaces<'_, N> {
    frame.receiver = Some(receiver);
    frame
}

/// A set: `SetField` of the value (`A`, hole 1) into the receiver (hole 0), through the entry's
/// flat own slot (`FlatStore`); an int32 row takes int32 values only, so no compiled read of the
/// slot's type is widened.
#[inline(always)]
fn set_row<const INT: bool, const CHAINED: bool, const A: u8>(s: S, payload: usize) -> Out {
    let mut stage = StencilStage::with_payload(ENTRY_HOLES, payload as u64);
    let mut places = Places::new::<MEM, A, MEM>(s);
    let step = step_property_of::<{ op_kind::SET_FIELD }, _, _>(&mut stage, &mut planned_frame(&mut places, [], Callout::FlatStore { int: INT }), receiver_set());
    finish_chained::<CHAINED>(places.s, step, |s, _| Out::Next(s))
}

/// A class-keyed get (`IC_GET_CLASS_ROW`, `IC_GET_HELD_CLASS_ROW`): the receiver (above the destination in hole 0) is a
/// direct object whose live class cell is the class hole 2 names, and the word is that cell's field at hole 3 (a field
/// index); the held read takes the field of the holder cell whose granule is hole 4 (`entry::class_held_field`). A miss
/// leaves as `get_row`'s does. The word goes to `D`.
#[inline(always)]
fn class_get_row<const HELD: bool, const CHAINED: bool, const D: u8>(s: S, payload: usize) -> Out {
    let mut stage = StencilStage::with_payload(ENTRY_HOLES, payload as u64);
    let object = site_object(&s);
    let key = stage.site(0);
    let slot = stage.site(0);
    let word = if HELD {
        let holder = stage.site(0);
        class_held_field(&mut stage, object, key, slot, holder, ())
    } else {
        class_own_field(&mut stage, object, key, slot, ())
    };
    finish_chained::<CHAINED>(s, word.map_or(Step::Unresolved, Step::Value), |s, word| deliver::<D>(s, word))
}

/// The Number in the value place stored to the field at hole 3 of the live class cell the receiver (hole 0) is, whose class
/// key is hole 2 (`IC_SET_CLASS_ROW`): the store gate admits the write (`StoreGate` of the class space, as
/// `add::add_cell_property`), then the field is stored. `None` where the row leaves with nothing written.
#[inline(always)]
fn class_set_value<const A: u8>(s: &S, payload: usize) -> Option<u64> {
    let word = number_word::<A, HOLE_A>(s)?;
    let mut stage = StencilStage::with_payload(ENTRY_HOLES, payload as u64);
    let object = word_at(s.frame, hole::<SET_RECEIVER>());
    let key = stage.site(0);
    let slot = stage.site(0);
    let (granule, cell) = class_key_cell(&mut stage, object, key, ())?;
    let open = stage.store_gate(StagedRecordSpace::Class, granule, word);
    stage.guard(open, ())?;
    class_store(&mut stage, cell, slot, word);
    Some(word)
}

/// `class_set_value` as the row's outcome: a hit continues at TAKEN where chained, else leaves through exit 0.
#[inline(always)]
fn class_set_row<const CHAINED: bool, const A: u8>(s: S, payload: usize) -> Out {
    let stored = class_set_value::<A>(&s, payload);
    finish_chained::<CHAINED>(s, stored.map_or(Step::Unresolved, Step::Value), |s, _| Out::Next(s))
}

/// The receiver proof row (`IC_PROOF_ROW`): continues where the object register at hole 0 is a direct object, whose
/// live class cell the class rows that follow test by its class word (`GuardClass`); any other word leaves with
/// nothing written.
#[inline(always)]
fn proof(s: S) -> Out {
    let object = word_at(s.frame, hole::<0>());
    let mut stage = StencilStage::from_hole(0);
    let direct = value::is_direct_object(&mut stage, object);
    match stage.guard(direct, ()) {
        Some(()) => Out::Next(s),
        None => Out::Exit(s),
    }
}

/// The cache row `KIND` (a code of `codes.rs`); `CHAINED` as above; `D` is a get's destination
/// place and `A` a store's value place.
#[inline(always)]
pub(crate) fn run<const KIND: u8, const CHAINED: bool, const D: u8, const A: u8>(s: S) -> Out {
    let payload = payload(&s);
    match KIND {
        IC_GET_ROW => get_row::<CHAINED, D>(s, payload),
        IC_SET_INT_ROW => set_row::<true, CHAINED, A>(s, payload),
        IC_GET_CLASS_ROW => class_get_row::<false, CHAINED, D>(s, payload),
        IC_GET_HELD_CLASS_ROW => class_get_row::<true, CHAINED, D>(s, payload),
        IC_SET_CLASS_ROW => class_set_row::<CHAINED, A>(s, payload),
        IC_PROOF_ROW => proof(s),
        _ => set_row::<false, CHAINED, A>(s, payload),
    }
}
