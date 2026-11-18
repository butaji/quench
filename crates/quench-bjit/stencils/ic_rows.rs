//! The inline-cache rows: a property read or store whose site observed the receiver's shape,
//! as the shared staged IC (`staged/ic.rs`, `staged/slot_store.rs`) run on `StencilStage`. One
//! row is one cache entry; a site of several entries is a chain of rows (`CHAINED`).
//!
//! Holes. A get: 0 is the destination frame offset with the receiver's above it (`SITE_OBJECT_AT`),
//! 1 the upper-word table a packed reference decodes through, then the
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

use crate::codes::{IC_GET_HOLDER_ROW, IC_GET_ROW, IC_KNOWN_GET_ROW, IC_KNOWN_SET_INT_ROW, IC_KNOWN_SET_ROW, IC_PROOF_ROW, IC_SET_INT_ROW, MEM, SITE_OBJECT_AT};
use crate::heap_rows::{payload, word_at};
use crate::layout::{JS_STEP_OBJECT_RECORD_BASE, JS_STEP_STORES_SPECIALIZED};
use crate::numeric::{HOLE_A, Out, S, deliver, is_int, number_word};
use crate::places::Places;
use crate::places_property::{Callout, receiver_read, receiver_set, run_planned};
use crate::stage::StencilStage;
use crate::staged::ic::{self, IcOp};
use crate::staged::object::{Form, Rep, direct_object_record, object_record_at};
use crate::staged::region::Region;
use crate::staged::slot_store::{own_flat_known, proved_flat_store};
use crate::staged::value::direct_object_id;
use crate::staged::Stage;
use crate::words::{hole, hole64};

/// The hole the receiver's frame offset is in: for a get, the upper half of hole 0, whose lower half
/// is the destination's (hole 1 is the narrow decode's upper-word table, `NarrowLayout::upper_words`,
/// as a site row's); for a store, operand 0.
const GET_RECEIVER: u8 = HOLE_A;
const SET_RECEIVER: u8 = 0;
/// The first hole of an entry's own operands: the two before it are the row's frame operands.
const ENTRY_HOLES: u8 = 2;

/// The receiver's record id, address and the record table, proved: the frame word at hole `H`
/// (for a get, above the destination's offset in hole 0, as a site row's) is a direct object and
/// the VM has an object table.
#[inline(always)]
fn receiver<const H: u8>(s: &S, stage: &mut StencilStage, payload: usize) -> Option<(u64, u64, u64)> {
    let offset = if H == GET_RECEIVER { (hole64::<0>() >> SITE_OBJECT_AT) as usize } else { hole::<H>() };
    let bits = word_at(s.frame, offset);
    let records = word_at(payload, JS_STEP_OBJECT_RECORD_BASE);
    let (id, record) = direct_object_record(stage, bits, records, ())?;
    Some((id, record, records))
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

/// A get: `GetField` of the receiver (hole 1) through the entry's ops, the own slot (`GuardShape`,
/// `LoadSlot`) or the prototype's (`Prototype`, then the holder's shape and its arena slot); the
/// word goes to the destination `D`.
#[inline(always)]
fn get_row<const HOLDER: bool, const CHAINED: bool, const D: u8>(s: S, payload: usize) -> Out {
    let mut stage = StencilStage::with_payload(ENTRY_HOLES, payload as u64);
    let mut places = Places::<D, MEM, MEM>::new(s);
    let word = if HOLDER {
        let ops = [IcOp::GuardShape(0), IcOp::Prototype, IcOp::GuardShape(0), IcOp::LoadPrototypeSlot(0)];
        run_planned(&mut stage, &mut places, ops, Callout::Plan, receiver_read())
    } else {
        let ops = [IcOp::GuardShape(0), IcOp::LoadSlot { slot: 0, form: Form::Arena, storage_len: 1 }];
        run_planned(&mut stage, &mut places, ops, Callout::Plan, receiver_read())
    };
    match word {
        Some(word) => hit::<CHAINED>(deliver::<D>(places.s, word)),
        None => miss::<CHAINED>(places.s),
    }
}

/// Whether a store of the atom at the next hole is admitted: the program runs specialized. That no
/// method cache names the atom is a Stable fact the code holds (`Decoded::atom`, `held_atoms`): naming it
/// retires the code, and a run already inside learns that from the invalidation word its rows test after
/// a call (`Heap::Live`), so the row reads no bitset. The atom's hole stays numbered.
#[inline(always)]
fn store_admitted(stage: &mut StencilStage, payload: u64) -> Option<()> {
    let specialized = stage.load_u64(Region::Dispatch.mutable(), payload, JS_STEP_STORES_SPECIALIZED as i32);
    let on = stage.ne_imm(specialized, 0u32);
    stage.guard(on, ())?;
    let _atom = stage.site(0);
    Some(())
}

/// A set: `SetField` of the value (`A`, hole 1) into the receiver (hole 0), through the entry's
/// flat own slot (`FlatStore`); an int32 row takes int32 values only, so no compiled read of the
/// slot's type is widened.
#[inline(always)]
fn set_row<const INT: bool, const CHAINED: bool, const A: u8>(s: S, payload: usize) -> Out {
    let mut stage = StencilStage::with_payload(ENTRY_HOLES, payload as u64);
    let mut places = Places::<MEM, A, MEM>::new(s);
    match run_planned(&mut stage, &mut places, [], Callout::FlatStore { int: INT }, receiver_set()) {
        Some(_) => hit::<CHAINED>(Out::Next(places.s)),
        None => miss::<CHAINED>(places.s),
    }
}

/// The record of the receiver at hole `H` (as `receiver`'s), which every path proved a direct object
/// (`base_region::proofs`): its id masked and scaled into the record table, no family test.
#[inline(always)]
fn known_record<const H: u8>(s: &S, stage: &mut StencilStage, payload: usize) -> u64 {
    let offset = if H == GET_RECEIVER { (hole64::<0>() >> SITE_OBJECT_AT) as usize } else { hole::<H>() };
    let bits = word_at(s.frame, offset);
    let records = word_at(payload, JS_STEP_OBJECT_RECORD_BASE);
    let id = direct_object_id(stage, bits);
    object_record_at(stage, records, id)
}

/// The own slot of the receiver known to be a direct object (`ic::own_field`: hole 2 the shape, hole 3 the
/// slot's flat byte offset), with no family test.
#[inline(always)]
fn known_get<const D: u8>(s: S, payload: usize) -> Out {
    let mut stage = StencilStage::with_payload(ENTRY_HOLES, payload as u64);
    let record = known_record::<GET_RECEIVER>(&s, &mut stage, payload);
    let word = ic::own_field(&mut stage, record, 0, 0, ());
    match word {
        Some(word) => hit::<true>(deliver::<D>(s, word)),
        None => miss::<true>(s),
    }
}

/// The Number in the value place stored to the flat slot (hole 4: its byte offset) of the receiver known
/// to be a direct object where the atom (hole 2) is admitted (`store_admitted`) and the receiver is of the
/// exact shape at hole 3; an int32 slot (`INT`) takes int32 words only.
#[inline(always)]
fn known_set<const INT: bool, const A: u8>(s: S, payload: usize) -> Out {
    let Some(word) = number_word::<A, HOLE_A>(&s).filter(|word| !INT || is_int(*word)) else { return miss::<true>(s) };
    let mut stage = StencilStage::with_payload(ENTRY_HOLES, payload as u64);
    if store_admitted(&mut stage, payload as u64).is_none() {
        return miss::<true>(s);
    }
    let record = known_record::<SET_RECEIVER>(&s, &mut stage, payload);
    if own_flat_known(&mut stage, record, 0, ()).is_none() {
        return miss::<true>(s);
    }
    proved_flat_store(&mut stage, record, 0, (word, if INT { Representation::Int32 } else { Representation::Tagged }));
    hit::<true>(Out::Next(s))
}

/// Continues where the object register at hole 0 is a direct object and the VM has an object table
/// (`receiver`), else leaves through exit 0.
#[inline(always)]
fn proof(s: S, payload: usize) -> Out {
    let mut stage = StencilStage::with_payload(ENTRY_HOLES, payload as u64);
    match receiver::<SET_RECEIVER>(&s, &mut stage, payload).map(|_| ()) {
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
        IC_GET_ROW => get_row::<false, CHAINED, D>(s, payload),
        IC_GET_HOLDER_ROW => get_row::<true, CHAINED, D>(s, payload),
        IC_SET_INT_ROW => set_row::<true, CHAINED, A>(s, payload),
        IC_PROOF_ROW => proof(s, payload),
        IC_KNOWN_GET_ROW => known_get::<D>(s, payload),
        IC_KNOWN_SET_ROW => known_set::<false, A>(s, payload),
        IC_KNOWN_SET_INT_ROW => known_set::<true, A>(s, payload),
        _ => set_row::<false, CHAINED, A>(s, payload),
    }
}
