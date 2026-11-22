//! The stop of a semantic entry (`native_js_operation_entry`), in the lane body's own protocol. The
//! entry runs inside an image body's generic row, which leaves with the entry's (status, word): a
//! return completes the activation (`LANE_BODY_RETURNED`, the value as the word, the record in
//! `returned_pc`), an exception ends it (`LANE_BODY_THREW`, the error in `Vm::lane_completion`, as a
//! native `throw` stashes it), and a termination is `TERMINATED_STATUS`. Any other stop (a frame the
//! step pushed or resumed) keeps its step in the payload with `NATIVE_JS_STOPPED`, as before.
//! `unsettle` is the inverse, for a caller that reads a run as a `JsNativeStep`.

use super::*;
use quench_bjit::call_record::TERMINATED_STATUS;

/// The (status, word) the entry returns for `step`, the payload's activation's, recording what the
/// status's reader takes from the payload and the VM.
pub(crate) fn settle(vm: &mut Vm, dispatch: &mut JsStepDispatch, step: JsNativeStep) -> (i64, u64) {
    let pc = step.instruction_pc;
    match step.result {
        Ok(StepResult::Return(value)) => {
            dispatch.head.returned_pc = pc;
            (i64::from(LANE_BODY_RETURNED), value.to_bits())
        }
        Err(error) if error.is_execution_terminated() => (i64::from(TERMINATED_STATUS), 0),
        Err(error) => {
            vm.lane_completion.raise(error);
            vm.lane_raise_site = raise_site(vm.frames[dispatch.frame].function, pc as u32);
            dispatch.head.returned_pc = pc;
            (i64::from(LANE_BODY_THREW), 0)
        }
        result => {
            dispatch.outcome = Some(JsNativeStep { result, ..step });
            (i64::from(NATIVE_JS_STOPPED), 0)
        }
    }
}

/// The step a run that ended with `status` and `word` made, where `settle` made the status: a
/// completion or an exception; `None` for any other status.
pub(crate) fn unsettle(vm: &mut Vm, dispatch: &JsStepDispatch, (status, word): (u32, u64)) -> Option<JsNativeStep> {
    let pc = dispatch.head.returned_pc;
    let result = match status {
        LANE_BODY_RETURNED => Ok(StepResult::Return(Value::from_bits(word))),
        LANE_BODY_THREW => Err(vm.lane_completion.take_raised()?),
        _ => return None,
    };
    Some(JsNativeStep { instruction_pc: pc, next_pc: pc + 1, result })
}
