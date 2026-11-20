//! The general step a lane makes in place for a record without a row of its own (`Op::steps_in_lane`),
//! in a JavaScript or a Wasm run, and what the run carries past it.
use super::*;

/// What a run carries past the general step of `instruction`, from `incoming`: `Carried::stepped`,
/// whose view is a JavaScript run's, with a Wasm run's view of its memory in its place
/// (`memory_past_step`). A Wasm record trusts the view it is handed: a JavaScript one would have
/// it address the context's bytes.
#[inline(always)]
fn stepped<H: Host>(cx: *mut LaneContext<H>, incoming: Carried, r: Registers, instruction: WideInstruction) -> Carried {
    Carried { memory: memory_past_step(cx, incoming.memory), ..Carried::stepped(cx, incoming, r, instruction) }
}

/// The view a run carries past a step from `incoming`: a JavaScript run's (`MemoryView::script`)
/// as it was; in a Wasm run, the view of the run's resolved binding read again, since the step may
/// have grown the memory (as `LaneCall::complete_call` reads it), and any other Wasm view (none,
/// or another binding's) as it was.
#[inline(never)]
fn memory_past_step<H: Host>(cx: *mut LaneContext<H>, incoming: MemoryView) -> MemoryView {
    // SAFETY: handlers run with exclusive access to their context.
    let (binding, resolved) = unsafe { (*cx).resolved };
    if incoming.bytes.is_null() || incoming.bytes != resolved.bytes {
        return incoming;
    }
    let fresh = memory_view(unsafe { vm(cx) }, binding);
    unsafe {
        (*cx).resolved = (binding, fresh);
        (*cx).memory = fresh;
    }
    fresh
}

lane_handler! {
    /// An instruction whose general step only continues or throws (`Op::steps_in_lane`),
    /// run in place by that step when the lane runs the frame's own code: the frame's
    /// registers are the lane's, and its PC is published as the general loop publishes it.
    /// A throw is stashed in `Vm::lane_raised` for the general path to raise at this
    /// instruction. Within a frameless activation, which has no frame, the general path runs
    /// it.
    #[inline(never)]
    pub(super) fn general_step_in_lane<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        if vm.lane_stack.has_frames() {
            resume!(cx, step_beside_activations_in_lane(cx, r, ip, view));
        }
        // SAFETY: the context's program outlives the lane run, and `ip` is a record of the
        // active view, which is the running frame's.
        let (code, frame, pc) = unsafe {
            (&*(*cx).code, (*cx).frame(), ip.offset_from((*cx).base) as usize)
        };
        let function = code.function(vm.frames[frame].function as usize);
        let instruction = function.widened(function.code[pc]);
        // A record that returns its result (`RETURN_REGISTER`), or a return op itself (a raw
        // return such as `WasmReturn64`), completes the frame, which the general path does.
        if instruction.result_flags() & crate::bytecode::RETURN_REGISTER != 0
            || instruction.op().traits().has(crate::bytecode::Traits::RETURN_FAMILY)
        {
            return exit(ip);
        }
        vm.frames[frame].pc = pc;
        vm.frames[frame].pc_is_resume = false;
        let mut next = pc + 1;
        match vm.step(code, frame, instruction, &mut next, false) {
            Ok(StepResult::Continue) => {}
            Ok(other) => unreachable!("an in-place step only continues: {:?} gave {}", instruction.op(), match other { StepResult::Return(_) => "Return", _ => "another result" }),
            Err(error) => {
                vm.lane_completion.raise(error);
                return exit(ip);
            }
        }
        // A step that may allocate or call guest code can make a collection due. An opcode without
        // `Traits::MAY_ALLOCATE` allocates nothing, so no collection can have become due by it.
        if instruction.op().traits().has(Traits::MAY_ALLOCATE) {
            note_allocation(vm);
        }
        // SAFETY: the step continues at a record of the running frame's view.
        let target = unsafe { (*cx).base.add(next) };
        if next > pc { next!(cx, r, target, stepped(cx, view, r, instruction)) } else { exit(target) }
    }
}

/// `general_step_in_lane` within a frameless activation: a value step (`Vm::call_out`) of
/// the activation's registers, made beside the activations as a native runs beside them
/// (`DeferredLaneCall`), so guest code it reaches makes them frames first; its result lands
/// in the running function's registers then. Any other step takes the general path.
#[cold]
#[inline(never)]
fn step_beside_activations_in_lane<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, view: Carried) -> Resumed {
    let vm = unsafe { vm(cx) };
    let Some(function) = vm.lane_stack.top_frame().map(|activation| activation.function()) else {
        return Resumed::Exit(exit(ip));
    };
    // SAFETY: the context's program outlives the lane run, and `ip` is a record of the
    // innermost activation's view.
    let (code, pc) = unsafe { (&*(*cx).code, ip.offset_from((*cx).base) as usize) };
    let body = code.function(function as usize);
    let instruction = body.widened(body.code[pc]);
    if !instruction.op().steps_with_result() {
        return Resumed::Exit(exit(ip));
    }
    let (operation, result) = crate::staged::Operation::<(), ()>::decode(instruction, crate::staged::DecodeContext::of(code))
        .and_then(|row| row.call_out()).expect("a result step is a call-out row");
    let operands = operation.operands().read(&code.method_arguments, |operand| call_operand_in_lane(cx, r, operand));
    let value = match unwound_in_place(cx, ip, |vm, code| {
        let site = super::call_out::CallSite { owner: vm.call_out_owner(operation), function, resume: pc + 1 };
        vm.call_out(code, site, operation, operands)
    }) {
        Ok(value) => value,
        Err(next) => return Resumed::Exit(next),
    };
    // Guest code the step ran made the activations frames, whose registers these are.
    let r = unsafe { (*cx).registers };
    if let (Some(value), Some(result)) = (value, result) {
        r.set(result, value);
    }
    Resumed::At(r, following(ip), stepped(cx, view, r, instruction))
}
