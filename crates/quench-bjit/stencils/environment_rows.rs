//! The environment rows: a capture slot, a slot of the environment window's storage, and the
//! global object's slot, each read, stored or defined. The capture, environment and global-name
//! rows build the staged `Operation` the interpreter decodes for their instruction and run
//! `staged::step_names` over the stencil frame `Places` (`places.rs`, `places_environment.rs`). The
//! lexical rows run the same names over `PlannedPlaces` with the binding's address (`run_lexical`).

use crate::words::{hop, word_at};
use crate::layout::{JS_STEP_PROPERTY_BUFFER, PROPERTY_BUFFER_BASE};
use crate::numeric::{HOLE_A, HOLE_B, Out, S, deliver};
use crate::places::{DEST, LEFT, Places, settle, tagged_place};
use crate::places_property::run_lexical;
use crate::stage::StencilStage;
use crate::staged::{Operation, Stage, StagedRecordSpace, environment, op_kind, step_names_of};
use crate::words::{hole, hole64};
use crate::codes::{CALLEE_CAPTURE_ROW, CAPTURE_ROW, CAPTURE_SET_ROW, ENV_ROW, ENV_SET_ROW, GLOBAL_READ_ROW, GLOBAL_STORE_ROW, LEXICAL_READ_ROW, LEXICAL_WRITE_ROW};

/// The depth-0 capture slot (the stage's operands are holes 1 and 2) into the destination; a hit
/// continues at TAKEN, as every row that may have a helper step behind it does (`hop`).
#[inline(always)]
fn capture_row<const D: u8>(s: S) -> Out {
    let mut stage = StencilStage::from_hole(HOLE_A);
    let mut frame = Places::new::<D, 0, 0>(s);
    let step = step_names_of::<{ op_kind::LOAD_CAPTURE }, _, _>(&mut stage, &mut frame, Operation::LoadCapture { destination: DEST, depth: 0, slot: 0, root: false });
    hop(settle(frame, step, Out::Next))
}

/// A store of the value operand (hole 0) into the same capture slot (holes 1 and 2), where the
/// run's slots admit every direct store; a hit continues at TAKEN. The value operand is the frame's
/// destination place, which this row instantiates as `A`'s place.
#[inline(always)]
fn capture_set_row<const A: u8>(s: S) -> Out {
    let mut stage = StencilStage::from_hole(HOLE_A);
    let mut frame = Places::new::<A, A, 0>(s);
    let step = step_names_of::<{ op_kind::STORE_CAPTURE }, _, _>(&mut stage, &mut frame, Operation::StoreCapture { source: DEST, depth: 0, slot: 0 });
    settle(frame, step, Out::Taken)
}

/// The slot (holes 1 and 2) of the environment window's storage into the destination: the
/// storage's own dense run, whether the window holds its locals in place or an environment's slots.
/// A hit continues at TAKEN, as a capture row does.
#[inline(always)]
fn env_row<const D: u8>(s: S) -> Out {
    let mut stage = StencilStage::from_hole(HOLE_A);
    let mut frame = Places::new::<D, 0, 0>(s);
    let step = step_names_of::<{ op_kind::LOAD_ENV_LOCAL }, _, _>(&mut stage, &mut frame, Operation::LoadEnvLocal { destination: DEST, slot: 0 });
    hop(settle(frame, step, Out::Next))
}

/// A store of the value operand (hole 0) into the same slot of an initialized binding. Its caller proved the value
/// no heap reference (`base_region::steps::plan_fast`), so the environment's write barrier has nothing to record.
#[inline(always)]
fn env_set_row<const A: u8>(s: S) -> Out {
    let mut stage = StencilStage::from_hole(HOLE_A);
    let mut frame = Places::new::<A, A, 0>(s);
    let step = step_names_of::<{ op_kind::STORE_ENV_LOCAL }, _, _>(&mut stage, &mut frame, Operation::StoreEnvLocal { source: DEST, slot: 0, result: None, initialize: false });
    settle(frame, step, Out::Taken)
}

/// A definition of the value operand (hole 0) into the same slot, whatever initialized or uninitialized binding it holds:
/// the staged `StoreEnvLocal` with `initialize`, which the frame defines (`environment_write`).
#[inline(always)]
fn env_define_row<const A: u8>(s: S) -> Out {
    let mut stage = StencilStage::from_hole(HOLE_A);
    let mut frame = Places::new::<A, A, 0>(s);
    let step = step_names_of::<{ op_kind::STORE_ENV_LOCAL }, _, _>(&mut stage, &mut frame, Operation::StoreEnvLocal { source: DEST, slot: 0, result: None, initialize: true });
    settle(frame, step, Out::Taken)
}

/// The global object's slot (hole 2: its byte offset) read into the destination, as the staged `LoadName`
/// with no binding fact about the name (`GlobalBinding::Dynamic`), whose global read the frame answers.
#[inline(always)]
fn global_read_row<const D: u8>(s: S, payload: usize) -> Out {
    let header = word_at(payload, JS_STEP_PROPERTY_BUFFER) as usize;
    let mut stage = StencilStage::with_values(HOLE_B, word_at(header, PROPERTY_BUFFER_BASE));
    let mut frame = Places::new::<D, 0, 0>(s);
    let step = step_names_of::<{ op_kind::LOAD_NAME }, _, _>(&mut stage, &mut frame, Operation::LoadName { destination: DEST, atom: 0, site: 0, receiver: None, proof: () });
    settle(frame, step, Out::Next)
}

/// The first operand stored to the global object's slot (hole 0: the record id), through the store
/// gate where `GATED`: a value that may be a heap reference joins the barrier's log, and a store
/// the property arena observes (`store_watch`) is the VM's. The gate is read-only, so it runs
/// before the staged `GlobalWrite`, which leaves at the same edge where the layout fails.
#[inline(always)]
fn global_store_row<const A: u8, const GATED: bool>(s: S, payload: usize) -> Out {
    let header = word_at(payload, JS_STEP_PROPERTY_BUFFER) as usize;
    let mut stage = StencilStage::storing(HOLE_B, word_at(header, PROPERTY_BUFFER_BASE), payload);
    if GATED {
        let open = stage.store_gate(StagedRecordSpace::Object, hole::<0>() as u64, tagged_place::<A, HOLE_A>(&s));
        need!(s, stage.guard(open, ()));
    } else {
        // An ungated value is immediate: it needs no log unless old records keep counts.
        let open = stage.immediate_store_open();
        need!(s, stage.guard(open, ()));
    }
    let mut frame = Places::new::<0, A, 0>(s);
    let step = step_names_of::<{ op_kind::GLOBAL_WRITE }, _, _>(&mut stage, &mut frame, Operation::GlobalWrite { source: LEFT, atom: 0, site: 0, initialize: false });
    settle(frame, step, Out::Next)
}

/// The global lexical binding at the address in hole 1 into the destination, as the staged `LoadName` whose binding is
/// `GlobalBinding::Lexical` (`PlannedPlaces::lexical`, the frame's `global_lexical`, `staged::lexical_read`); a hit
/// continues at NEXT, a binding in its dead zone leaves.
#[inline(always)]
fn lexical_read_row<const D: u8>(s: S) -> Out {
    let mut stage = StencilStage::from_hole(0);
    let mut frame = Places::new::<D, 0, 0>(s);
    let step = run_lexical::<{ op_kind::LOAD_NAME }>(&mut stage, &mut frame, hole64::<HOLE_A>(), Operation::LoadName { destination: DEST, atom: 0, site: 0, receiver: None, proof: () });
    settle(frame, step, Out::Next)
}

/// The capture slot of an inlined callee at the address in hole 1: the closure the site guards or holds is fixed, so its
/// environment's slot is too, and the row is the staged read every capture row makes of its slot (`environment::capture_at`).
#[inline(always)]
fn callee_capture_row<const D: u8>(s: S) -> Out {
    let mut stage = StencilStage::from_hole(0);
    deliver::<D>(s, need!(s, environment::capture_at(&mut stage, hole64::<HOLE_A>(), ())))
}

/// The first operand (hole 1) stored to the initialized global lexical binding at the address in hole 2, as the staged
/// `GlobalWrite` of that binding (`PlannedPlaces::global_write`): the binding is read live, then its cell is one write.
#[inline(always)]
fn lexical_write_row<const A: u8>(s: S) -> Out {
    let mut stage = StencilStage::from_hole(0);
    let mut frame = Places::new::<0, A, 0>(s);
    let step = run_lexical::<{ op_kind::GLOBAL_WRITE }>(&mut stage, &mut frame, hole64::<HOLE_B>(), Operation::GlobalWrite { source: LEFT, atom: 0, site: 0, initialize: false });
    settle(frame, step, Out::Next)
}

/// The environment row of kind `KIND`.
#[inline(always)]
pub(crate) fn run<const KIND: u8, const ENTRY: bool, const D: u8, const A: u8>(s: S, payload: usize) -> Out {
    match KIND {
        CAPTURE_ROW => capture_row::<D>(s),
        CAPTURE_SET_ROW => capture_set_row::<A>(s),
        ENV_ROW => env_row::<D>(s),
        ENV_SET_ROW => env_set_row::<A>(s),
        GLOBAL_READ_ROW => global_read_row::<D>(s, payload),
        GLOBAL_STORE_ROW => global_store_row::<A, ENTRY>(s, payload),
        LEXICAL_READ_ROW => lexical_read_row::<D>(s),
        LEXICAL_WRITE_ROW => lexical_write_row::<A>(s),
        CALLEE_CAPTURE_ROW => callee_capture_row::<D>(s),
        _ => env_define_row::<A>(s),
    }
}
