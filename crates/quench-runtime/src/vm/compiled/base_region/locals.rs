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
//! * `LoadEnvLocal` of a binding the frame's own environment holds is the read of that environment's dense view
//!   (`ENV_ROW`, `staged::environment::dense_capture`), which a body's entry sets and its helpers reload
//!   (`Vm::refresh_body_bases`); a slot outside the view, a dead zone or a frame with no view leaves into the helper
//!   step. `StoreEnvLocal` of such an assignment is the write of the same slot (`ENV_SET_ROW`,
//!   `staged::environment::dense_capture_store`) where the view admits every direct store, whose barrier the admission
//!   took once (`EnvView::after`); else the helper step.
use super::*;
use crate::bytecode::{Function, InstructionField};

/// Whether `slot` of `function` (the function with index `id`) is a local only its frame owns.
fn frame_owned(function: &Function, id: u32, slot: usize) -> bool {
    id != crate::vm::ROOT_FUNCTION_ID && function.plain_local_slot_is_safe(slot) && !function.local_slot_uses_environment(slot)
}

/// Whether `slot` of `function` (index `id`) is a binding the frame's own environment holds that a direct view of it
/// answers, as the general tier's direct access does (`general_capture::EnvLocalSite::direct`): the function
/// resolves no names at run time and no mapped `arguments` aliases the slot.
fn env_owned(function: &Function, id: u32, slot: usize) -> bool {
    id != crate::vm::ROOT_FUNCTION_ID && function.local_slot_uses_environment(slot) && !function.has_dynamic_scope_access()
        && crate::vm::arguments::mapped_arguments_slot(function, slot).is_none()
}

/// The payload byte offset of the frame's environment view, which the environment rows name.
const ENV_VIEW: u64 = std::mem::offset_of!(super::super::JsStepDispatch, env_view) as u64;

/// The slot of the environment-local instruction `instruction` of `function` (index `id`) where the frame's environment
/// view answers it (`env_owned`).
fn env_owned_slot(function: &Function, id: u32, instruction: WideInstruction) -> Option<u16> {
    u16::try_from(instruction.local_slot()).ok().filter(|slot| env_owned(function, id, usize::from(*slot)))
}

/// Whether `instruction`, a `StoreEnvLocal`, is the direct store the view admits
/// (`general_capture::EnvLocalSite::direct`): it assigns (an initializing store of an environment-owned slot cannot tell
/// its dead zone from a slot the environment took), copies its value to no register, is no self binding's and has no
/// result flag.
fn env_store(function: &Function, instruction: WideInstruction) -> bool {
    instruction.optional_register_b().is_none() && instruction.boolean_field(InstructionField::C) == Some(false)
        && instruction.result_flags() == 0 && !function.is_self_binding_slot(instruction.local_slot())
}

/// The fast row of the local instruction `instruction` of `function` (index `id`), if it has one.
pub(super) fn fast_row(function: &Function, id: u32, instruction: WideInstruction) -> Option<Decoded> {
    let owned = |slot: usize| u16::try_from(slot).ok().filter(|_| frame_owned(function, id, slot));
    let store = |slot: u16, value: Source| Decoded { a: Some(value), ..Decoded::step(Step::Fast(Heap::StoreLocal(slot)), None) };
    let checked = |slot: u16, value: Source| Decoded { a: Some(value), ..Decoded::step(Step::Fast(Heap::StoreChecked(slot)), None) };
    match instruction.op().class() {
        Class::LoadEnvLocal if !instruction.writes_numeric_local() => {
            env_owned_slot(function, id, instruction).map(|slot| Decoded { hole: ENV_VIEW, ..Decoded::step(Step::Fast(Heap::EnvLocal(slot)), Some(instruction.result_register())) })
        }
        Class::StoreEnvLocal if env_store(function, instruction) => {
            env_owned_slot(function, id, instruction).map(|slot| Decoded { a: Some(Source::Register(instruction.register_a())), hole: ENV_VIEW, ..Decoded::step(Step::Fast(Heap::EnvStore(slot)), None) })
        }
        Class::LoadLocal if !instruction.writes_numeric_local() => {
            owned(instruction.local_slot()).map(|slot| Decoded::step(Step::Fast(Heap::LoadChecked(slot)), Some(instruction.result_register())))
        }
        Class::StoreLocal if instruction.optional_register_b().is_none() && instruction.boolean_field(InstructionField::C) == Some(false) => {
            owned(instruction.local_slot()).map(|slot| checked(slot, Source::Register(instruction.register_a())))
        }
        Class::InitializeTdz => owned(instruction.local_slot()).map(|slot| store(slot, Source::Value(Value::DELETED.to_bits()))),
        Class::StoreLocal if instruction.optional_register_b().is_none() && instruction.boolean_field(InstructionField::C) == Some(true) => {
            owned(instruction.local_slot()).map(|slot| store(slot, Source::Register(instruction.register_a())))
        }
        _ => None,
    }
}
