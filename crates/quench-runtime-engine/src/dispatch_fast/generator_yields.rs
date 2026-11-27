//! The lane's suspensions: `yield` and `yield*` of a generator the lane runs over its record's
//! register file (`generator_windows`) or of an async generator's in-place request, and the
//! `await` of a promise job's activation or a request's. Each runs its staged row on the lane's
//! frame (`lane_frame::suspension_row_in_lane`), which suspends the activation in place
//! (`suspend_at_in_lane`); any other `yield` or `yield*`, a materialized generator frame's, and any
//! step the lane's frame declines take the general path, which suspends the frame back into its
//! generator.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::lane_frame::{suspension_row_in_lane, yield_star_kind, Suspender, LANE_YIELD};
use quench_runtime::vm::activation::{JobRoot, SuspendKind};
use quench_runtime::staged::{SuspendEffect, Suspending, Suspension};

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

/// The suspension a row on the lane's frame makes of the record at `ip` (`suspending`), in place
/// as `suspender` says: the frame resumes at this record (a `yield*` step,
/// `Suspending::again`) or after it, with the value it is resumed with in the row's
/// destination. A window's frame suspends into its generator and its caller takes the step's
/// result (`generator_windows::lane_suspend_generator`); a request's parks
/// (`async_requests::lane_suspend_request`); a job root's suspends into its owner and the run
/// ends (`suspend_job_root`), unless an `await` continues in place (`settled_await_in_place`);
/// a settling body hands its frame over (`suspend_settling`). `r` holds the record's registers.
#[inline(always)]
pub(crate) fn lane_suspend_at(cx: *mut LaneContext, r: Registers, ip: Ip, suspender: Suspender,
    suspending: Suspending<u64>) -> Resumed {
    // SAFETY: the record lies in the running view.
    let at = (unsafe { record_pc(cx, ip) } + usize::from(!suspending.again), suspending.destination);
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
fn settled_await_in_place(cx: *mut LaneContext, job: JobRoot, suspending: Suspending<u64>) -> Option<Value> {
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
fn suspend_job_root(cx: *mut LaneContext, (pc, destination): (usize, quench_runtime::bytecode::Register), job: JobRoot,
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

#[quench_macros::ext(DispatchFastGeneratorYieldsVm)]
impl Vm {
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
        (record.state() == quench_runtime::tier_surface::PromiseState::Fulfilled).then(|| record.result())
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
    fn await_row<>(cx, r, ip, i, view) {
        if super::async_requests::running_request(unsafe { vm(cx) }) {
            resume!(cx, suspension_row_in_lane(cx, r, ip, i, (Suspension::Await, Suspender::Request)))
        }
        let Some(suspender) = Suspender::of_await(cx) else {
            return exit(ip);
        };
        resume!(cx, suspension_row_in_lane(cx, r, ip, i, (Suspension::Await, suspender)))
    }
}

/// The handlers of a frame's suspensions: each runs its staged `Suspend` row on the lane's frame
/// (`suspension_row_in_lane`), with the suspender its record's activation makes.
pub(crate) const fn suspend_handler(opcode: Op) -> Option<Handler> {
    use quench_runtime::staged::SuspendRow;
    match quench_runtime::staged::suspend_row(opcode) {
        Some(SuspendRow::Yield) => Some(yield_row),
        Some(SuspendRow::YieldStar) => Some(yield_star_row),
        Some(SuspendRow::Await) => Some(await_row),
        None => None,
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
fn suspend_settling(cx: *mut LaneContext, r: Registers, (pc, destination): (usize, quench_runtime::bytecode::Register),
    value: Value) -> Resumed {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let top = vm.lane_stack.top_frame().expect("a settling activation runs");
    let record = top.record();
    // The converted value may be a fresh promise, which the safepoints after the run collect.
    note_allocation(vm);
    let frame = quench_runtime::vm::activation::FrameRecord::capture(settling_frame(vm, code, &record, (r, pc)));
    let promise = vm.settling_promise(top.payload().settles());
    let continuation = vm.suspend_activation(frame, quench_runtime::vm::activation::Completion::Await(value), Some(destination), promise);
    let activation = vm.lane_stack.pop_activation().expect("a settling activation runs");
    let (memory, _) = restore_caller_in_lane(cx, &activation);
    match vm.enqueue_async_resume(code, quench_runtime::tier_surface::AwaitKind::function(continuation), promise, value) {
        Ok(()) => resume_after_call_in_lane(&activation, memory, promise),
        Err(error) => {
            vm.lane_completion.raise(error);
            Resumed::Exit(exit(activation.call()))
        }
    }
}

/// The frame of the settling `activation` running with registers `r`, which resumes at `pc`:
/// its registers, receiver and environment, with the loops it opened (`LaneStack::iterators`).
/// A plain activation's frame is made off the frame stack with its window's registers moved in
/// once (`Vm::detached_plain_frame`); an environment activation's (`pushed_settling_frame`)
/// owns the environment its window made, or makes it now, and takes the window's locals, as
/// its materialization would (`materialize_environment_frame`).
///
/// A settling activation is an async function's call, never a program's root, so no global
/// lexical binding is published here (`Vm::persist_global_lexical_bindings`).
fn settling_frame(vm: &mut Vm, code: &ResidualProgram, activation: &Activation, (r, pc): (Registers, usize)) -> Box<quench_runtime::vm::Frame> {
    let ActivationKind::Script(script) = activation.kind else { unreachable!("a settling activation is a script one") };
    let count = usize::from(code.function(activation.function as usize).registers);
    let argc = usize::from(script.argc);
    // SAFETY: the activation's window holds its registers, which nothing writes until the frame takes them.
    let window = unsafe { activation.window(count) };
    let context = CallContext::user_function(activation.function, script.callee);
    let detached = if script.environment {
        None
    } else {
        let passed = &window[..argc.min(count)];
        vm.detached_plain_frame(code, activation.function, (r.env(), r.this()), context, (passed, window))
    };
    let mut frame = detached.unwrap_or_else(|| pushed_settling_frame(vm, code, activation, r, (count, argc)));
    frame.pc = pc;
    frame.pc_is_resume = true;
    let depth = vm.lane_stack.activation_count() - 1;
    vm.lane_stack.take_iterators(depth, &mut frame.active_iterators);
    debug_assert_ne!(frame.function, quench_runtime::vm::ROOT_FUNCTION_ID);
    frame
}

/// `settling_frame`'s frame pushed as a full activation and popped again, holding the window's
/// `count` registers and the `argc` values passed: the way any frameless activation becomes a
/// frame (`Vm::push_script_frame_with_registers`), and an environment activation's then takes
/// its environment and locals (`materialize_environment_frame`).
fn pushed_settling_frame(vm: &mut Vm, code: &ResidualProgram, activation: &Activation, r: Registers,
    (count, argc): (usize, usize)) -> Box<quench_runtime::vm::Frame> {
    let ActivationKind::Script(script) = activation.kind else { unreachable!("a settling activation is a script one") };
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
    frame.with_objects = vm.with_stack.split_off(frame.with_base);
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
/// promise, whose root its side record keeps (`Completion::SettlesPromise`, `SidePayload::Settles`).
/// An activation refused leaves the caller to release it, before any effect.
pub(crate) fn activate_settling_environment(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    passed: &[Value],
    this: Value,
) -> Result<(Registers, Ip), Ip> {
    let entered = activate_environment_in_lane(cx, r, ip, target, passed, (this, false));
    let vm = unsafe { vm(cx) };
    // The caller attaches the promise's root to the activation (`SidePayload::Settles`), and
    // releases it where the activation is refused.
    if entered.is_ok() {
        vm.lane_stack.recomplete_top(Completion::SettlesPromise);
    }
    entered
}
