//! The direct runs of the packed step helper (`vm_steps::lane_body_step_packed`). The helper decodes the
//! instruction's staged operation (`Operation::direct_helper`) and runs its direct arm, which reads the running
//! frame and declines, before any effect, where the general step decides. A declined row runs the general step
//! (`lane_body_step`) from its PC; a completed one skips the frame bookkeeping and base refreshes, which only
//! guest code or an allocation need. The arm is the one `Vm::step` runs (`write_binary_result`), so both paths
//! share their semantics. The planner admits a row only where its site's feedback makes a decline rare
//! (`function::feedback_admits`).

use super::*;
use super::super::compiled::LANE_CALL_COMPLETED;
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
    // SAFETY: the running lane's context; the VM and frame are the ones `lane_in_place` dispatches.
    let (vm, frame) = unsafe { (&mut *(*cx).vm, (*cx).frame()) };
    if !vm.lane_stack.activations().is_empty() || !unsafe { (*cx).leaf.call }.is_null() {
        return None;
    }
    if instruction.op().binds_this() || instruction.writes_current_this() {
        return None;
    }
    let done = match operation {
        Operation::Binary { left, right, operator, .. } => binary(vm, frame, instruction, left, right, operator),
        _ => false,
    };
    done.then_some(LANE_CALL_COMPLETED)
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
