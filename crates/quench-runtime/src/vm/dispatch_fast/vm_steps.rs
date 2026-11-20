//! A lane body's call-outs (`staged::CallOut`: the value steps, `ForInNext`, a `Binary`, the
//! environment, arguments and global-name steps), the one helper a row's slow
//! case calls: it makes the step by the interpreter's own definition (`Vm::call_out`), as
//! `step_beside_activations_in_lane` does for a frameless activation, and continues, so no such
//! record hands its body back.
use super::*;

/// The call-out of the record at `call_pc`, its operands read from the running registers:
/// `LANE_CALL_COMPLETED` with the result written to its register, `LANE_CALL_MISSED` where it
/// raised (the error stashed for the general path, which re-runs the record and raises it), or
/// `LANE_CALL_LEFT` where guest code it ran made the lane's activations frames, which leave the
/// body at the next record. A decision (`CallOut::Decide`) completes with `LANE_CALL_HOLDS` where
/// the comparison held.
pub(in crate::vm) unsafe extern "C" fn vm_operation_step<H: Host>(_vm: u64, _residual: u64, context: u64, call_pc: u64) -> u64 {
    // An ahead body runs from its entry trampoline, with no lane run to step in: it leaves at
    // the record, which the entry's per-PC fallback executes (as `body_call_helper`).
    if context == 0 { return super::super::compiled::LANE_CALL_MISSED; }
    let cx = context as usize as *mut LaneContext<H>;
    let status = native_frames::in_innermost_frame(cx, || unsafe { vm_operation_step_in(cx, call_pc) });
    // SAFETY: `context` is the running lane's; the step may have allocated or run guest code.
    unsafe { refresh_body_bases::<H>(context) };
    // SAFETY: as above; a region the lane runs reads its payload's bases after the step too.
    unsafe { refresh_region_bases_of::<H>(context) };
    status
}

/// `vm_operation_step` with the context of the running body, which a native callee's frame made the
/// innermost (`native_frames::in_innermost_frame`).
///
/// # Safety
/// As `vm_operation_step`.
unsafe fn vm_operation_step_in<H: Host>(cx: *mut LaneContext<H>, call_pc: u64) -> u64 {
    use super::super::compiled::{LANE_CALL_COMPLETED, LANE_CALL_HOLDS, LANE_CALL_LEFT, LANE_CALL_MISSED};
    let (r, ip) = unsafe { ((*cx).registers, (*cx).base.add(call_pc as usize)) };
    let function = running_function_in_lane(cx);
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let body = code.function(function as usize);
    let instruction = body.widened(body.code[call_pc as usize]);
    let (operation, result) = crate::staged::Operation::<(), ()>::decode(instruction, crate::staged::DecodeContext::of(code))
        .and_then(|row| row.call_out()).expect("a call-out record decodes");
    let operands = operation.operands().read(&code.method_arguments, |operand| call_operand_in_lane(cx, r, operand));
    let Some(value) = in_place_in_lane(cx, ip, |vm, code| {
        let site = super::super::call_out::CallSite { owner: vm.call_out_owner(operation), function, resume: call_pc as usize + 1 };
        vm.call_out(code, site, operation, operands)
    }) else { return LANE_CALL_MISSED };
    // Guest code the step ran may have made the activations frames, whose registers these are.
    let after = unsafe { (*cx).registers };
    if let (Some(value), Some(register)) = (value, result) {
        after.set(register, value);
    }
    if after.0 == r.0 {
        let holds = matches!(operation, crate::staged::CallOut::Decide { .. }) && value == Some(Value::TRUE);
        return if holds { LANE_CALL_HOLDS } else { LANE_CALL_COMPLETED };
    }
    // A decision that failed took its jump (`JumpBinaryFalse`), which the activations resume at.
    let resume = match operation {
        crate::staged::CallOut::Decide { .. } if value != Some(Value::TRUE) =>
            unsafe { (*cx).base.add(instruction.jump_target() as usize) },
        _ => following(ip),
    };
    unsafe { (*cx).callee_resume = resume; (*cx).callee_acc = Value::UNDEFINED; }
    LANE_CALL_LEFT
}

// merge: theirs deletes this call-out (its string `+` runs as the denoted row `lane_string_concat`);
// ours keeps it as `body_string_concat`, which ours' compiled.rs still names.
/// `+` of the record at `call_pc` whose operands are registers holding strings (or Numbers, one a
/// string) as `Vm::primitive_concatenation` defines it: `LANE_CALL_COMPLETED` with the result in
/// its register, or `LANE_CALL_MISSED` before any effect where the operands are no such pair
/// (guest code never runs, so the body is never left).
pub(in crate::vm) unsafe extern "C" fn body_string_concat<H: Host>(_vm: u64, _residual: u64, context: u64, call_pc: u64) -> u64 {
    use super::super::compiled::{LANE_CALL_COMPLETED, LANE_CALL_MISSED};
    let cx = context as usize as *mut LaneContext<H>;
    let vm = unsafe { vm(cx) };
    let r = unsafe { (*cx).registers };
    // Read where it is (no spill, one-frame-one-call.md F4).
    let function = match vm.lane_stack.top_frame() {
        Some(activation) => activation.function(),
        None => vm.frames[unsafe { (*cx).frame() }].function,
    };
    // SAFETY: the context's program outlives the lane run.
    let body = unsafe { &*(*cx).code }.function(function as usize);
    let instruction = decoded_instruction(body, body.code[call_pc as usize]);
    let [Some(left), Some(right)] = [instruction.operand_b(), instruction.operand_c()].map(|operand| operand.register_index().map(|register| r.get(register))) else { return LANE_CALL_MISSED };
    match concat_value(cx, left, right) {
        Some(value) => {
            r.set(instruction.result_register(), value);
            LANE_CALL_COMPLETED
        }
        None => LANE_CALL_MISSED,
    }
}

/// The decision of `left operator right` that runs no guest code and allocates nothing: a comparison
/// of two Numbers, a strict equality (`Vm::strict_equal`), a loose one no conversion decides, `instanceof` by the remembered constructor layout
/// (`Vm::steady_instanceof`) and `in` of a string key in an ordinary object (`Vm::plain_in`);
/// `None` for any other pair, which the full algorithm decides. The one rule of the interpreter's
/// staged frame and of the lane's decide helper.
pub(super) fn comparison_decision<H: Host>(vm: &mut Vm<H>, operator: u32, left: Value, right: Value) -> Option<bool> {
    // Two Numbers: the Number comparison (`operations::number_relation`), which runs no guest code.
    if let Some((holds, _)) = crate::vm::operations::number_relation(operator, left, right) {
        return Some(holds);
    }
    match crate::vm::operations::equality_operator(operator) {
        Some((true, negated)) => Some(vm.strict_equal(left, right) != negated),
        // A loose equality no conversion decides (`Vm::identity_equality`): nullish values, two objects, one type.
        Some((false, _)) => vm.identity_equality(operator, left, right),
        None if operator == INSTANCEOF_OPERATOR => vm.steady_instanceof(left, right),
        None => vm.plain_in(operator, left, right).or_else(|| vm.string_relation(operator, left, right)),
    }
}

/// A lane body's decision of `left operator right` by the words a `CallOut::Decide` carries, which
/// needs no record of the VM's: `Value::TRUE` or `Value::FALSE`, or `Value::DELETED` where only
/// the full algorithm decides, which leaves the body at the test (`comparison_decision`).
pub(in crate::vm) unsafe extern "C" fn decide_comparison<H: Host>(vm: u64, _residual: u64, left: u64, right: u64, operator: u64) -> u64 {
    // SAFETY: as `field_set_in_lane`; the decision reads the heap, runs no guest code and allocates nothing.
    let vm = unsafe { &mut *(vm as *mut Vm<H>) };
    comparison_decision(vm, operator as u32, Value::from_bits(left), Value::from_bits(right))
        .map_or_else(|| Value::DELETED.to_bits(), |holds| Value::boolean(holds).to_bits())
}


/// The record `record` at `call_pc`, a step that needs the frame its function runs in (`Op::steps_in_body`: the scope,
/// name, field, literal and class steps, and a call form that binds `this` or runs `eval`), of an activation that has
/// none (a native callee, a lane callee): the lane's activations become the frames the interpreter would have run
/// (`Vm::materialize_deferred_lane`), the step is made on the innermost as the general loop makes it, and the body
/// leaves for the general path past the record (`LANE_CALL_LEFT`), which resumes in those frames. A record that returned
/// its result is returned from its frame there (`frame_return`), and an error the step raised is stashed for
/// the general path to raise at the record (`in_place_in_lane`). `LANE_CALL_MISSED` where the record is no such step.
///
/// # Safety
/// As `body_general_step`; `record` is the record at `call_pc` of the innermost activation's function.
unsafe fn frames_step_in<H: Host>(cx: *mut LaneContext<H>, call_pc: u64, record: WideInstruction) -> u64 {
    use super::super::compiled::LANE_CALL_MISSED;
    let instruction = record.without_return();
    if !(instruction.op().steps_in_body() || instruction.op().makes_script_call()) { return LANE_CALL_MISSED; }
    let (base, pc) = unsafe { ((*cx).base, call_pc as usize) };
    let ip = unsafe { base.add(pc) };
    let made = in_place_in_lane(cx, ip, |vm, code| {
        vm.materialize_deferred_lane();
        let frame = vm.frames.len() - 1;
        vm.frames[frame].pc = pc;
        vm.frames[frame].pc_is_resume = false;
        let mut next = pc + 1;
        vm.step(code, frame, instruction, &mut next, false)?;
        Ok((next, record.returns_from_frame().then(|| vm.read(frame, instruction.result_register()))))
    });
    let resume = match made {
        Some((_, Some(value))) => frame_return(unsafe { vm(cx) }, ip, value),
        Some((next, None)) => exit(unsafe { base.add(next) }),
        None => exit(ip),
    };
    unsafe { left_operand_at(cx, resume) }
}

/// The body leaves for the general path at `resume`, which resumes in the frames the activations became
/// (`LANE_CALL_LEFT`).
///
/// # Safety
/// `cx` is the running lane's context.
unsafe fn left_operand_at<H: Host>(cx: *mut LaneContext<H>, resume: Ip) -> u64 {
    unsafe { (*cx).callee_resume = resume; (*cx).callee_acc = Value::UNDEFINED; }
    super::super::compiled::LANE_CALL_LEFT
}

/// The record `record` at `call_pc` (`Op::steps_on_owner`: a local, capture or name step) of an activation that
/// has no frame, made over the activation's window as the general loop makes it over a frame (`Vm::owner_step` over the
/// running window, `Vm::running_owner`): `LANE_CALL_COMPLETED` with its effects in the window, so the body goes on
/// in place. Guest code the step ran (a getter or a proxy trap of a `with` object or the global object) made the
/// activations frames, which finish the step: the body leaves for the general path past the record
/// (`LANE_CALL_LEFT`), as `frames_step_in` does. `LANE_CALL_MISSED` where it raised: the error is stashed for
/// the general path to raise at the record (`in_place_in_lane`).
///
/// # Safety
/// As `frames_step_in`.
unsafe fn owner_step_in<H: Host>(cx: *mut LaneContext<H>, call_pc: u64, record: WideInstruction) -> u64 {
    use super::super::compiled::{LANE_CALL_COMPLETED, LANE_CALL_MISSED};
    let instruction = record.without_return();
    let (base, pc) = unsafe { ((*cx).base, call_pc as usize) };
    let ip = unsafe { base.add(pc) };
    let made = in_place_in_lane(cx, ip, |vm, code| {
        let owner = vm.running_owner();
        vm.owner_step(code, owner, instruction, pc + 1)?;
        // The activations stayed the lane's while the step made no frame of them.
        Ok(vm.lane_deferred.is_none().then(|| {
            record.returns_from_frame().then(|| vm.read(vm.frames.len() - 1, instruction.result_register()))
        }))
    });
    match made {
        None => LANE_CALL_MISSED,
        Some(None) => LANE_CALL_COMPLETED,
        Some(Some(returned)) => {
            let resume = match returned {
                Some(value) => frame_return(unsafe { vm(cx) }, ip, value),
                None => exit(unsafe { base.add(pc + 1) }),
            };
            unsafe { left_operand_at(cx, resume) }
        }
    }
}


/// The record at `call_pc`, an instruction whose general step only continues, jumps or throws
/// (`Op::steps_in_body`: the lane's own in-place steps and those that may replace the frame's
/// environment), or a call form that is not a plain call or a construction over registers
/// (`Op::makes_script_call`: the callee runs as it does for any call a helper makes, the body's
/// activation not pushed, so its result is the step's), made by that step on the running frame as
/// `general_step_in_lane` makes it: `LANE_CALL_COMPLETED` with its effects in the frame,
/// `LANE_CALL_BRANCHED` where it also took its conditional jump (`ForInNext` past its last key),
/// or `LANE_CALL_MISSED` where it raised (the error stashed for the general path to raise at the
/// record, which is how a body's `throw` ends it) or where the record is no such step; nothing ran
/// then. Where the body has no frame of its own (a native callee's activation) a step that needs
/// one makes the activations frames and leaves past it (`frames_step_in`, `LANE_CALL_LEFT`). The step may have
/// replaced the frame's environment, so the lane's capture caches, which name the old one, are
/// dropped after it (`clone_env`).
/// `lane_body_step_record`, then the heap bases the body's rows read through the payload reloaded
/// (`Vm::refresh_body_bases`), and those of the loop region's payload the lane runs
/// (`refresh_region_bases_of`): the step may have allocated or run guest code.
///
/// # Safety
/// As `body_general_step_record`.
pub(in crate::vm) unsafe extern "C" fn body_general_step<H: Host>(vm: u64, residual: u64, context: u64, call_pc: u64) -> u64 {
    // A comparison jump a pure decision settles (an image body's `!=` of two objects through a Number row) runs no
    // guest code and moves nothing: no step, no base to refresh.
    if let Some(status) = unsafe { decided_compare_jump::<H>(context as usize as *mut LaneContext<H>, call_pc) } { return status; }
    // SAFETY: the caller's contract.
    let traced = crate::trace::Channel::BodyLeave.on().then(|| {
        let cx = context as usize as *mut LaneContext<H>;
        let function = running_function_in_lane(cx);
        let op = unsafe { &*(*cx).code }.function(function as usize).code.get(call_pc as usize).map(|word| word.op());
        (function, op, unsafe { self::vm(cx) }.lane_stack.has_frames())
    });
    let status = unsafe { lane_body_step_record::<H>(vm, residual, context, call_pc) };
    if let Some((function, op, activations)) = traced && status != super::super::compiled::LANE_CALL_COMPLETED {
        eprintln!("QUENCH_CALL_STATUS helper=step function={function} pc={call_pc} op={op:?} activations={activations} status={status}");
    }
    unsafe { refresh_body_bases::<H>(context) };
    // SAFETY: as `lane_body_step_record`; a region the lane runs reads its payload's bases after the step too.
    unsafe { refresh_region_bases_of::<H>(context) };
    status
}

/// `lane_body_step` for a row the planner packed (`LaneHelper::StepPacked`, `CallRow::HelperPacked`): its family
/// runs the instruction the planner decoded, `first` and `imm` (`family_helpers::direct`), where it can; else the
/// general step, which decodes the record at `call_pc` as `lane_body_step` does.
///
/// # Safety
/// As `lane_body_step`, and `first` and `imm` are the packed words of the record at `call_pc`.
pub(in crate::vm) unsafe extern "C" fn lane_body_step_packed<H: Host>(vm: u64, residual: u64, context: u64, call_pc: u64, first: u64, imm: u64) -> u64 {
    let cx = context as usize as *mut LaneContext<H>;
    // SAFETY: the caller's contract.
    if let Some(status) = unsafe { family_helpers::direct::<H>(cx, first, imm) } {
        return status;
    }
    unsafe { body_general_step::<H>(vm, residual, context, call_pc) }
}

unsafe fn lane_body_step_record<H: Host>(_vm: u64, _residual: u64, context: u64, call_pc: u64) -> u64 {
    let cx = context as usize as *mut LaneContext<H>;
    native_frames::in_innermost_frame(cx, || unsafe { body_general_step_in(cx, call_pc) })
}

/// The step of the record at `call_pc` of the innermost lane activation: over its window where it is
/// a local, capture or name step (`owner_step_in`), over its operand values where it is a value step
/// (`value_steps`), else in the frames the activations become (`frames_step_in`).
///
/// # Safety
/// As `body_general_step`.
unsafe fn activation_step_in<H: Host>(cx: *mut LaneContext<H>, call_pc: u64) -> u64 {
    let body = unsafe { &*(*cx).code }.function(running_function_in_lane(cx) as usize);
    let record = body.widened(body.code[call_pc as usize]);
    if record.without_return().op().steps_on_owner() {
        unsafe { owner_step_in(cx, call_pc, record) }
    } else {
        unsafe { super::value_steps::activation_value_step(cx, call_pc, record) }.unwrap_or_else(|| unsafe { frames_step_in(cx, call_pc, record) })
    }
}

/// `body_general_step` with the context of the running body, which a native callee's frame made the
/// innermost (`native_frames::in_innermost_frame`). Where the body runs as lane activations (a native
/// callee, a lane callee) the steps that have no frame to run in take the value step's form, which
/// reads the operands from the activation's registers, the local, capture and name steps the owner step's, which
/// runs over the activation's window (`owner_step_in`), and the steps that need a frame the frames step's, which
/// makes the activations frames (`lane_body_value_step_in`, `frames_step_in`).
///
/// # Safety
/// As `body_general_step`.
unsafe fn body_general_step_in<H: Host>(cx: *mut LaneContext<H>, call_pc: u64) -> u64 {
    use super::super::compiled::{LANE_CALL_BRANCHED, LANE_CALL_COMPLETED, LANE_CALL_MISSED};
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run, and `call_pc` is a record of the running frame's function.
    if vm.lane_stack.has_frames() {
        return unsafe { activation_step_in(cx, call_pc) };
    }
    let (code, frame, ip) = unsafe { (&*(*cx).code, (*cx).frame(), (*cx).base.add(call_pc as usize)) };
    let function = code.function(vm.frames[frame].function as usize);
    // A returning record (`return a + b`) is made as the step that writes its result register: the body's
    // own `Return` row completes the activation with it.
    let instruction = function.widened(function.code[call_pc as usize]).without_return();
    // A value step (an operator or a comparison jump) is an image body's guard miss: the frame's own step makes it.
    let value_step = instruction.op().value_step().is_some() && instruction.result_flags() == 0;
    if !(instruction.op().steps_in_body() || instruction.op().makes_script_call() || value_step) { return LANE_CALL_MISSED; }
    let following = call_pc as usize + 1;
    let branched = in_place_in_lane(cx, ip, |vm, code| {
        let mut next = following;
        vm.step(code, frame, instruction, &mut next, false).map(|result| {
            debug_assert!(matches!(result, StepResult::Continue), "an in-place step only continues");
            next != following
        })
    });
    if branched.is_some() && (instruction.op().binds_this() || instruction.writes_current_this()) {
        // The step bound the frame's `this` (`super()`): the rows that read the receiver take the payload's.
        let this = vm.current_this(frame).unwrap_or(Value::DELETED);
        vm.publish_body_receiver(this);
    }
    // The dropped caches are derived again where the lane next needs them.
    handler_edges::drop_environment_caches(cx);
    branched.map_or(LANE_CALL_MISSED, |branched| if branched { LANE_CALL_BRANCHED } else { LANE_CALL_COMPLETED })
}

/// The allocation of the record at `call_pc`, a fixed-shape object the record defines (`Step::Allocate`): its value is
/// written to the record's destination, `LANE_CALL_MISSED` where the lane's allocator declines, and the record is
/// the interpreter's to make.
pub(in crate::vm) unsafe extern "C" fn lane_allocate_step<H: Host>(_vm: u64, _residual: u64, context: u64, call_pc: u64) -> u64 {
    if context == 0 { return super::super::compiled::LANE_CALL_MISSED; }
    let cx = context as usize as *mut LaneContext<H>;
    let status = native_frames::in_innermost_frame(cx, || unsafe { lane_allocate_in(cx, call_pc) });
    // SAFETY: `context` is the running lane's; the allocation may have collected, which moves no register word.
    unsafe { refresh_body_bases::<H>(context) };
    // SAFETY: as above; a region the lane runs reads its payload's bases after the allocation too.
    unsafe { refresh_region_bases_of::<H>(context) };
    status
}

/// `lane_allocate_step` with the context of the running body (`native_frames::in_innermost_frame`).
///
/// # Safety
/// As `lane_allocate_step`.
unsafe fn lane_allocate_in<H: Host>(cx: *mut LaneContext<H>, call_pc: u64) -> u64 {
    use super::super::compiled::{LANE_CALL_COMPLETED, LANE_CALL_MISSED};
    let r = unsafe { (*cx).registers };
    let function = running_function_in_lane(cx);
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let body = code.function(function as usize);
    let instruction = body.widened(body.code[call_pc as usize]);
    let Some((destination, allocation)) = crate::staged::Allocation::decode(instruction, &[]) else { return LANE_CALL_MISSED };
    let Some(value) = (unsafe { super::lane_frame::lane_allocation::<H>(cx.cast(), r, allocation) }) else { return LANE_CALL_MISSED };
    // SAFETY: the allocation ran no guest code, so the lane's register file is `r`'s still.
    unsafe { (*cx).registers }.set(destination, value);
    LANE_CALL_COMPLETED
}

/// The `JumpBinaryFalse` record at `call_pc` of the running body where `comparison_decision` decides it over its
/// register and constant operands: `LANE_CALL_COMPLETED` where the comparison holds (the record falls through),
/// `LANE_CALL_BRANCHED` where it does not (the record jumps); `None` for any other record or pair, which the
/// general step makes.
///
/// # Safety
/// As `body_general_step`.
unsafe fn decided_compare_jump<H: Host>(cx: *mut LaneContext<H>, call_pc: u64) -> Option<u64> {
    use super::super::compiled::{LANE_CALL_BRANCHED, LANE_CALL_COMPLETED};
    // SAFETY: the context's program outlives the lane run.
    let body = unsafe { &*(*cx).code }.function(running_function_in_lane(cx) as usize);
    let instruction = body.widened(*body.code.get(call_pc as usize)?);
    if instruction.op() != crate::bytecode::Op::JumpBinaryFalse || instruction.result_flags() != 0 { return None; }
    let r = unsafe { (*cx).registers };
    let (left, right) = (operand_value_in_lane(cx, r, instruction.operand_b().0)?, operand_value_in_lane(cx, r, instruction.operand_c().0)?);
    let holds = comparison_decision(unsafe { vm(cx) }, instruction.binary_operator_field(), left, right)?;
    Some(if holds { LANE_CALL_COMPLETED } else { LANE_CALL_BRANCHED })
}
