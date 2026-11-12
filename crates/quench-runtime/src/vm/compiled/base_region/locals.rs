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
use super::*;
use crate::bytecode::{Function, InstructionField};

/// Whether `slot` of `function` (the function with index `id`) is a local only its frame owns.
fn frame_owned(function: &Function, id: u32, slot: usize) -> bool {
    id != crate::vm::ROOT_FUNCTION_ID && function.plain_local_slot_is_safe(slot) && !function.local_slot_uses_environment(slot)
}

/// The fast row of the local instruction `instruction` of `function` (index `id`), if it has one.
pub(super) fn fast_row(function: &Function, id: u32, instruction: WideInstruction) -> Option<Decoded> {
    let owned = |slot: usize| u16::try_from(slot).ok().filter(|_| frame_owned(function, id, slot));
    let store = |slot: u16, value: Source| Decoded { a: Some(value), ..Decoded::step(Step::Fast(Heap::StoreLocal(slot)), None) };
    let checked = |slot: u16, value: Source| Decoded { a: Some(value), ..Decoded::step(Step::Fast(Heap::StoreChecked(slot)), None) };
    match instruction.op().class() {
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
