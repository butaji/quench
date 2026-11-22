//! The frame-local instructions a body makes as a fast row over the activation's locals
//! (`quench-bjit`'s `LOAD_LOCAL_ROW`/`STORE_LOCAL_ROW`, which read and write the locals a body run
//! publishes: `JsStepDispatch::native_locals`), with the interpreter's own step behind it as the slow
//! arm (`Step::Fast`): a slot the frame always owns needs no ownership proof per use, only the
//! published locals the row finds or leaves for the helper.
//!
//! A slot is the frame's own where its function is no root body (whose slots may stand for global or
//! imported bindings), the slot is a plain local (`Function::plain_local_slot_is_safe`: no self binding,
//! no mapped `arguments`) and the function's environment never holds it
//! (`Function::local_slot_uses_environment`, a fact of the function, not of a run), so the
//! environment-owned and mapped paths of `Vm::load_local_binding` and `Vm::store_with_binding` never
//! apply to it. Then:
//!
//! * `InitializeTdz` writes the uninitialized marker (`Value::DELETED`) to the slot: a store of a constant.
//! * `StoreLocal` that initializes its binding (a declaration's own store: no temporal dead zone to
//!   check) and defines no register beside it writes the slot: a store of the register. One that does not
//!   initialize reads the slot first (`staged::write_initialized_local`): a binding in its temporal dead
//!   zone holds the deleted marker, and the row leaves into the helper step, which raises.
//! * `LoadLocal` is the read that leaves for the same marker (`staged::read_initialized_local`).
//!
//! A body also runs as the lane activation of a function with an environment window
//! (`dispatch_fast::activate_environment_in_lane`), which keeps the locals its closures capture in the storage its
//! slots word names, in place among the locals or in the environment a closure made. Its body reads that
//! storage through the payload (`JsStepDispatch::environment_storage`, set at each entry), by the rows of
//! `staged::environment::environment_slot`, `environment_slot_store` and `environment_slot_define` (`ENV_ROW`,
//! `ENV_SET_ROW`, `ENV_DEFINE_ROW`): a read of an initialized binding, an assignment of one, and the opening of a dead
//! zone and a declaration's first value, which define it. The storage is the window's own, so a store needs no barrier of its
//! own beyond the interpreter's (the environment is young, or a resumed generator's, which joined the remembered
//! set when its window opened). A body that runs in a frame holds storage with no slots, which every such row
//! misses at; a loop region has no such storage and makes no such row.
use super::*;
use quench_runtime::bytecode::{Function, InstructionField};

/// Whether `slot` of `function` (the function with index `id`) is a local only its frame owns.
fn frame_owned(function: &Function, id: u32, slot: usize) -> bool {
    id != quench_runtime::vm::ROOT_FUNCTION_ID && function.plain_local_slot_is_safe(slot) && !function.local_slot_uses_environment(slot)
}

/// Whether `slot` of `function` (index `id`) lives in the storage of an activation's environment window, as the
/// window handlers' `ENVIRONMENT` flavor reads it (`specialize_environment_window`): the function resolves no names at
/// run time and no mapped `arguments` aliases the slot.
pub(crate) fn env_owned(function: &Function, id: u32, slot: usize) -> bool {
    id != quench_runtime::vm::ROOT_FUNCTION_ID && function.local_slot_uses_environment(slot) && !function.has_dynamic_scope_access()
        && quench_runtime::vm::arguments::mapped_arguments_slot(function, slot).is_none()
}

/// The fast row of the access of an environment-owned slot by `instruction`, if it has one: a read, an assignment
/// or a declaration's first value (a definition, which tells its dead zone from a slot another environment shares:
/// `environment_slot_define`), or the opening of its dead zone (`InitializeTdz`, the hole defined). A store copies its
/// value to no register, is no self binding's and has no result flag.
fn env_row(function: &Function, id: u32, instruction: WideInstruction) -> Option<Decoded> {
    let owned = || u16::try_from(instruction.local_slot()).ok().filter(|slot| env_owned(function, id, usize::from(*slot)));
    let plain_store = || instruction.optional_register_b().is_none() && instruction.result_flags() == 0
        && !function.is_self_binding_slot(instruction.local_slot());
    let define = |slot: u16, value: Source| Decoded { a: Some(value), ..Decoded::step(Step::Fast(Heap::EnvDefine(slot)), None) };
    use quench_runtime::staged::{protocol_owned::OwnedVarStep, Operation, Protocol};
    match crate::compiled::decoded(instruction) {
        Some(Operation::Protocol { step: Protocol::Owned(OwnedVarStep::LoadLocal { .. }), .. }) | Some(Operation::LoadEnvLocal { .. })
            if !instruction.writes_numeric_local() => {
            owned().map(|slot| Decoded::step(Step::Fast(Heap::EnvLocal(slot)), Some(instruction.result_register())))
        }
        Some(Operation::Protocol { step: Protocol::Owned(OwnedVarStep::StoreLocal { .. }), .. }) | Some(Operation::StoreEnvLocal { .. })
            if plain_store() => {
            let value = Source::Register(instruction.register_a());
            match instruction.boolean_field(InstructionField::C)? {
                false => owned().map(|slot| Decoded { a: Some(value), ..Decoded::step(Step::Fast(Heap::EnvStore(slot)), None) }),
                true => owned().map(|slot| define(slot, value)),
            }
        }
        Some(Operation::Protocol { step: Protocol::Owned(OwnedVarStep::InitializeTdz { .. }), .. }) =>
            owned().map(|slot| define(slot, Source::Value(Value::DELETED.to_bits()))),
        _ => None,
    }
}

/// The fast row of the local instruction `instruction` of `function` (index `id`), if it has one: a frame-owned
/// slot's, or in a body an environment window's (`env_row`).
pub(crate) fn fast_row(function: &Function, id: u32, instruction: WideInstruction, ending: Ending) -> Option<Decoded> {
    frame_row(function, id, instruction).or_else(|| env_row(function, id, instruction).filter(|_| ending == Ending::Body))
}

/// The fast row of a frame-owned slot's access by `instruction` of `function` (index `id`), if it has one.
fn frame_row(function: &Function, id: u32, instruction: WideInstruction) -> Option<Decoded> {
    let owned = |slot: usize| u16::try_from(slot).ok().filter(|_| frame_owned(function, id, slot));
    let store = |slot: u16, value: Source| Decoded { a: Some(value), ..Decoded::step(Step::Fast(Heap::StoreLocal(slot)), None) };
    let checked = |slot: u16, value: Source| Decoded { a: Some(value), ..Decoded::step(Step::Fast(Heap::StoreChecked(slot)), None) };
    use quench_runtime::staged::{protocol_owned::OwnedVarStep, Operation, Protocol};
    match crate::compiled::decoded(instruction) {
        Some(Operation::Protocol { step: Protocol::Owned(OwnedVarStep::LoadLocal { .. }), .. }) if !instruction.writes_numeric_local() => {
            owned(instruction.local_slot()).map(|slot| Decoded::step(Step::Fast(Heap::LoadChecked(slot)), Some(instruction.result_register())))
        }
        Some(Operation::Protocol { step: Protocol::Owned(OwnedVarStep::StoreLocal { .. }), .. })
            if instruction.optional_register_b().is_none() && instruction.boolean_field(InstructionField::C) == Some(false) => {
            owned(instruction.local_slot()).map(|slot| checked(slot, Source::Register(instruction.register_a())))
        }
        Some(Operation::Protocol { step: Protocol::Owned(OwnedVarStep::InitializeTdz { .. }), .. }) =>
            owned(instruction.local_slot()).map(|slot| store(slot, Source::Value(Value::DELETED.to_bits()))),
        Some(Operation::Protocol { step: Protocol::Owned(OwnedVarStep::StoreLocal { .. }), .. })
            if instruction.optional_register_b().is_none() && instruction.boolean_field(InstructionField::C) == Some(true) => {
            owned(instruction.local_slot()).map(|slot| store(slot, Source::Register(instruction.register_a())))
        }
        _ => None,
    }
}
