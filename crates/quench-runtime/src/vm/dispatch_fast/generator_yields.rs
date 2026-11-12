//! The lane's suspensions: the yields of generators their callers resumed in place, `yield` and
//! `yield*` of a delegate generator, and the `await` of a promise job's activation. A generator
//! the lane resumed over its record's register file suspends in place (`generator_windows`); one
//! it resumed as a frame suspends to its caller here; a job's activation suspends into its owner.
use super::*;
use super::generator_windows::{lane_yield_in_window, lane_yield_star_in_window, running_generator};

script_handler! {
    /// `yield` of the frame of a generator its caller resumed in place
    /// (`PendingGeneralCall::completion`), made as the general loop makes it: the frame
    /// suspends into the generator at the record after this one, and the caller continues
    /// with the step's result (`Vm::finish_in_loop_generator_step`). Any other `yield`
    /// takes the general path.
    #[inline(never)]
    fn lane_yield<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        if running_generator(vm) {
            resume!(cx, lane_yield_in_window(cx, r, ip, i))
        }
        // SAFETY: the general loop's pending calls outlive the lane run.
        let pending = unsafe { &mut *(*cx).pending };
        let frame = unsafe { (*cx).frame() };
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
        if running_generator(vm) {
            resume!(cx, lane_yield_star_in_window(cx, r, ip, i))
        }
        let frame = unsafe { (*cx).frame() };
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

impl<H: Host> Vm<H> {
    /// What `await` of `awaited` waits on, as `PromiseResolve(%Promise%, awaited)` converts it
    /// where that observes nothing: a non-object as it is, an object by `lane_promise_resolve`.
    /// `None` leaves a conversion that could run guest code, or throw into the frame's
    /// handlers, to the general path.
    fn lane_await_source(&mut self, code: &ResidualProgram, awaited: Value) -> Option<Value> {
        if self.is_object_like(awaited) {
            return self.lane_promise_resolve(code, self.intrinsic_promise_constructor(), awaited);
        }
        Some(awaited)
    }
}

script_handler! {
    /// `await` of the activation a promise job's lane run is rooted in (`LaneRoot::Job`): the
    /// frame suspends into its owner, resuming after this record with the awaited value in its
    /// register, the activation pops, and the run ends for the job to attach the await's
    /// reactions (`Vm::suspend_job_window`). Any other `await`, a conversion that could run
    /// guest code included, takes the general path.
    #[inline(never)]
    fn lane_await<>(cx, r, ip, i, view) {
        let LaneRoot::Job(job) = (unsafe { (*cx).root }) else {
            return exit(ip);
        };
        let vm = unsafe { vm(cx) };
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let Some(value) = vm.lane_await_source(code, r.get(i.b)) else {
            return exit(ip);
        };
        // The source may be a fresh promise, which the safepoints after the run collect.
        note_allocation(vm);
        // SAFETY: the record lies in the running view.
        let pc = unsafe { ip.offset_from((*cx).base) } as usize;
        vm.suspend_job_window(job, pc + 1, i.a & crate::bytecode::REGISTER_MASK, value, 0);
        vm.lane_stack.pop_activation();
        vm.lane_awaited = Some(value);
        exit(ip)
    }
}

/// The handlers of a frame's suspensions (`lane_yield`, `lane_yield_star`, `lane_await`).
pub(super) const fn suspend_handler<H: Host>(opcode: Op) -> Option<Handler> {
    match opcode.class() {
        Class::Yield => Some(lane_yield::<H>),
        Class::YieldStar => Some(lane_yield_star::<H>),
        Class::Await => Some(lane_await::<H>),
        _ => None,
    }
}
