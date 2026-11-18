//! How an environment window specializes the records that read its locals or make its closures
//! (`specialize_environment_window`): which records it takes and the handler each gets. A local
//! record of a plain slot takes its generated row with `FLAG_WINDOW` (`lane_frame::row_frame` reads
//! the window's locals there); any other local record takes its miss, which leaves to the general path.
use super::*;

/// What an environment window does with one record.
pub(super) enum WindowRecord {
    /// The record reads no local and makes no closure: it keeps its handler.
    Keeps,
    /// A direct eval, which runs in a window activation (`lane_eval`); in a frame's run it leaves.
    Eval,
    /// A record of the local slot, or the closure or environment clone, `.0` names.
    Local(u32),
}

pub(super) fn window_record(function: &Function, instruction: WideInstruction) -> WindowRecord {
    let op = instruction.op();
    if op.class().eq(Class::Call) && instruction.direct_eval() {
        return WindowRecord::Eval;
    }
    WindowRecord::Local(match op.class() {
        Class::MakeClosure => instruction.imm(),
        // A clone's own immediate is its plan, which the row reads (`staged::Operation::CloneEnv`).
        Class::CloneEnv => instruction.imm(),
        Class::LoadLocal | Class::LoadLocalPlain | Class::LoadEnvLocal | Class::StoreLocal | Class::StoreLocalPlain
        | Class::StoreEnvLocal | Class::InitializeTdz => crate::staged::window_slot(instruction.imm()) as u32,
        // A function's Annex B store to its `var` binding is a store to the slot
        // (`Function::window_reads_locals`).
        Class::StoreVarBinding if function.parent.is_some() => crate::staged::window_slot(instruction.imm()) as u32,
        _ => return WindowRecord::Keeps,
    })
}

/// The handler of the window record of `op` on local slot `own`, or `None` where the record keeps its
/// own (a `LoadEnvLocal` or `StoreEnvLocal` of a slot the window does not hold in its locals, which its
/// environment row answers and misses from in a window run). `scoped` marks a function that resolves
/// names at run time, whose environment stores go through its scope first (`scoped_names`).
pub(super) fn window_local_handler<H: Host>(function: &Function, op: Op, own: usize, scoped: bool) -> Option<Handler> {
    // A clone is its generated row, which clones the window's environment (`CallOut::CloneEnv`).
    if op as u16 == crate::staged::CLONE_ENV_ROW {
        return Some(lane_row::<H, { crate::staged::CLONE_ENV_ROW }, ACC_NONE, FLAG_UNREAD> as Handler);
    }
    let class = op.class();
    if class == Class::MakeClosure {
        return Some(if scoped { super::lane_frame::closure_row::<H, { super::lane_frame::FLAG_SCOPE }>() } else { super::lane_frame::closure_row::<H, { super::lane_frame::FLAG_WINDOW_CLOSURE }>() });
    }
    // A store to the function's own name is ignored or throws (`Function::strict`), which
    // the general step decides.
    if function.is_self_binding_slot(own) && matches!(class, Class::StoreLocal | Class::StoreLocalPlain | Class::StoreEnvLocal) {
        return Some(lane_exit::<H>);
    }
    // A function that resolves names at run time may store to a `with` object's binding instead of the
    // local: its environment store takes the scope first (`scoped_names::lane_store_scoped_local_miss`).
    let environment = !op.captures_environment() && function.local_slot_uses_environment(own);
    if scoped && class == Class::StoreEnvLocal {
        return Some(if environment { super::lane_frame::scoped_local_row::<H, { super::lane_frame::FLAG_SCOPE_ENVIRONMENT }>() } else { super::lane_frame::scoped_local_row::<H, { super::lane_frame::FLAG_SCOPE }>() });
    }
    let environment_slot = class == Class::LoadEnvLocal || class == Class::StoreEnvLocal;
    // A plain slot of a non-root function: the frame's own local, which the window holds too
    // (`plain_frame_local`). A program's root is not one: its slots may stand for bindings outside
    // its window (`Vm::root_binding`).
    let plain = function.parent.is_some() && plain_frame_local(function, own);
    if !plain {
        // The record leaves for the general store, load or initialization (`row_miss`), which takes the
        // binding rules of the activation it runs in. An environment slot keeps its own row.
        return if environment_slot { None } else { Some(row_miss::<H, FLAG_UNREAD>(op)) };
    }
    Some(match class {
        Class::LoadLocal | Class::LoadEnvLocal => lane_row::<H, { crate::staged::LOCAL_LOAD_ROW }, ACC_NONE, FLAG_WINDOW> as Handler,
        Class::LoadLocalPlain => lane_row::<H, { opc::LoadLocalPlain }, ACC_NONE, FLAG_WINDOW> as Handler,
        Class::StoreLocal | Class::StoreEnvLocal => lane_row::<H, { crate::staged::LOCAL_STORE_ROW }, ACC_NONE, FLAG_WINDOW> as Handler,
        Class::StoreLocalPlain => lane_row::<H, { opc::StoreLocalPlain }, ACC_NONE, FLAG_WINDOW> as Handler,
        Class::InitializeTdz => lane_row::<H, { crate::staged::LOCAL_INITIALIZE_ROW }, ACC_NONE, FLAG_WINDOW> as Handler,
        _ => return None,
    })
}
