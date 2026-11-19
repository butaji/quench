//! The lane's suspensions: `yield` and `yield*` of a generator the lane runs over its record's
//! register file (`generator_windows`) or of an async generator's in-place request, and the
//! `await` of a promise job's activation or a request's. Each runs its staged row on the lane's
//! frame (`lane_frame::lane_suspension`), which suspends the activation in place
//! (`lane_suspend_at`); any other `yield` or `yield*`, a materialized generator frame's, and any
//! step the lane's frame declines take the general path, which suspends the frame back into its
//! generator.
use super::*;
use super::lane_frame::{lane_suspension, yield_star_kind, Suspender, LANE_YIELD};
use super::super::activation::{JobRoot, SuspendKind};
use crate::staged::{SuspendEffect, Suspending, Suspension};

script_handler! {
    /// `yield` of a generator activation that suspends in place (`Suspender::of_yield`): a
    /// window's frame suspends into its record and the caller continues with the step's result;
    /// a request's parks, its record awaiting the value before the request settles; the root of
    /// a job's run suspends into its record and ends the run (`lane_suspend_at`). Any other
    /// `yield` takes the general path.
    #[inline(never)]
    fn lane_yield<>(cx, r, ip, i, view) {
        // A generator's window, the common case, runs the row with its suspender known.
        if super::generator_windows::running_generator(unsafe { vm(cx) }) {
            resume!(cx, lane_suspension(cx, r, ip, i, (LANE_YIELD, Suspender::Window)))
        }
        let Some(suspender) = Suspender::of_yield(cx) else {
            return exit(ip);
        };
        resume!(cx, lane_suspension(cx, r, ip, i, (LANE_YIELD, suspender)))
    }
}

/// The pc of the record at `ip` in the running view.
#[inline(always)]
fn record_pc<H: Host>(cx: *mut LaneContext<H>, ip: Ip) -> usize {
    // SAFETY: the record lies in the running view.
    unsafe { ip.offset_from((*cx).base) as usize }
}

/// The suspension a row on the lane's frame makes of the record at `ip` (`suspending`), in place
/// as `suspender` says: the frame resumes at this record (a `yield*` step,
/// `Suspending::again`) or after it, with the value it is resumed with in the row's
/// destination. A window's frame suspends into its generator and its caller takes the step's
/// result (`generator_windows::lane_suspend_generator`); a request's parks
/// (`async_requests::lane_suspend_request`); a job root's suspends into its owner and the run
/// ends (`suspend_job_root`), unless an `await` continues in place (`settled_await_in_place`);
/// a settling body hands its frame over (`suspend_settling`). `r` holds the record's registers.
#[inline(always)]
pub(super) fn lane_suspend_at<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, suspender: Suspender,
    suspending: Suspending<u64>) -> Resumed {
    let at = (record_pc(cx, ip) + usize::from(!suspending.again), suspending.destination);
    let (value, yielded) = (Value::from_bits(suspending.value), suspending.effect == SuspendEffect::Yield);
    match suspender {
        Suspender::Window => {
            let delegated = suspending.delegated.map(Value::from_bits);
            super::generator_windows::lane_suspend_generator(cx, at.0, at.1, value, delegated)
        }
        Suspender::Request => super::async_requests::lane_suspend_request(cx, at, yielded, value),
        Suspender::JobRoot(job) => {
            // The converted value may be a fresh promise, which the safepoints after the run collect.
            note_allocation(unsafe { vm(cx) });
            if let Some(settled) = settled_await_in_place(cx, job, suspending) {
                r.set(at.1, settled);
                return Resumed::At(r, following(ip), Carried::script(cx));
            }
            suspend_job_root(cx, at, job, value, yielded);
            Resumed::Exit(exit(ip))
        }
        Suspender::Settling => suspend_settling(cx, r, at, value),
    }
}

/// The value an `await` of `suspending` by the activation of `job` continues with in place, as
/// `Vm::settled_await` answers it, while the lane has fuel for the continuation; `None` where
/// the activation suspends. A `yield` never continues in place.
#[inline(always)]
fn settled_await_in_place<H: Host>(cx: *mut LaneContext<H>, job: JobRoot, suspending: Suspending<u64>) -> Option<Value> {
    if suspending.effect != SuspendEffect::Await {
        return None;
    }
    let settled = unsafe { vm(cx) }.settled_await(job, Value::from_bits(suspending.value))?;
    (!script_fuel_out(cx)).then_some(settled)
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
    fn lane_yield_star<>(cx, r, ip, i, view) {
        if !super::generator_windows::running_generator(unsafe { vm(cx) }) {
            return exit(ip);
        }
        resume!(cx, lane_suspension(cx, r, ip, i, (yield_star_kind(i), Suspender::Window)))
    }
}

impl<H: Host> Vm<H> {
    /// What an `await` of `source` (the row's `await_source`) by the activation of `job` evaluates
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
    /// `await` of the activation the lane runs, by the suspender `Suspender::of_await` chooses: an
    /// async generator's request parks (`Suspender::Request`), a settling body hands its frame to
    /// the continuation (`Suspender::Settling`), and the activation a promise job's run is rooted
    /// in suspends into its owner (`Suspender::JobRoot`). A request is the common case and runs
    /// the row with its suspender known. Any other `await` takes the general path, which a
    /// conversion that could run guest code, or a settling body that keeps its values, reaches.
    #[inline(never)]
    fn lane_await<>(cx, r, ip, i, view) {
        if super::async_requests::running_request(unsafe { vm(cx) }) {
            resume!(cx, lane_suspension(cx, r, ip, i, (Suspension::Await, Suspender::Request)))
        }
        let Some(suspender) = Suspender::of_await(cx) else {
            return exit(ip);
        };
        resume!(cx, lane_suspension(cx, r, ip, i, (Suspension::Await, suspender)))
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

/// `await` of an async body its caller's run called and settles (`Completion::SettlesPromise`),
/// suspending as `Suspender::of_await` allowed (a body that kept the values it was passed, or
/// whose call returns its completion, takes the general path): its registers, and for an
/// environment activation the environment and locals its window holds (`settling_frame`), become
/// the frame of the continuation the awaited value's reaction resumes (a job, `Vm::run_lane_job`),
/// the activation pops, and its caller goes on with the call's promise, as the general loop's
/// in-place async call leaves it (`StepResult::Suspend(Suspended::Await)`). `(pc, destination)`
/// is where the continuation resumes, and `value` the converted awaited value.
#[inline(never)]
fn suspend_settling<H: Host>(cx: *mut LaneContext<H>, r: Registers, (pc, destination): (usize, crate::bytecode::Register),
    value: Value) -> Resumed {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let activation = *vm.lane_stack.activations.last().expect("a settling activation runs");
    // The converted value may be a fresh promise, which the safepoints after the run collect.
    note_allocation(vm);
    let frame = settling_frame(vm, code, &activation, (r, pc));
    let root = vm.lane_stack.settling.pop().expect("each settling activation has its entry");
    let root = vm.settling_root(root);
    let promise = vm.release_settled_call_root(root);
    let continuation = vm.suspend_activation(frame, super::super::activation::Completion::Await(value), Some(destination), promise);
    vm.lane_stack.pop_activation();
    let (memory, _) = lane_restore_caller(cx, &activation);
    match vm.enqueue_async_resume(code, crate::heap::AwaitKind::function(continuation), promise, value) {
        Ok(()) => lane_resume_after_call(&activation, memory, promise),
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
    let depth = vm.lane_stack.activations.len() - 1;
    vm.lane_stack.take_iterators(depth, &mut frame.active_iterators);
    vm.persist_global_lexical_bindings(code, &frame);
    frame
}

/// The `argc` values passed to the environment `activation` of a function of `count`
/// registers: kept after its environment and locals (`lane_activate_environment`).
///
/// # Safety
/// `activation` is a live environment activation, whose window nothing writes meanwhile.
unsafe fn environment_passed<'a>(code: &ResidualProgram, activation: &'a Activation, count: usize, argc: usize) -> &'a [Value] {
    let kept = count + usize::from(WINDOW_LOCALS) + usize::from(code.function(activation.function as usize).locals);
    // SAFETY: as the caller guarantees.
    &(unsafe { activation.window(kept + argc) })[kept..]
}

/// The environment activation (`lane_activate_environment`) of settling `target`, called at
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
    let entered = lane_activate_environment(cx, r, ip, target, passed, (this, false));
    let vm = unsafe { vm(cx) };
    if entered.is_ok() {
        let activation = vm.lane_stack.activations.last_mut().expect("the activation was pushed");
        if let ActivationKind::Script(script) = &mut activation.kind {
            script.completion = Completion::SettlesPromise;
        }
    } else if let Some(root) = vm.lane_stack.settling.pop().flatten() {
        vm.heap.release_root(root);
    }
    entered
}
