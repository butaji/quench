//! The lane's suspensions: `yield` and `yield*` of a generator the lane runs over its record's
//! register file (`generator_windows`) or of an async generator's in-place request, and the
//! `await` of a promise job's activation or a request's. Each runs its staged row on the lane's
//! frame (`lane_frame::suspension_row_in_lane`), which suspends the activation in place
//! (`suspend_at_in_lane`); any other `yield` or `yield*`, a materialized generator frame's, and any
//! step the lane's frame declines take the general path, which suspends the frame back into its
//! generator.
use super::*;
use super::lane_frame::{awaited_value_in_lane, suspension_row_in_lane, yield_star_kind, Suspender, LANE_YIELD};
use super::super::activation::{JobRoot, SuspendKind};
use crate::staged::{SuspendEffect, Suspending, Suspension};

script_handler! {
    /// `yield` of a generator activation that suspends in place (`Suspender::of_yield`): a
    /// window's frame suspends into its record and the caller continues with the step's result;
    /// a request's parks, its record awaiting the value before the request settles; the root of
    /// a job's run suspends into its record and ends the run (`suspend_at_in_lane`). Any other
    /// `yield` takes the general path.
    #[inline(never)]
    fn yield_row<>(cx, r, ip, i, view) {
        // A generator's window, the common case, runs the row with its suspender known.
        if super::generator_windows::running_generator(unsafe { vm(cx) }) {
            resume!(cx, suspension_row_in_lane(cx, r, ip, i, (LANE_YIELD, Suspender::Window)))
        }
        let Some(suspender) = Suspender::of_yield(cx) else {
            return exit(ip);
        };
        resume!(cx, suspension_row_in_lane(cx, r, ip, i, (LANE_YIELD, suspender)))
    }
}

/// The pc of the record at `ip` in the running view.
#[inline(always)]
fn record_pc<H: Host>(cx: *mut LaneContext<H>, ip: Ip) -> usize {
    // SAFETY: the record lies in the running view.
    unsafe { ip.offset_from((*cx).base) as usize }
}

/// Where a frame stopping at the `yield` or `await` record `i` at `ip` resumes: the pc after
/// the record, and the register that takes the value it is resumed with (`lane_await`).
#[inline(always)]
fn resume_point<H: Host>(cx: *mut LaneContext<H>, ip: Ip, i: &LaneInstruction) -> (usize, crate::bytecode::Register) {
    (record_pc(cx, ip) + 1, i.a & crate::bytecode::REGISTER_MASK)
}

/// The suspension a row on the lane's frame makes of the record at `ip` (`suspending`), in place
/// as `suspender` says: the frame resumes at this record (a `yield*` step,
/// `Suspending::again`) or after it, with the value it is resumed with in the row's
/// destination. A window's frame suspends into its generator and its caller takes the step's
/// result (`generator_windows::suspend_generator_in_lane`); a request's parks
/// (`async_requests::suspend_request_in_lane`); a job root's suspends into its owner and the run
/// ends (`suspend_job_root`).
#[inline(always)]
pub(super) fn suspend_at_in_lane<H: Host>(cx: *mut LaneContext<H>, ip: Ip, suspender: Suspender, suspending: Suspending<u64>) -> Resumed {
    let at = (record_pc(cx, ip) + usize::from(!suspending.again), suspending.destination);
    let (value, yielded) = (Value::from_bits(suspending.value), suspending.effect == SuspendEffect::Yield);
    match suspender {
        Suspender::Window => {
            let delegated = suspending.delegated.map(Value::from_bits);
            super::generator_windows::suspend_generator_in_lane(cx, at.0, at.1, value, delegated)
        }
        Suspender::Request => super::async_requests::suspend_request_in_lane(cx, at, yielded, value),
        Suspender::JobRoot(job) => {
            // The converted value may be a fresh promise, which the safepoints after the run collect.
            note_allocation(unsafe { vm(cx) });
            suspend_job_root(cx, at, job, value, yielded);
            Resumed::Exit(exit(ip))
        }
    }
}

/// The activation a job's lane run is rooted in (`LaneRoot::Job`) stops with `value`, a `yield`
/// when `yielded`, else an `await`, resuming at `pc` with the value it is resumed with in
/// `destination`: its frame suspends into its owner as its kind does
/// (`Vm::suspend_async_generator_yield` for an async generator's `yield`,
/// `Vm::suspend_job_window` for the rest), the activation pops, and the run ends.
fn suspend_job_root<H: Host>(cx: *mut LaneContext<H>, (pc, destination): (usize, crate::bytecode::Register), job: JobRoot,
    value: Value, yielded: bool) {
    let vm = unsafe { vm(cx) };
    if yielded && job.kind == SuspendKind::AsyncGenerator {
        vm.suspend_async_generator_yield(job, pc, destination, value, 0);
    } else {
        vm.suspend_job_window(job, pc, destination, value, 0);
    }
    vm.lane_stack.pop_activation();
    vm.lane_completion.set(LaneCompletion::Suspended(value));
}

script_handler! {
    /// `yield*` of a generator activation the lane runs over its record
    /// (`generator_windows::running_generator`), by its row on the lane's frame: the delegate's
    /// result read where it is a plain iterator result, a delegate generator stepped in place,
    /// and the result passed on as this generator's own yield. Any other `yield*` takes the
    /// general step.
    #[inline(never)]
    fn yield_star_row<>(cx, r, ip, i, view) {
        if !super::generator_windows::running_generator(unsafe { vm(cx) }) {
            return exit(ip);
        }
        resume!(cx, suspension_row_in_lane(cx, r, ip, i, (yield_star_kind(i), Suspender::Window)))
    }
}

impl<H: Host> Vm<H> {
    /// What an `await` of `source` (`awaited_value_in_lane`) by the activation of `job` evaluates
    /// to without suspending: its reaction would be the only queued job, so suspending, queueing
    /// it and resuming observes nothing more. Only an async function a microtask drain resumed
    /// (`queue_resumed`), with no other job queued and no host watching promises, awaiting a
    /// non-object or a fulfilled promise.
    fn settled_await(&self, job: JobRoot, source: Value) -> Option<Value> {
        if self.queue_resumed != job.object || !self.realm.jobs.is_empty() || self.host.observes_promise_lifecycle() {
            return None;
        }
        if !self.is_object_like(source) {
            return Some(source);
        }
        let record = self.promise_record(source)?;
        (record.state() == crate::heap::PromiseState::Fulfilled).then(|| record.result())
    }
}

script_handler! {
    /// `await` of the activation a promise job's lane run is rooted in (`LaneRoot::Job`): the
    /// frame suspends into its owner, resuming after this record with the awaited value in its
    /// register, the activation pops, and the run ends for the job to attach the await's
    /// reactions (`Vm::suspend_job_window`). Any other `await`, a conversion that could run
    /// guest code included, takes the general path.
    #[inline(never)]
    fn await_row<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let completion = vm.lane_stack.activations().last().and_then(Activation::script).map(|script| script.completion);
        if completion == Some(Completion::SettlesPromise) {
            resume!(cx, await_settling_in_lane(cx, r, ip, i))
        }
        if completion == Some(Completion::AsyncRequest) {
            resume!(cx, suspension_row_in_lane(cx, r, ip, i, (Suspension::Await, Suspender::Request)))
        }
        // Only the job's own activation suspends into its owner.
        let (LaneRoot::Job(job), Some(Completion::ResumesJob)) = (unsafe { (*cx).root }, completion) else {
            return exit(ip);
        };
        let Some(value) = awaited_value_in_lane(cx, r, r.get(i.b)) else {
            return exit(ip);
        };
        // The source may be a fresh promise, which the safepoints after the run collect.
        note_allocation(vm);
        if let Some(settled) = vm.settled_await(job, value)
            && !script_fuel_out(cx)
        {
            r.set(i.a & crate::bytecode::REGISTER_MASK, settled);
            next!(cx, r, following(ip), Carried::script(cx))
        }
        suspend_job_root(cx, resume_point(cx, ip, i), job, value, false);
        exit(ip)
    }
}

/// The handlers of a frame's suspensions: each runs its staged `Suspend` row on the lane's frame
/// (`suspension_row_in_lane`), with the suspender its record's activation makes.
pub(super) const fn suspend_handler<H: Host>(opcode: Op) -> Option<Handler> {
    match opcode.class() {
        Class::Yield => Some(yield_row::<H>),
        Class::YieldStar => Some(yield_star_row::<H>),
        Class::Await => Some(await_row::<H>),
        _ => None,
    }
}

/// `await` of an async body its caller's run called and settles (`Completion::SettlesPromise`):
/// its registers, and for an environment activation the environment and locals its window holds
/// (`settling_frame`), become the frame of the continuation the awaited value's reaction resumes
/// (a job, `Vm::run_lane_job`), the activation pops, and its caller goes on with the call's
/// promise, as the general loop's in-place async call leaves it (`StepResult::Suspend(Suspended::Await)`). A body
/// that kept the values it was passed, or whose call returns its completion, takes the general
/// path.
#[inline(never)]
fn await_settling_in_lane<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, i: &LaneInstruction) -> Resumed {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let activation = *vm.lane_stack.activations().last().expect("a settling activation runs");
    let ActivationKind::Script(script) = activation.kind else { unreachable!("a settling activation is a script one") };
    // SAFETY: the call record belongs to the caller's live view.
    let returns = unsafe { (*activation.call).a } & crate::bytecode::RETURN_REGISTER != 0;
    let Some(value) = (!script.forwards && !returns).then(|| awaited_value_in_lane(cx, r, r.get(i.b))).flatten() else {
        return Resumed::Exit(exit(ip));
    };
    note_allocation(vm);
    // SAFETY: the record lies in the running view.
    let pc = unsafe { ip.offset_from((*cx).base) } as usize;
    let frame = settling_frame(vm, code, &activation, (r, pc + 1));
    let root = vm.lane_stack.settling.pop().expect("each settling activation has its entry");
    let root = vm.settling_root(root);
    let promise = vm.release_settled_call_root(root);
    let destination = i.a & crate::bytecode::REGISTER_MASK;
    let continuation = vm.suspend_activation(frame, super::super::activation::Completion::Await(value), Some(destination), promise);
    vm.lane_stack.pop_activation();
    let (memory, _) = restore_caller_in_lane(cx, &activation);
    match vm.enqueue_async_resume(code, crate::heap::AwaitKind::function(continuation), promise, value) {
        Ok(()) => resume_after_call_in_lane(&activation, memory, promise),
        Err(error) => {
            vm.lane_completion.raise(error);
            Resumed::Exit(exit(activation.call))
        }
    }
}

/// The frame of the settling `activation` running with registers `r`, which resumes at `pc`:
/// its registers, receiver and environment, with the loops it opened (`LaneStack::iterators`).
/// An environment activation's frame owns the environment its window made, or makes it now,
/// and takes the window's locals, as its materialization would (`materialize_environment_frame`).
fn settling_frame<H: Host>(vm: &mut Vm<H>, code: &ResidualProgram, activation: &Activation, (r, pc): (Registers, usize)) -> Box<super::super::Frame> {
    let ActivationKind::Script(script) = activation.kind else { unreachable!("a settling activation is a script one") };
    let count = usize::from(code.function(activation.function as usize).registers);
    let argc = usize::from(script.argc);
    // SAFETY: the activation's window holds its registers, which nothing writes until the frame takes them.
    let window = unsafe { activation.window(count) };
    // SAFETY: an environment activation's window holds its kept values after its locals.
    let passed = if script.environment { unsafe { environment_passed(code, activation, count, argc) } } else { &window[..argc.min(count)] };
    let context = CallContext::user_function(activation.function, script.callee);
    vm.push_script_frame_with_registers(code, activation.function, r.env(), r.this(), context, passed, window);
    if script.environment {
        // A settling activation runs its caller's program (`Completion::of`).
        vm.materialize_environment_frame(code, activation, (activation.caller_program, r.env(), r.this()), argc);
    }
    let mut frame = vm.frames.pop().expect("the activation pushed a frame");
    frame.pc = pc;
    frame.pc_is_resume = true;
    frame.with_objects = vm.with_stack.split_off(frame.with_base);
    let depth = vm.lane_stack.activations().len() - 1;
    vm.lane_stack.take_iterators(depth, &mut frame.active_iterators);
    frame
}

/// The `argc` values passed to the environment `activation` of a function of `count`
/// registers: kept after its environment and locals (`activate_environment_in_lane`).
///
/// # Safety
/// `activation` is a live environment activation, whose window nothing writes meanwhile.
unsafe fn environment_passed<'a>(code: &ResidualProgram, activation: &'a Activation, count: usize, argc: usize) -> &'a [Value] {
    let kept = count + usize::from(WINDOW_LOCALS) + usize::from(code.function(activation.function as usize).locals);
    // SAFETY: as the caller guarantees.
    &(unsafe { activation.window(kept + argc) })[kept..]
}

/// The environment activation (`activate_environment_in_lane`) of settling `target`, called at
/// `ip` with registers `r`, passing `passed` with receiver `this`: its return settles the call's
/// promise, whose root `LaneStack::settling` holds last (`Completion::SettlesPromise`); an
/// activation refused releases it, before any effect.
pub(super) fn activate_settling_environment<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    passed: &[Value],
    this: Value,
) -> Result<(Registers, Ip), Ip> {
    let entered = activate_environment_in_lane(cx, r, ip, target, passed, (this, false));
    let vm = unsafe { vm(cx) };
    if entered.is_ok() {
        let activation = vm.lane_stack.activations_mut().last_mut().expect("the activation was pushed");
        if let ActivationKind::Script(script) = &mut activation.kind {
            script.completion = Completion::SettlesPromise;
        }
    } else if let Some(root) = vm.lane_stack.settling.pop().flatten() {
        vm.heap.release_root(root);
    }
    entered
}
