//! How an environment window specializes the records that read its locals or make its closures
//! (`specialize_environment_window`): which records it takes and the handler each gets. A local
//! record of a plain slot takes its generated row with `FLAG_WINDOW` (`lane_frame::row_frame` reads
//! the window's locals there); any other local record takes its miss, which leaves to the general path.
use super::*;
// merge: theirs window_record/window_local_handler taken whole (staged queries, op-based); HEAD's class-match helpers and window handlers are dropped with the hand fast paths. meaning_fast import dropped as unused.

/// What an environment window does with one record.
pub(crate) enum WindowRecord {
    /// The record reads no local and makes no closure: it keeps its handler.
    Keeps,
    /// A direct eval, which runs in a window activation (`lane_eval`); in a frame's run it leaves.
    Eval,
    /// A record of the local slot, or the closure or environment clone, `.0` names.
    Local(u32),
}

pub(crate) fn window_record(function: &Function, instruction: WideInstruction) -> WindowRecord {
    let op = instruction.op();
    if quench_runtime::staged::calls(op) && instruction.direct_eval() {
        return WindowRecord::Eval;
    }
    // A closure's immediate names its function; a clone's is its plan, which the row reads
    // (`staged::Operation::CloneEnv`).
    if quench_runtime::staged::makes_closure(op) || quench_runtime::staged::clones_environment(op) {
        return WindowRecord::Local(instruction.imm());
    }
    // A function's Annex B store to its `var` binding is a store to the slot
    // (`Function::window_reads_locals`).
    let slot_access = quench_runtime::staged::accesses_local_slot(op) || (quench_runtime::staged::stores_var_binding(op) && function.parent.is_some());
    if slot_access {
        return WindowRecord::Local(quench_runtime::staged::window_slot(instruction.imm()) as u32);
    }
    WindowRecord::Keeps
}

/// The handler of the window record of `op` on local slot `own`, or `None` where the record keeps its
/// own (a `LoadEnvLocal` or `StoreEnvLocal` of a slot the window does not hold in its locals, which its
/// environment row answers and misses from in a window run). `scoped` marks a function that resolves
/// names at run time, whose environment stores go through its scope first (`scoped_names`).
pub(crate) fn window_local_handler(function: &Function, op: Op, own: usize, scoped: bool) -> Option<Handler> {
    // A clone is its generated row, which clones the window's environment (`CallOut::CloneEnv`).
    if op as u16 == quench_runtime::staged::CLONE_ENV_ROW {
        return Some(generated_row!(quench_runtime::staged::CLONE_ENV_ROW, ACC_NONE, FLAG_UNREAD) as Handler);
    }
    if quench_runtime::staged::makes_closure(op) {
        return Some(if scoped { super::lane_frame::closure_row::<{ super::lane_frame::FLAG_SCOPE }>() } else { super::lane_frame::closure_row::<{ super::lane_frame::FLAG_WINDOW_CLOSURE }>() });
    }
    // A store to the function's own name is ignored or throws (`Function::strict`), which
    // the general step decides.
    if function.is_self_binding_slot(own) && quench_runtime::staged::stores_local_slot(op) {
        return Some(exit_lane);
    }
    // A function that resolves names at run time may store to a `with` object's binding instead of the
    // local: its environment store takes the scope first (the general step).
    let environment = !op.captures_environment() && function.local_slot_uses_environment(own);
    if scoped && quench_runtime::staged::stores_environment_slot(op) {
        return Some(if environment { super::lane_frame::scoped_local_row::<{ super::lane_frame::FLAG_SCOPE_ENVIRONMENT }>() } else { super::lane_frame::scoped_local_row::<{ super::lane_frame::FLAG_SCOPE }>() });
    }
    let environment_slot = quench_runtime::staged::addresses_environment_slot(op);
    // A plain slot of a non-root function: the frame's own local, which the window holds too
    // (`plain_frame_local`). A program's root is not one: its slots may stand for bindings outside
    // its window (`Vm::root_binding`).
    let plain = function.parent.is_some() && plain_frame_local(function, own);
    if !plain {
        // The record leaves for the general store, load or initialization (`row_miss`), which takes the
        // binding rules of the activation it runs in. An environment slot keeps its own row.
        return if environment_slot { None } else { Some(row_miss::<FLAG_UNREAD>(op)) };
    }
    let row = quench_runtime::staged::local_access_row(op)?;
    Some(match row {
        quench_runtime::staged::LOCAL_LOAD_ROW => generated_row!(quench_runtime::staged::LOCAL_LOAD_ROW, ACC_NONE, FLAG_WINDOW) as Handler,
        quench_runtime::staged::LOAD_LOCAL_PLAIN_ROW => generated_row!(quench_runtime::staged::LOAD_LOCAL_PLAIN_ROW, ACC_NONE, FLAG_WINDOW) as Handler,
        quench_runtime::staged::LOCAL_STORE_ROW => generated_row!(quench_runtime::staged::LOCAL_STORE_ROW, ACC_NONE, FLAG_WINDOW) as Handler,
        quench_runtime::staged::STORE_LOCAL_PLAIN_ROW => generated_row!(quench_runtime::staged::STORE_LOCAL_PLAIN_ROW, ACC_NONE, FLAG_WINDOW) as Handler,
        quench_runtime::staged::LOCAL_INITIALIZE_ROW => generated_row!(quench_runtime::staged::LOCAL_INITIALIZE_ROW, ACC_NONE, FLAG_WINDOW) as Handler,
        _ => return None,
    })
}
