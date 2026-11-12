//! The inline-cache rows: a property read or store whose site observed the receiver's shape,
//! as the shared staged IC (`staged/ic.rs`, `staged/slot_store.rs`) run on `StencilStage`. One
//! row is one cache entry; a site of several entries is a chain of rows (`CHAINED`).
//!
//! Holes. A get: 0 is the destination frame offset, 1 the receiver's frame offset, then the
//! entry's operands in the order the staged ops name them: the receiver shape (2) and the slot's
//! byte offset (3); a prototype-held read (`IC_GET_HOLDER_ROW`) names the holder's shape between
//! them (2, 3 and the slot offset 4). A store: 0 is the receiver's frame offset, 1 the value
//! (any Number place), then the atom (2), the shape (3) and the slot's byte offset (4). The shape
//! ids and slot offsets are values the stitcher patches from the site's IC record
//! (`base_rows::get_entry_holes`); nothing is read from the record at run time.
//!
//! Outcomes. A hit continues at `HOLE_NEXT`, or at `HOLE_TAKEN` where the row is `CHAINED`: the
//! stitcher points it past the rest of the site's chain. A miss of a `CHAINED` row falls to
//! `HOLE_NEXT`, the site's next entry; the last entry's miss, and a destination that cannot hold
//! the value, leave through exit 0 with nothing written, so the interpreter runs the
//! instruction. Receiver proofs (a direct object, the record table present) are per-execution
//! facts, proved once per row.
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

use crate::codes::{IC_GET_HOLDER_ROW, IC_GET_ROW, IC_SET_INT_ROW};
use crate::heap_rows::{payload, word_at};
use crate::layout::{JS_STEP_METHOD_CACHE_ATOMS, JS_STEP_METHOD_CACHE_ATOM_WORDS, JS_STEP_OBJECT_RECORD_BASE, JS_STEP_STORES_SPECIALIZED};
use crate::numeric::{HOLE_A, Out, S, deliver, is_int, number_word};
use crate::stage::StencilStage;
use crate::staged::ic::{self, IcOp, Reached};
use crate::staged::object::{Form, direct_object_record};
use crate::staged::slot_store::own_flat_store;
use crate::staged::Stage;
use crate::words::hole;

/// The hole the receiver's frame offset is in, for a get (operand 1) and a store (operand 0).
const GET_RECEIVER: u8 = HOLE_A;
const SET_RECEIVER: u8 = 0;
/// The first hole of an entry's own operands: the two before it are the row's frame operands.
const ENTRY_HOLES: u8 = 2;
/// Bits in a method-cache bitset word, as a shift.
const WORD_SHIFT: u32 = u64::BITS.trailing_zeros();
const WORD_BITS: u64 = u64::BITS as u64;

/// The receiver's record id, address and the record table, proved: the frame word at hole `H`
/// is a direct object and the VM has an object table.
#[inline(always)]
fn receiver<const H: u8>(s: &S, stage: &mut StencilStage, payload: usize) -> Option<(u64, u64, u64)> {
    let bits = word_at(s.frame, hole::<H>());
    let records = word_at(payload, JS_STEP_OBJECT_RECORD_BASE);
    let (id, record) = direct_object_record(stage, bits, records, ())?;
    Some((id, record, records))
}

/// The word the entry's ops read from the receiver: its own flat slot, or the slot of its
/// prototype, which keeps its values in the arena (`IcOp::LoadPrototypeSlot`).
#[inline(always)]
fn read<const HOLDER: bool>(s: &S, payload: usize) -> Option<u64> {
    let mut stage = StencilStage::with_payload(ENTRY_HOLES, payload as u64);
    let (_, record, records) = receiver::<GET_RECEIVER>(s, &mut stage, payload)?;
    let at = Reached { record, shape: None, inline: None };
    if HOLDER {
        let ops = [IcOp::GuardShape(0), IcOp::Prototype, IcOp::GuardShape(0), IcOp::LoadPrototypeSlot(0)];
        ic::run(&mut stage, &ops, at, Some(records), ())
    } else {
        let ops = [IcOp::GuardShape(0), IcOp::LoadSlot { slot: 0, form: Form::Arena, storage_len: 1 }];
        ic::run(&mut stage, &ops, at, None, ())
    }
}

/// A hit of a chained row skips the site's other entries.
#[inline(always)]
fn hit<const CHAINED: bool>(out: Out) -> Out {
    match out {
        Out::Next(s) if CHAINED => Out::Taken(s),
        other => other,
    }
}

/// A miss of a chained row tries the next entry, the last one's leaves.
#[inline(always)]
fn miss<const CHAINED: bool>(s: S) -> Out {
    if CHAINED { Out::Next(s) } else { Out::Exit(s) }
}

#[inline(always)]
fn get_row<const HOLDER: bool, const CHAINED: bool, const D: u8>(s: S, payload: usize) -> Out {
    match read::<HOLDER>(&s, payload) {
        Some(word) => hit::<CHAINED>(deliver::<D>(s, word)),
        None => miss::<CHAINED>(s),
    }
}

/// Whether a store of the atom at the next hole is admitted: the program runs specialized and no
/// method cache names the atom, an atom past the bitset being unnamed (`emit_atom_unnamed`, the
/// Cranelift reading of the same two payload words).
#[inline(always)]
fn store_admitted(stage: &mut StencilStage, payload: u64) -> Option<()> {
    let specialized = stage.load_u64(payload, JS_STEP_STORES_SPECIALIZED as i32);
    let on = stage.ne_imm(specialized, 0u32);
    stage.guard(on, ())?;
    let atom = stage.site(0);
    let index = stage.shr_imm(atom, WORD_SHIFT);
    let length = stage.load_u64(payload, JS_STEP_METHOD_CACHE_ATOM_WORDS as i32);
    let covered = stage.ult(index, length);
    let named = stage.choose(covered, |stage| {
        let words = stage.load_u64(payload, JS_STEP_METHOD_CACHE_ATOMS as i32);
        let offset = stage.mul_imm(index, size_of::<u64>() as u64);
        let address = stage.add(words, offset);
        let word = stage.load_u64(address, 0);
        let position = stage.and_imm(atom, WORD_BITS - 1);
        let shifted = stage.shr_imm(word, position);
        stage.and_imm(shifted, 1u64)
    }, |stage| stage.constant(0));
    let unnamed = stage.eq_imm(named, 0u32);
    stage.guard(unnamed, ())
}

/// The store of the Number in the value place into the receiver's flat slot; a slot of int32
/// type (`INT`) takes int32 words only, so no compiled read of the slot's type is widened.
#[inline(always)]
fn write<const INT: bool, const A: u8>(s: &S, payload: usize) -> Option<()> {
    let word = number_word::<A, HOLE_A>(s)?;
    if INT && !is_int(word) {
        return None;
    }
    let mut stage = StencilStage::with_payload(ENTRY_HOLES, payload as u64);
    store_admitted(&mut stage, payload as u64)?;
    let (id, record, _) = receiver::<SET_RECEIVER>(s, &mut stage, payload)?;
    own_flat_store(&mut stage, record, id, 0, 0, word, ())
}

#[inline(always)]
fn set_row<const INT: bool, const CHAINED: bool, const A: u8>(s: S, payload: usize) -> Out {
    match write::<INT, A>(&s, payload) {
        Some(()) => hit::<CHAINED>(Out::Next(s)),
        None => miss::<CHAINED>(s),
    }
}

/// The cache row `KIND` (a code of `codes.rs`); `CHAINED` as above; `D` is a get's destination
/// place and `A` a store's value place.
#[inline(always)]
pub(crate) fn run<const KIND: u8, const CHAINED: bool, const D: u8, const A: u8>(s: S) -> Out {
    let payload = payload(&s);
    match KIND {
        IC_GET_ROW => get_row::<false, CHAINED, D>(s, payload),
        IC_GET_HOLDER_ROW => get_row::<true, CHAINED, D>(s, payload),
        IC_SET_INT_ROW => set_row::<true, CHAINED, A>(s, payload),
        _ => set_row::<false, CHAINED, A>(s, payload),
    }
}
