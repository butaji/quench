//! The direct runs of the packed step helper (`vm_steps::lane_body_step_packed`). A family's fast arm
//! reads the running frame's operands and declines, before any effect, where the read needs the general
//! step (guest code, an allocation, a key or base it does not take). A declined row runs the general step
//! (`lane_body_step`) from its PC, as every step does; a completed one skips the frame bookkeeping and base
//! refreshes, which only guest code or an allocation need.

use super::*;
use super::super::compiled::LANE_CALL_COMPLETED;

/// Whether the packed helper has a direct run for `op`, so the planner packs its steps (`steps::packed_words`).
pub(in crate::vm) fn covers(op: Op) -> bool {
    matches!(op, Op::GetIndex)
}

/// The direct run of the packed instruction `first` and `imm` (`WideInstruction::from_words`) in the running
/// frame: `Some(status)` where the family completed it, `None` where the general step must run, which is
/// every case the frame form (activations, a materialized leaf) or the family's fast arm does not take.
///
/// # Safety
/// `cx` is the running lane's context.
pub(in crate::vm) unsafe fn direct<H: Host>(cx: *mut LaneContext<H>, first: u64, imm: u64) -> Option<u64> {
    let instruction = WideInstruction::from_words(first, u32::try_from(imm).ok()?)?.without_return();
    // SAFETY: the running lane's context; the VM and frame are the ones `lane_in_place` dispatches.
    let (vm, frame) = unsafe { (&mut *(*cx).vm, (*cx).frame()) };
    if !vm.lane_stack.activations().is_empty() || !unsafe { (*cx).leaf.call }.is_null() {
        return None;
    }
    if instruction.op().binds_this() || instruction.writes_current_this() {
        return None;
    }
    match instruction.op() {
        Op::GetIndex => element_get(vm, frame, instruction).then_some(LANE_CALL_COMPLETED),
        _ => None,
    }
}

/// `GetIndex` whose base and key are registers, the key an index the element storage or a non-BigInt typed
/// array answers (`dense_element`, `typed_array_number_element`): the element written to the result register.
/// `false`, with nothing written, where the general read decides.
fn element_get<H: Host>(vm: &mut Vm<H>, frame: usize, instruction: WideInstruction) -> bool {
    let (Some(base), Some(key)) = (instruction.operand_b().register_index(), instruction.operand_c().register_index()) else {
        return false;
    };
    let (object, key) = (vm.read(frame, base), vm.read(frame, key));
    let Some(index) = key.as_int().and_then(|index| usize::try_from(index).ok()) else {
        return false;
    };
    let Some(value) = vm.dense_element(object, index).or_else(|| vm.typed_array_number_element(object, index)) else {
        return false;
    };
    vm.write(frame, instruction.result_register(), value);
    true
}
