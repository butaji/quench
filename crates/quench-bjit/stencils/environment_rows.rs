//! The environment rows: a capture slot, a slot of the environment window's storage, and the
//! global object's slot, each read, stored or defined through the staged environment rows
//! (`staged/environment.rs`) run on `StencilStage`.

use crate::layout::{JS_STEP_CAPTURE_LEN, JS_STEP_CAPTURE_STORES, JS_STEP_CAPTURE_VALUES, JS_STEP_ENV_STORAGE, JS_STEP_OBJECT_RECORD_BASE, JS_STEP_PROPERTY_BUFFER, PROPERTY_BUFFER_BASE};
use crate::numeric::{HOLE_A, HOLE_B, Out, S, deliver};
use crate::words::{hole, hole64};
use crate::places::tagged_place;
use crate::stage::StencilStage;
use crate::staged::object::{self, Layout, Slots};
use crate::staged::region::Region;
use crate::staged::{Stage, environment, lexical_read};
use crate::staged::slot_store::{gate_open, store_word};
use crate::typed_ir::Type;
use crate::codes::{CALLEE_CAPTURE_ROW, CAPTURE_ROW, CAPTURE_SET_ROW, ENV_ROW, ENV_SET_ROW, GLOBAL_READ_ROW, GLOBAL_STORE_ROW, LEXICAL_READ_ROW, LEXICAL_WRITE_ROW};
use crate::heap_rows::{hop, word_at};

/// The depth-0 capture slot (holes 1 and 2) into the destination; a hit continues at TAKEN, as every row that may
/// have a helper step behind it does (`hop`).
#[inline(always)]
fn capture_row<const D: u8>(s: S, payload: usize) -> Out {
    let mut stage = StencilStage::from_hole(HOLE_A);
    let values = word_at(payload, JS_STEP_CAPTURE_VALUES);
    let len = word_at(payload, JS_STEP_CAPTURE_LEN);
    let word = need!(s, environment::dense_capture(&mut stage, values, len, 0, ()));
    hop(deliver::<D>(s, word))
}

/// A store of the value operand (hole 0) into the same capture slot (holes 1 and 2), where the
/// run's slots admit every direct store (`environment::dense_capture_store`); a hit continues at TAKEN.
#[inline(always)]
fn capture_set_row<const A: u8>(s: S, payload: usize) -> Out {
    let mut stage = StencilStage::from_hole(HOLE_A);
    let slots = (word_at(payload, JS_STEP_CAPTURE_VALUES), word_at(payload, JS_STEP_CAPTURE_LEN), word_at(payload, JS_STEP_CAPTURE_STORES));
    need!(s, environment::dense_capture_store(&mut stage, slots, 0, tagged_place::<A, 0>(&s), ()));
    Out::Taken(s)
}

/// The slot (holes 1 and 2) of the environment window's storage the payload names, into the destination: the
/// storage's own dense run, whether the window holds its locals in place or an environment's slots
/// (`environment::environment_slot`). A hit continues at TAKEN, as a capture row does.
#[inline(always)]
fn env_row<const D: u8>(s: S, payload: usize) -> Out {
    let mut stage = StencilStage::from_hole(HOLE_A);
    let word = need!(s, environment::environment_slot(&mut stage, word_at(payload, JS_STEP_ENV_STORAGE), 0, ()));
    hop(deliver::<D>(s, word))
}

/// A store of the value operand (hole 0) into the same slot of an initialized binding. Its caller proved the value
/// no heap reference (`base_region::steps::plan_fast`), so the environment's write barrier has nothing to record.
#[inline(always)]
fn env_set_row<const A: u8>(s: S, payload: usize) -> Out {
    let mut stage = StencilStage::from_hole(HOLE_A);
    need!(s, environment::environment_slot_store(&mut stage, word_at(payload, JS_STEP_ENV_STORAGE), 0, tagged_place::<A, 0>(&s), ()));
    Out::Taken(s)
}

/// A definition of the value operand (hole 0) into the same slot, whatever initialized or uninitialized binding it holds.
#[inline(always)]
fn env_define_row<const A: u8>(s: S, payload: usize) -> Out {
    let mut stage = StencilStage::from_hole(HOLE_A);
    need!(s, environment::environment_slot_define(&mut stage, word_at(payload, JS_STEP_ENV_STORAGE), 0, tagged_place::<A, 0>(&s), ()));
    Out::Taken(s)
}

/// The values of the global object's shape a row's layout proof counts: the one slot the row names,
/// which the stage's per-site byte offset selects. The shape itself is held by a dependency (the
/// global layout fact), so the row proves only the record's representation.
const GLOBAL_SLOT_COUNT: usize = 1;

/// The layout of the global object `id` names in the record table at the payload: `None` where the
/// table is absent or the object's values are not behind its extras in the arena.
#[inline(always)]
fn global_layout(stage: &mut StencilStage, payload: usize, id: u64) -> Option<Layout<u64, bool>> {
    let records = word_at(payload, JS_STEP_OBJECT_RECORD_BASE) as u64;
    if records == 0 {
        return None;
    }
    let record = object::object_record_at(stage, records, id);
    let inline = object::inline_storage(stage, record);
    object::arena_extras_layout(stage, record, inline, GLOBAL_SLOT_COUNT, ())
}

/// The global object's slot (hole 2: its byte offset) read into the destination.
#[inline(always)]
fn global_read_row<const D: u8>(s: S, payload: usize) -> Out {
    let header = word_at(payload, JS_STEP_PROPERTY_BUFFER) as usize;
    let mut stage = StencilStage::with_values(HOLE_B, word_at(header, PROPERTY_BUFFER_BASE));
    let layout = need!(s, global_layout(&mut stage, payload, hole::<HOLE_A>() as u64));
    deliver::<D>(s, object::read_slot(&mut stage, layout, 0, crate::typed_ir::Representation::Tagged))
}

/// The first operand stored to the global object's slot (hole 0: the record id), through the store
/// gate where `GATED`: a value that may be a heap reference joins the barrier's log, and a store
/// the property arena observes (`slots_tracked`) is the VM's.
#[inline(always)]
fn global_store_row<const A: u8, const GATED: bool>(s: S, payload: usize) -> Out {
    let header = word_at(payload, JS_STEP_PROPERTY_BUFFER) as usize;
    let mut stage = StencilStage::storing(HOLE_B, word_at(header, PROPERTY_BUFFER_BASE), payload);
    let id = hole::<0>() as u64;
    let value = tagged_place::<A, HOLE_A>(&s);
    let layout = need!(s, global_layout(&mut stage, payload, id));
    let Slots::Flat(base) = layout.slots else { return Out::Exit(s) };
    if GATED {
        need!(s, gate_open(&mut stage, id, (value, Type::ANY), ()));
    }
    let offset = stage.site(0);
    let address = stage.add_imm(base, offset);
    store_word(&mut stage, address, value, object::Rep::Tagged);
    Out::Next(s)
}

/// The global lexical binding at the address in hole 1 (`staged::lexical_read`) into the destination; a hit continues at NEXT, a binding in
/// its dead zone leaves.
#[inline(always)]
fn lexical_read_row<const D: u8>(s: S) -> Out {
    let mut stage = StencilStage::from_hole(0);
    deliver::<D>(s, need!(s, lexical_read(&mut stage, hole64::<HOLE_A>(), ())))
}

/// The capture slot of an inlined callee at the address in hole 1: the closure the site guards or holds is fixed, so its
/// environment's slot is too, and the row is the staged read every capture row makes of its slot (`environment::capture_at`).
#[inline(always)]
fn callee_capture_row<const D: u8>(s: S) -> Out {
    let mut stage = StencilStage::from_hole(0);
    deliver::<D>(s, need!(s, environment::capture_at(&mut stage, hole64::<HOLE_A>(), ())))
}

/// The first operand (hole 1) stored to the initialized global lexical binding at the address in hole 2. No code holds the binding's value
/// (the cell is `Mutable`) and the record is a root the collector scans whole, so the store is one write.
#[inline(always)]
fn lexical_write_row<const A: u8>(s: S) -> Out {
    let mut stage = StencilStage::from_hole(0);
    let (address, value) = (hole64::<HOLE_B>(), tagged_place::<A, HOLE_A>(&s));
    need!(s, lexical_read(&mut stage, address, ()));
    stage.store_u64(Region::Cells.mutable(), address, 0, value);
    Out::Next(s)
}

/// The environment row of kind `KIND`.
#[inline(always)]
pub(crate) fn run<const KIND: u8, const ENTRY: bool, const D: u8, const A: u8>(s: S, payload: usize) -> Out {
    match KIND {
        CAPTURE_ROW => capture_row::<D>(s, payload),
        CAPTURE_SET_ROW => capture_set_row::<A>(s, payload),
        ENV_ROW => env_row::<D>(s, payload),
        ENV_SET_ROW => env_set_row::<A>(s, payload),
        GLOBAL_READ_ROW => global_read_row::<D>(s, payload),
        GLOBAL_STORE_ROW => global_store_row::<A, ENTRY>(s, payload),
        LEXICAL_READ_ROW => lexical_read_row::<D>(s),
        LEXICAL_WRITE_ROW => lexical_write_row::<A>(s),
        CALLEE_CAPTURE_ROW => callee_capture_row::<D>(s),
        _ => env_define_row::<A>(s, payload),
    }
}
