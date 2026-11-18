//! How an environment window specializes the records that read its locals or make its closures
//! (`specialize_environment_window`): which records it takes and the handler each gets.
use super::*;

/// The handlers derivation gives the records an environment window specializes, which no other
/// pass changed, beside the local rows' generated handlers (`is_row`).
pub(super) fn plain_window_handlers<H: Host>() -> [Handler; 10] {
    [
        lane_clone_env::<H>,
        lane_load_local::<H>,
        lane_load_frame_local::<H, true>,
        lane_load_frame_local::<H, false>,
        lane_store_local::<H>,
        lane_store_frame_local::<H, true>,
        lane_store_frame_local::<H, false>,
        lane_initialize_tdz::<H>,
        lane_make_closure::<H>,
        lane_exit::<H>,
    ]
}

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
        Class::CloneEnv => 0,
        Class::LoadLocal | Class::LoadEnvLocal | Class::StoreLocal
        | Class::StoreEnvLocal | Class::InitializeTdz => instruction.local_slot() as u32,
        // A function's Annex B store to its `var` binding is a store to the slot
        // (`Function::window_reads_locals`).
        Class::StoreVarBinding if function.parent.is_some() => instruction.local_slot() as u32,
        _ if is_plain_local(instruction) => instruction.local_slot() as u32,
        _ => return WindowRecord::Keeps,
    })
}

/// Whether `instruction` decodes as a plain local load or store; a record the decode rejects (a slot
/// past u16) keeps the opcode test.
fn is_plain_local(instruction: WideInstruction) -> bool {
    match crate::vm::compiled::decoded(instruction) {
        Some(crate::staged::Operation::LoadLocalPlain { .. } | crate::staged::Operation::StoreLocalPlain { .. }) => true,
        Some(_) => false,
        None => matches!(instruction.op().class(), Class::LoadLocalPlain | Class::StoreLocalPlain),
    }
}

/// The handler of the window record of `op` on local slot `own`: the mapped parameter's, the program
/// root's, one that leaves for a store to the function's own name, the scoped one of a function
/// that resolves names at run time (`scoped`), else the plain window's.
pub(super) fn window_local_handler<H: Host>(function: &Function, op: Op, own: usize, scoped: bool) -> Handler {
    let class = op.class();
    let environment = !op.captures_environment() && function.local_slot_uses_environment(own);
    if let Some(mapped) = mapped_windows::mapped_parameter_handler::<H>(function, class, own) {
        return mapped;
    }
    // A program's root, whose slots may stand for bindings outside its window (`root_windows`).
    if function.parent.is_none() && !scoped && let Some(root) = root_windows::root_local_handler::<H>(class, environment) {
        return root;
    }
    // A store to the function's own name is ignored or throws (`Function::strict`), which
    // the general step decides.
    if function.is_self_binding_slot(own) && matches!(class, Class::StoreLocal | Class::StoreLocalPlain | Class::StoreEnvLocal) {
        return lane_exit::<H>;
    }
    if scoped && let Some(scoped_handler) = scoped_window_handler::<H>(class, environment) {
        return scoped_handler;
    }
    plain_window_handler::<H>(class, environment)
}

/// The handler a function that resolves names at run time gives a closure or an environment store.
fn scoped_window_handler<H: Host>(class: Class, environment: bool) -> Option<Handler> {
    match (class, environment) {
        (Class::MakeClosure, _) => Some(scoped_names::scoped_closure::<H>()),
        (Class::StoreEnvLocal, true) => Some(scoped_names::scoped_store_local::<H, true>()),
        (Class::StoreEnvLocal, false) => Some(scoped_names::scoped_store_local::<H, false>()),
        _ => None,
    }
}

/// The handler of an environment window's record of `class`, `environment` where the local lives
/// in the window's environment once a closure made one.
fn plain_window_handler<H: Host>(class: Class, environment: bool) -> Handler {
    match class {
        Class::MakeClosure => lane_make_window_closure::<H>,
        Class::CloneEnv => lane_clone_window_env::<H>,
        Class::LoadLocal | Class::LoadLocalPlain | Class::LoadEnvLocal if environment => lane_load_window_local::<H, true>,
        Class::LoadLocal | Class::LoadLocalPlain | Class::LoadEnvLocal => lane_load_window_local::<H, false>,
        Class::InitializeTdz if environment => lane_initialize_window_tdz::<H, true>,
        Class::InitializeTdz => lane_initialize_window_tdz::<H, false>,
        _ if environment => lane_store_window_local::<H, true>,
        _ => lane_store_window_local::<H, false>,
    }
}
