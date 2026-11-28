//! Awaited and marked call results, and the iteration steps of a lane.
use super::*;

/// A `Call` record's operand, flagged when only the `Await` right after the call reads its
/// result: the await overwrites the result register, or liveness (`live`) shows it dead after
/// the await. A settling call there makes its promise only where one becomes observable
/// (`ImmediateLayout::LANE_CALL_RESULT_AWAITED_FLAG`). Liveness is derived only for a call
/// followed by an `Await`, of a function whose suspension derives it anyway
/// (`Vm::suspend_activation`).
pub(in super::super) fn call_awaited_marked_in_lane<'live>(
    operand: LaneOperand,
    decoded: &[WideInstruction],
    pc: usize,
    live: &dyn Fn() -> Option<&'live std::rc::Rc<quench_runtime::compile::liveness::RegisterMasks>>,
) -> LaneOperand {
    let call = decoded[pc];
    let Some(&awaited) = decoded.get(pc + 1) else {
        return operand;
    };
    if !quench_runtime::staged::calls(call.op()) || call.result_flags() != 0 || !quench_runtime::staged::awaits(awaited.op()) {
        return operand;
    }
    let result = call.result_register();
    let feeds = awaited.register_b() == result
        && (awaited.result_register() == result
            || live()
                .and_then(|masks| masks.get(pc + 2))
                .is_some_and(|live| quench_runtime::compile::liveness::register_dead(live, usize::from(result))));
    if !feeds {
        return operand;
    }
    // SAFETY: a `Call`'s operand is its immediate (`operand_in_lane`).
    let imm = unsafe { operand.imm };
    LaneOperand { imm: imm | ImmediateLayout::LANE_CALL_RESULT_AWAITED_FLAG }
}

/// A `Call` record's operand, flagged when liveness (`live`, the registers live before each
/// pc) shows its result register dead after the call: nothing reads the value the callee
/// returns (`ImmediateLayout::lane_call_result_used`).
pub(in super::super) fn call_result_marked_in_lane<'live>(
    operand: LaneOperand,
    instruction: WideInstruction,
    pc: usize,
    live: &dyn Fn() -> Option<&'live std::rc::Rc<quench_runtime::compile::liveness::RegisterMasks>>,
) -> LaneOperand {
    if !quench_runtime::staged::calls(instruction.op()) {
        return operand;
    }
    let Some(masks) = live() else {
        return operand;
    };
    if !quench_runtime::compile::liveness::call_result_dead(instruction, pc, masks) {
        return operand;
    }
    // SAFETY: a `Call`'s operand is its immediate (`operand_in_lane`).
    let imm = unsafe { operand.imm };
    LaneOperand { imm: imm | ImmediateLayout::LANE_CALL_RESULT_UNUSED_FLAG }
}

/// Whether the `Call` at `pc` is an iteration step: no arguments, its result checked by the
/// `RequireIteratorResult` after it, as the compiler emits a for-of or destructuring step,
/// whose result register only the step's own `done` and `value` reads take
/// (`iterator_step_miss`).
pub(in super::super) fn iteration_step_in_lane(decoded: &[WideInstruction], pc: usize) -> bool {
    let call = decoded[pc];
    call.result_flags() == 0
        && ImmediateLayout::argument_count(call.imm()) == 0
        && decoded.get(pc + 1).is_some_and(|next| {
            quench_runtime::staged::requires_iterator_result(next.op()) && next.register_b() == call.result_register()
        })
}

/// How the iteration step at `pc` (`iteration_step_in_lane`) begins a for-of step: after its
/// `RequireIteratorResult`, a plain read of the result into a register, a test of that
/// register (`ForOfTest`), and a second plain read of the result where the body begins.
/// `iterator_step_reads_miss` then makes the reads from the step itself; their atoms it
/// tests as it runs.
pub(in super::super) fn for_of_step_in_lane(decoded: &[WideInstruction], pc: usize) -> Option<ForOfTest> {
    let result = decoded[pc].result_register();
    let record = |offset: usize| decoded.get(pc + offset).copied();
    let reads_result = |offset: usize| {
        record(offset).is_some_and(|read| quench_runtime::staged::reads_named_field(read.op()) && read.result_flags() == 0 && read.b() == result)
    };
    record(FOR_OF_REQUIRE_RESULT).filter(|require| quench_runtime::staged::meaning_fast::require_iterator_result_operand(*require).is_some())?;
    let done = record(FOR_OF_DONE_READ).filter(|_| reads_result(FOR_OF_DONE_READ))?.result_register();
    let test = record(FOR_OF_DONE_TEST).filter(|test| test.a() == done)?;
    let form = match quench_runtime::staged::truth_jump(test.op())? {
        false => ForOfTest::Emitted,
        true => ForOfTest::Threaded,
    };
    let exit = record(form.exit())?;
    let leaves = (if form == ForOfTest::Emitted { quench_runtime::staged::jump_family(exit.op()) } else { quench_runtime::staged::truth_jump(exit.op()) == Some(true) })
        && exit.jump_target() as usize > pc + form.value_read();
    let enters = form == ForOfTest::Threaded || test.jump_target() as usize == pc + form.value_read();
    (leaves && enters && reads_result(form.value_read())).then_some(form)
}
