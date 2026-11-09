//! The lane's yields of generator frames their callers resumed in place: `yield`, and
//! `yield*` of a delegate generator.
use super::*;

script_handler! {
    /// `yield` of the frame of a generator its caller resumed in place
    /// (`PendingGeneralCall::completion`), made as the general loop makes it: the frame
    /// suspends into the generator at the record after this one, and the caller continues
    /// with the step's result (`Vm::finish_in_loop_generator_step`). Any other `yield`
    /// takes the general path.
    #[inline(never)]
    fn lane_yield<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        // SAFETY: the general loop's pending calls outlive the lane run.
        let pending = unsafe { &mut *(*cx).pending };
        let frame = unsafe { (*cx).frame };
        let resumed_here = vm.lane_stack.activations.is_empty()
            && frame + 1 == vm.frames.len()
            && pending.last().is_some_and(|call| call.completion.is_generator() && call.caller + 1 == frame);
        if !resumed_here {
            return exit(ip);
        }
        let value = r.get(i.b);
        // SAFETY: the record lies in the running view.
        let pc = unsafe { ip.offset_from((*cx).base) } as usize;
        vm.frames[frame].pc = pc + 1;
        vm.frames[frame].pc_is_resume = true;
        let mut call = pending.pop().expect("a pending call resumed the generator");
        let suspended = vm.frames.pop().expect("the generator's frame is the innermost");
        let stop = super::generator::InLoopGeneratorStop::Yielded {
            destination: i.a & crate::bytecode::REGISTER_MASK,
            delegated_result: None,
        };
        let resume = call.completion.take_generator().expect("a generator call");
        let result = vm.finish_in_loop_generator_step(suspended, resume, value, stop);
        drop(call.call_guard);
        vm.active_registers = vm.frames[call.caller].registers.as_mut_ptr();
        vm.write(call.caller, call.destination, result);
        // The result may be a fresh allocation, which the caller's safepoints collect after.
        note_allocation(vm);
        resume!(cx, lane_enter_script_frame(cx, call.caller))
    }
}

script_handler! {
    /// `yield*` of a synchronous generator's frame the lane runs innermost: a step of a
    /// delegate generator made in place (`Vm::yield_star_in_place`), whose fresh result waits
    /// in the state register for this record, made again; that result taken back, the
    /// delegate's completion goes to the destination, and any other step passes on as this
    /// frame's own yield of the result where its caller resumed it in place, as `lane_yield`
    /// makes a yield. Anything else takes the general step.
    #[inline(never)]
    fn lane_yield_star<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let frame = unsafe { (*cx).frame };
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        if !vm.lane_stack.activations.is_empty()
            || frame + 1 != vm.frames.len()
            || code.function(vm.frames[frame].function as usize).is_async
        {
            return exit(ip);
        }
        let (state, next_method) = crate::bytecode::ImmediateLayout::register_pair(i.imm());
        let destination = i.a & crate::bytecode::REGISTER_MASK;
        // SAFETY: the record lies in the running view.
        let pc = unsafe { ip.offset_from((*cx).base) } as usize;
        // SAFETY: the general loop's pending calls outlive the lane run.
        let pending = unsafe { &mut *(*cx).pending };
        let waiting = r.get(state);
        if waiting.is_undefined() {
            let (delegate, method) = (r.get(i.c), r.get(next_method));
            if !vm.steps_as_generator(delegate, method) {
                return exit(ip);
            }
            return match vm.yield_star_in_place(frame, pc, method, delegate, r.get(destination), state) {
                Some(Ok(StepResult::PushFrame { destination, call_guard, completion })) => {
                    pending.push(PendingGeneralCall {
                        caller: frame,
                        caller_cursor: None,
                        call_pc: pc as u32,
                        destination,
                        completion,
                        returns: false,
                        call_guard: Some(call_guard),
                        caller_program: None,
                    });
                    vm.lane_stack.refresh_activation_limit();
                    lane_enter_script_frame(cx, frame + 1).into_loop()
                }
                Some(Err(error)) => {
                    vm.lane_raised = Some(error);
                    exit(ip)
                }
                _ => exit(ip),
            };
        }
        let Some((value, done)) = vm.own_iterator_result_parts(waiting) else {
            return exit(ip);
        };
        if done {
            r.set(state, Value::UNDEFINED);
            r.set(destination, value);
            next!(cx, r, following(ip), Carried::script(cx))
        }
        if !pending.last().is_some_and(|call| call.completion.is_generator() && call.caller + 1 == frame) {
            return exit(ip);
        }
        r.set(state, Value::UNDEFINED);
        // The frame resumes at this record, which takes the value its caller sends next.
        vm.frames[frame].pc = pc;
        vm.frames[frame].pc_is_resume = true;
        let mut call = pending.pop().expect("a pending call resumed the generator");
        let suspended = vm.frames.pop().expect("the generator's frame is the innermost");
        let stop = super::generator::InLoopGeneratorStop::Yielded { destination, delegated_result: Some(waiting) };
        let resume = call.completion.take_generator().expect("a generator call");
        let result = vm.finish_in_loop_generator_step(suspended, resume, Value::UNDEFINED, stop);
        drop(call.call_guard);
        vm.active_registers = vm.frames[call.caller].registers.as_mut_ptr();
        vm.write(call.caller, call.destination, result);
        resume!(cx, lane_enter_script_frame(cx, call.caller))
    }
}

/// The handlers of a generator frame's yields (`lane_yield`, `lane_yield_star`).
pub(super) const fn yield_handler<H: Host>(opcode: Op) -> Option<Handler> {
    match opcode.class() {
        Class::Yield => Some(lane_yield::<H>),
        Class::YieldStar => Some(lane_yield_star::<H>),
        _ => None,
    }
}
