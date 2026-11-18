//! The lane's suspensions: `yield` and `yield*` of a generator the lane runs over its record's
//! register file (`generator_windows`), and the `await` of a promise job's activation. Each
//! suspends the activation in place; any other `yield` or `yield*`, a materialized generator
//! frame's, takes the general path, which suspends the frame back into its generator.
use super::*;
use super::generator_windows::{lane_yield_in_window, lane_yield_star_in_window, running_generator};
use super::async_requests::running_request;
use super::lane_frame::lane_await_source;
use super::super::activation::{JobRoot, SuspendKind};

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
        if running_request(unsafe { vm(cx) }) {
            resume!(cx, super::async_requests::lane_suspend_request(cx, r, ip, i, true))
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
                let Some(source) = lane_await_source(cx, r, value) else {
                    return exit(ip);
                };
                value = source;
                note_allocation(vm);
                vm.suspend_async_generator_yield(job, pc + 1, destination, value, 0);
            } else {
                vm.suspend_job_window(job, pc + 1, destination, value, 0);
            }
            vm.lane_stack.pop_activation();
            vm.lane_completion.set(LaneCompletion::Suspended(value));
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
    /// What an `await` of `source` (`lane_await_source`) by the activation of `job` evaluates
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
    fn lane_await<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let completion = vm.lane_stack.activations.last().and_then(Activation::script).map(|script| script.completion);
        if completion == Some(Completion::SettlesPromise) {
            resume!(cx, lane_await_settling(cx, r, ip, i))
        }
        if completion == Some(Completion::AsyncRequest) {
            resume!(cx, super::async_requests::lane_suspend_request(cx, r, ip, i, false))
        }
        // Only the job's own activation suspends into its owner.
        let (LaneRoot::Job(job), Some(Completion::ResumesJob)) = (unsafe { (*cx).root }, completion) else {
            return exit(ip);
        };
        let Some(value) = lane_await_source(cx, r, r.get(i.b)) else {
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
        // SAFETY: the record lies in the running view.
        let pc = unsafe { ip.offset_from((*cx).base) } as usize;
        vm.suspend_job_window(job, pc + 1, i.a & crate::bytecode::REGISTER_MASK, value, 0);
        vm.lane_stack.pop_activation();
        vm.lane_completion.set(LaneCompletion::Suspended(value));
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

/// `await` of an async body its caller's run called and settles (`Completion::SettlesPromise`):
/// its registers, and for an environment activation the environment and locals its window holds
/// (`settling_frame`), become the frame of the continuation the awaited value's reaction resumes
/// (a job, `Vm::run_lane_job`), the activation pops, and its caller goes on with the call's
/// promise, as the general loop's in-place async call leaves it (`StepResult::Suspend(Suspended::Await)`). A body
/// that kept the values it was passed, or whose call returns its completion, takes the general
/// path.
#[inline(never)]
fn lane_await_settling<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, i: &LaneInstruction) -> Resumed {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let activation = *vm.lane_stack.activations.last().expect("a settling activation runs");
    let ActivationKind::Script(script) = activation.kind else { unreachable!("a settling activation is a script one") };
    // SAFETY: the call record belongs to the caller's live view.
    let returns = unsafe { (*activation.call).a } & crate::bytecode::RETURN_REGISTER != 0;
    let Some(value) = (!script.forwards && !returns).then(|| lane_await_source(cx, r, r.get(i.b))).flatten() else {
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
