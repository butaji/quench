//! The lane's suspensions: `yield` and `yield*` of a generator the lane runs over its record's
//! register file (`generator_windows`), and the `await` of a promise job's activation. Each
//! suspends the activation in place; any other `yield` or `yield*`, a materialized generator
//! frame's, takes the general path, which suspends the frame back into its generator.
use super::*;
use super::generator_windows::{lane_yield_in_window, lane_yield_star_in_window, running_generator};
use super::super::activation::SuspendKind;

script_handler! {
    /// `yield` of the generator activation the lane runs over its record (`running_generator`):
    /// the frame suspends in place and the caller continues with the step's result
    /// (`lane_yield_in_window`). The root of a native `next`'s run (`LaneRoot::Job` of a sync
    /// generator) suspends into its record and ends the run with the yielded value
    /// (`Vm::suspend_job_window`); an async generator's root parks likewise, its record awaiting
    /// the value before the request settles (`Vm::suspend_async_generator_yield`). Any other
    /// `yield` takes the general path.
    #[inline(never)]
    fn lane_yield<>(cx, r, ip, i, view) {
        if running_generator(unsafe { vm(cx) }) {
            resume!(cx, lane_yield_in_window(cx, r, ip, i))
        }
        if let LaneRoot::Job(job) = unsafe { (*cx).root }
            && matches!(job.kind, SuspendKind::Generator | SuspendKind::AsyncGenerator)
        {
            let vm = unsafe { vm(cx) };
            let mut value = r.get(i.b);
            // SAFETY: the record lies in the running view.
            let pc = unsafe { ip.offset_from((*cx).base) } as usize;
            let destination = i.a & crate::bytecode::REGISTER_MASK;
            if job.kind == SuspendKind::AsyncGenerator {
                // The yield awaits its value, converted as an `await`'s source is.
                // SAFETY: the context's program outlives the lane run.
                let Some(source) = vm.lane_await_source(unsafe { &*(*cx).code }, value) else {
                    return exit(ip);
                };
                value = source;
                note_allocation(vm);
                vm.suspend_async_generator_yield(job, pc + 1, destination, value);
            } else {
                vm.suspend_job_window(job, pc + 1, destination, value, 0);
            }
            vm.lane_stack.pop_activation();
            vm.lane_suspended = Some(value);
        }
        exit(ip)
    }
}

script_handler! {
    /// `yield*` of a generator activation the lane runs over its record
    /// (`lane_yield_star_in_window`). Any other `yield*` takes the general step.
    #[inline(never)]
    fn lane_yield_star<>(cx, r, ip, i, view) {
        if running_generator(unsafe { vm(cx) }) {
            resume!(cx, lane_yield_star_in_window(cx, r, ip, i))
        }
        exit(ip)
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
        vm.lane_suspended = Some(value);
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
