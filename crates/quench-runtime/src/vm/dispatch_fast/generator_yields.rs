//! The lane's suspensions: `yield` and `yield*` of a generator the lane runs over its record's
//! register file (`generator_windows`), and the `await` of a promise job's activation. Each
//! suspends the activation in place; any other `yield` or `yield*`, a materialized generator
//! frame's, takes the general path, which suspends the frame back into its generator.
use super::*;
use super::generator_windows::{yield_in_window, yield_star_in_window, running_generator};
use super::super::activation::SuspendKind;

/// `yield` of the running generator activation, which a `Yield` row transfers to
/// (`Frame::suspend_yield`): the frame suspends in place and the caller continues with the step's
/// result (`yield_in_window`). `None` where no generator runs the lane, which `yield_miss` takes.
///
/// # Safety
/// `cx` is the running lane's context, `record` the running record of its view, and `r` its register file.
#[inline(never)]
pub(super) unsafe fn yield_in_lane<H: Host>(cx: *mut (), record: Ip, r: Registers, value: Register) -> Option<Resumed> {
    let cx = cx.cast::<LaneContext<H>>();
    if !running_generator(unsafe { vm(cx) }) {
        return None;
    }
    // SAFETY: the record lies in the running view.
    Some(yield_in_window(cx, r, record, unsafe { &*record }, value))
}

/// `yield*` of a generator activation the lane runs over its record, which a `YieldStar` row
/// transfers to (`Frame::suspend_yield_star`) (`yield_star_in_window`). `None` where no
/// generator runs the lane, which `yield_star_miss` takes.
///
/// # Safety
/// `cx` is the running lane's context, `record` the running record of its view, and `r` its register file.
#[inline(never)]
pub(super) unsafe fn yield_star_in_lane<H: Host>(cx: *mut (), record: Ip, r: Registers, iterable: Register) -> Option<Resumed> {
    let cx = cx.cast::<LaneContext<H>>();
    if !running_generator(unsafe { vm(cx) }) {
        return None;
    }
    // SAFETY: the record lies in the running view.
    Some(yield_star_in_window(cx, r, record, unsafe { &*record }, iterable))
}

script_handler! {
    /// `yield` the frame does not suspend in place (`yield_in_lane` declined: no running generator).
    /// The root of a native `next`'s run (`LaneRoot::Job` of a sync generator) suspends into its record
    /// and ends the run with the yielded value (`Vm::suspend_job_window`); an async generator's root
    /// parks likewise, its record awaiting the value before the request settles
    /// (`Vm::suspend_async_generator_yield`). Any other `yield` takes the general path.
    #[inline(never)]
    pub(super) fn yield_miss<>(cx, r, ip, i, view) {
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
                let Some(source) = vm.await_source_in_lane(unsafe { &*(*cx).code }, value) else {
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
    /// `yield*` the frame does not suspend in place (`yield_star_in_lane` declined): the general path.
    #[inline(never)]
    pub(super) fn yield_star_miss<>(cx, r, ip, i, view) {
        exit(ip)
    }
}

impl<H: Host> Vm<H> {
    /// What `await` of `awaited` waits on, as `PromiseResolve(%Promise%, awaited)` converts it
    /// where that observes nothing: a non-object as it is, an object by `lane_promise_resolve`.
    /// `None` leaves a conversion that could run guest code, or throw into the frame's
    /// handlers, to the general path.
    fn await_source_in_lane(&mut self, code: &ResidualProgram, awaited: Value) -> Option<Value> {
        if self.is_object_like(awaited) {
            return self.lane_promise_resolve(code, self.intrinsic_promise_constructor(), awaited);
        }
        Some(awaited)
    }
}

/// `Await` of the activation a promise job's lane run is rooted in (`LaneRoot::Job`): the frame
/// suspends into its owner, resuming after the record with the awaited value (converted as
/// `Vm::await_source_in_lane` converts it) in `destination`, the activation pops, and the run ends for
/// the job to attach the await's reactions (`Vm::suspend_job_window`). Any other `await`, and a
/// conversion that could run guest code, leave to the general step (`None`, nothing changed).
///
/// # Safety
/// `cx` is the running lane's context, `record` the running record of its view, and `r` its register file.
#[inline(never)]
pub(super) unsafe fn await_in_lane<H: Host>(cx: *mut (), record: *const LaneInstruction, r: Registers,
    destination: Register, source: Register) -> Option<()> {
    let cx = cx.cast::<LaneContext<H>>();
    let LaneRoot::Job(job) = (unsafe { (*cx).root }) else { return None };
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let value = vm.await_source_in_lane(code, r.get(source))?;
    // The source may be a fresh promise, which the safepoints after the run collect.
    note_allocation(vm);
    // SAFETY: the record lies in the running view.
    let pc = unsafe { record.offset_from((*cx).base) } as usize;
    vm.suspend_job_window(job, pc + 1, destination, value, 0);
    vm.lane_stack.pop_activation();
    vm.lane_suspended = Some(value);
    Some(())
}
