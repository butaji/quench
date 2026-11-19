//! The direct runs of the packed step helper (`vm_steps::lane_body_step_packed`). The helper decodes the
//! instruction's staged operation (`Operation::direct_helper`) and runs its direct arm, which reads the running
//! frame and declines, before any effect, where the general step decides. A declined row runs the general step
//! (`lane_body_step`) from its PC; a completed one skips the frame bookkeeping and base refreshes, which only
//! guest code or an allocation need. Each arm is the one `Vm::step` runs (`step_index_get`, `step_index_set`,
//! `write_binary_result`), so both paths share their semantics.

use super::*;
use super::super::compiled::LANE_CALL_COMPLETED;
use super::super::field_cache::FieldCacheProbe;
use crate::bytecode::{Atom, FieldBase, FieldLookup, Operand};
use crate::staged::{DecodeContext, Operation};

/// The staged operation the packed helper runs directly, decoded from `instruction`, where it has a direct arm.
pub(in crate::vm) fn direct_operation(instruction: WideInstruction) -> Option<Operation<(), ()>> {
    Operation::decode(instruction, DecodeContext::EMPTY).filter(|operation| operation.direct_helper())
}

/// Whether the packed helper runs `instruction` directly, so the planner packs its step (`steps::packed_words`).
pub(in crate::vm) fn covers(instruction: WideInstruction) -> bool {
    direct_operation(instruction).is_some()
}

/// The direct run of the packed instruction `first` and `imm` (`WideInstruction::from_words`) in the running
/// frame: `Some(status)` where the arm completed it, `None` where the general step must run, which is every
/// case the frame form (activations, a materialized leaf), the `this` binding, or the arm's own decline takes.
///
/// # Safety
/// `cx` is the running lane's context.
pub(in crate::vm) unsafe fn direct<H: Host>(cx: *mut LaneContext<H>, first: u64, imm: u64) -> Option<u64> {
    let instruction = WideInstruction::from_words(first, u32::try_from(imm).ok()?)?.without_return();
    let operation = direct_operation(instruction)?;
    // SAFETY: the running lane's context; the VM, code and frame are the ones `lane_in_place` dispatches.
    let (vm, frame, code) = unsafe { (&mut *(*cx).vm, (*cx).frame(), &*(*cx).code) };
    if !vm.lane_stack.activations().is_empty() || !unsafe { (*cx).leaf.call }.is_null() {
        return None;
    }
    if instruction.op().binds_this() || instruction.writes_current_this() {
        return None;
    }
    let done = match operation {
        Operation::ElementRead { base, key } => element_read(vm, code, frame, instruction, base, key),
        Operation::ElementWrite { value, base, key } => element_write(vm, frame, value, base, key),
        Operation::Binary { left, right, operator, .. } => binary(vm, frame, instruction, left, right, operator),
        Operation::GetField { lookup: FieldLookup::Atom { atom, base, cache_site }, .. } => {
            field_read(vm, code, frame, instruction, atom, base, cache_site)
        }
        _ => false,
    };
    done.then_some(LANE_CALL_COMPLETED)
}

/// `GetIndex`'s read (`Vm::element_read_fast`), its result written to the instruction's register.
fn element_read<H: Host>(vm: &mut Vm<H>, code: &ResidualProgram, frame: usize, instruction: WideInstruction, base: Register, key: Operand) -> bool {
    let object = vm.read(frame, base);
    let Ok(key) = vm.resolve_operand(code, frame, key) else {
        return false;
    };
    let Some(value) = vm.element_read_fast(object, key) else {
        return false;
    };
    vm.write(frame, instruction.result_register(), value);
    true
}

/// `SetIndex`'s store to a present dense element (`Vm::store_present_element`).
fn element_write<H: Host>(vm: &mut Vm<H>, frame: usize, value: Register, base: Register, key: Register) -> bool {
    let (object, key, value) = (vm.read(frame, base), vm.read(frame, key), vm.read(frame, value));
    let Some(index) = key.as_int().and_then(|index| usize::try_from(index).ok()) else {
        return false;
    };
    vm.store_present_element(object, index, value)
}

/// `GetField`'s read of an atom through a register base, from the field cache (`Vm::cached_field_hit`, the probe
/// `get_field_cached` makes first). A probe that misses, a private name, and a function-information name (whose
/// preparation may allocate) decline.
fn field_read<H: Host>(vm: &mut Vm<H>, code: &ResidualProgram, frame: usize, instruction: WideInstruction, atom: Atom, base: FieldBase, cache_site: u16) -> bool {
    let Some(register) = base.register_index() else {
        return false;
    };
    if vm.is_private_name(atom) || vm.reads_function_info(atom) {
        return false;
    }
    let object = vm.read(frame, register);
    let FieldCacheProbe::Hit(value) = vm.cached_field_hit(code, object, cache_site) else {
        return false;
    };
    vm.write(frame, instruction.result_register(), value);
    true
}

/// A binary operator on two operands `Vm::number_binary` answers (Numbers, no user code), written by
/// `write_binary_result`.
fn binary<H: Host>(vm: &mut Vm<H>, frame: usize, instruction: WideInstruction, left: Register, right: Register, operator: u32) -> bool {
    let (left, right) = (vm.read(frame, left), vm.read(frame, right));
    let Some(value) = vm.number_binary(operator, left, right) else {
        return false;
    };
    vm.write_binary_result(frame, instruction, value);
    true
}
