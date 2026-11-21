use super::activation::{Completion, Continuation};
use super::promise::PromiseState;
use super::*;
use crate::heap::{AwaitKind, AwaitRole, AwaitState};

/// What an await reaction resumes with, read from its `AwaitState`.
#[derive(Clone, Copy, Debug)]
pub(in crate::vm) struct AwaitResume {
    /// The spent state itself, whose reaction is running: the activation's next await
    /// suspends into it again (`Vm::await_again`). `None` for the run of an async generator's
    /// request, which awaits in a state of its own.
    state: Option<Value>,
    promise: Value,
    role: AwaitRole,
    rejected: bool,
}

impl Vm {
    /// `Continuation::suspend` of `frame` at an await or yield, with the registers liveness
    /// finds live there (`suspension_registers`): a collection while it is suspended traces
    /// those alone, so a dead temporary does not stay reachable until it resumes.
    pub(super) fn suspend_activation(
        &self,
        frame: Box<super::Frame>,
        completion: Completion,
        resume_register: Option<u16>,
        promise: Value,
    ) -> Continuation {
        let live = self.suspension_registers(frame.program, frame.function, frame.pc);
        let mut frame = frame;
        // A suspended body waits with its registers alone: the spare room a running frame keeps
        // for the windows a loop region inlines (`RegisterFile::new`, about 800 bytes) is
        // given back, and a resume that runs a region past the registers grows the file again
        // or takes the general path (`frame_reaches_in_lane`). doxbee-async kept 10K bodies
        // suspended at once, 9 MB of that room.
        let registers = frame.registers.len();
        frame.registers.shrink_to(registers);
        let mut continuation = Continuation::suspend(frame, completion, resume_register, promise);
        continuation.frame.suspend_registers(live);
        continuation
    }

    /// The registers of a frame of `function` of `program` suspending to resume at `pc` that
    /// are live there (its root mask at `pc`), when liveness covers the function, with the
    /// collection epoch: the dead ones keep their values until it resumes, and are cleared
    /// then only if a collection ran meanwhile (`FrameRecord::settle_registers`). A windowed
    /// mask leaves every register traced.
    pub(in crate::vm) fn suspension_registers(
        &self,
        program: super::ProgramId,
        function: u32,
        pc: usize,
    ) -> (Option<crate::compile::liveness::RegisterSet>, u64) {
        let masks = self.programs.residual(program).and_then(|program| program.register_root_masks(function));
        let live = masks.filter(|masks| !masks.is_windowed()).and_then(|masks| masks.get(pc));
        (live, self.heap.collection_epoch())
    }

    pub(super) fn call_user_maybe_async(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        env: Value,
        this: Value,
        args: &[Value],
        context: CallContext,
    ) -> Result<Value, JsError> {
        if p.function(id as usize).is_generator {
            return self.call_generator(p, id, env, this, args, context);
        }
        if !p.function(id as usize).is_async {
            return self.call_user(p, id, env, this, args, context);
        }
        // Init observers run before the async frame exists. Keep its incoming
        // closure environment, receiver and arguments alive across that callback.
        let promise = self.with_call_roots(
            [env, this].into_iter().chain(args.iter().copied()).chain(context.callee()),
            |vm| vm.promise_object(),
        )?;
        self.with_call_roots([promise], |vm| {
            if id == super::ROOT_FUNCTION_ID {
                vm.start_root_job(p, promise, env, this, context)?;
                return Ok(promise);
            }
            let outcome = match vm.call_user_frame(p, id, env, this, args, context) {
                Ok(outcome) => outcome,
                Err(error) => {
                    vm.reject_settled_call(p, promise, error)?;
                    return Ok(promise);
                }
            };
            vm.settle_async_call(p, promise, outcome)?;
            Ok(promise)
        })
    }

    /// Settles `promise`, the result of an async call, by the outcome of its first run, as any
    /// async activation's stop settles (`Vm::settle_stop`): a return resolves it, an await
    /// suspends the frame its outcome carries into a continuation resumed when the awaited value
    /// settles.
    fn settle_async_call(&mut self, p: &ResidualProgram, promise: Value, outcome: super::FrameOutcome) -> Result<(), JsError> {
        let resume = AwaitResume { state: None, promise, role: AwaitRole::AsyncFunction, rejected: false };
        self.settle_stop(p, StopOwner::Async(resume), None, Ok(outcome)).map(drop)
    }

    /// Starts the body of an async root, a module with a top-level `await`, as a job over its
    /// evaluation record: its frame, bound at its first instruction, is the continuation an
    /// await state keeps, which `resume_await` runs as an await's reaction does. The lane runs
    /// it from its first instruction, so its first `await` suspends a window and no frame
    /// (`Vm::run_job`); the body's end settles `promise`.
    fn start_root_job(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        env: Value,
        this: Value,
        context: CallContext,
    ) -> Result<(), JsError> {
        let frame = match self.unstarted_frame(p, super::ROOT_FUNCTION_ID, env, this, context) {
            Ok(frame) => frame,
            Err(error) => return self.reject_settled_call(p, promise, error),
        };
        let continuation = Continuation::suspend(frame, Completion::Await(Value::UNDEFINED), None, promise);
        let kind = AwaitKind::function(continuation);
        let state = self.heap.alloc_await_state(AwaitState::arm(kind, self.realm.globals, promise));
        // A due collection runs first, where none of the body is on the stack (`resume_await`).
        self.with_call_roots([state], |vm| {
            vm.maybe_collect(p);
            vm.resume_await(p, state, false, Value::UNDEFINED)
        })
        .map(drop)
    }

    /// `call_user_maybe_async` of async function `id` of `p` that a lane run calls: its frame
    /// starts as a job over its record, as an async root's does (`start_root_job`), so the run
    /// stays in the lane, beside the caller's activations, and its first `await` suspends the
    /// window in place (`Vm::run_job`). The call's promise.
    pub(in crate::vm) fn start_async_call(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        env: Value,
        this: Value,
        args: &[Value],
        context: CallContext,
    ) -> Result<Value, JsError> {
        let promise = self.with_call_roots(
            [env, this].into_iter().chain(args.iter().copied()).chain(context.callee()),
            |vm| vm.promise_object(),
        )?;
        self.with_call_roots([promise], |vm| {
            let frame = match vm.unstarted_frame_with(p, id, env, this, args, context) {
                Ok(frame) => frame,
                Err(error) => return vm.reject_settled_call(p, promise, error).map(|()| promise),
            };
            let continuation = Continuation::suspend(frame, Completion::Await(Value::UNDEFINED), None, promise);
            let kind = AwaitKind::function(continuation);
            let state = vm.armed_await_state(kind, promise);
            vm.with_call_roots([state], |vm| vm.start_await_job(p, state))?;
            Ok(promise)
        })
    }

    /// Rejects an async call's `promise` with the error its body completed with.
    pub(super) fn reject_settled_call(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        error: JsError,
    ) -> Result<(), JsError> {
        let reason = error
            .thrown_value()
            .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
        self.promise_settle(p, promise, PromiseState::Rejected, reason)
    }

    /// The rooted promise of a settling lane call (`SidePayload::Settles`), made now for a call
    /// that deferred it to its awaiting caller: something other than the await may read it.
    /// Without lifecycle hooks, which settling calls exclude, making it runs no guest code.
    pub(super) fn settling_root(&mut self, root: Option<crate::heap::RootId>) -> crate::heap::RootId {
        root.unwrap_or_else(|| {
            let promise = self.promise_object().expect("a promise without lifecycle hooks runs no guest code");
            self.heap.root(promise)
        })
    }

    /// The promise of a settling lane call (`LaneStack::settling`) whose activation stops now,
    /// taken off its entry `root`: the rooted one, unrooted, or one made now for a call that
    /// deferred it to its awaiting caller, as `settling_root` makes it but never rooted.
    ///
    /// The root `settling_root` would push lasted only until `release_settled_call_root` popped
    /// it again, with nothing between: making the promise without lifecycle hooks, which
    /// settling calls exclude, runs no guest code and collects nothing, so the caller holds it
    /// as `release_settled_call_root` would have handed it back.
    pub(super) fn settling_promise(&mut self, root: Option<crate::heap::RootId>) -> Value {
        match root {
            Some(root) => self.release_settled_call_root(root),
            None => self.promise_object().expect("a promise without lifecycle hooks runs no guest code"),
        }
    }

    /// The promise an async call pushed in place settles (`PendingGeneralCall::completion`),
    /// unrooted now that the call completes.
    pub(super) fn release_settled_call_root(&mut self, root: crate::heap::RootId) -> Value {
        let promise = self.heap.root_value(root).expect("a settling call roots its promise");
        self.heap.release_root(root);
        promise
    }

    /// A plain `Call` of an async function that never suspends (`CallPlan::settles_in_loop`),
    /// pushed in place as `try_inline_user_call` pushes ordinary bodies: the call makes its
    /// promise first, as `call_user_maybe_async` does, and the pending call settles it
    /// with the body's completion. A failed push rejects it at once.
    #[inline(never)]
    pub(super) fn try_settling_call(
        &mut self,
        p: &ResidualProgram,
        f: usize,
        i: crate::bytecode::WideInstruction,
        pc: usize,
        callee: Value,
        target: (ProgramId, u32, Value),
    ) -> Option<Result<StepResult, JsError>> {
        if i.returns_from_frame() {
            return None;
        }
        let promise = match self.promise_object() {
            Ok(promise) => promise,
            Err(error) => return Some(Err(error)),
        };
        let root = self.heap.root(promise);
        let destination = i.result_register();
        let this = self.read(f, i.register_c());
        // The window is read after the promise, whose observers may run guest code.
        let arguments: *const [Value] = self.register_window(f, i.call_window());
        // SAFETY: see `push_general_user_frame`; the window outlives callee setup.
        let pushed = self.push_in_loop_call(p, f, pc, target, callee, this, unsafe { &*arguments }, destination);
        Some(match pushed {
            Some(Ok(StepResult::PushFrame { destination, call_guard, .. })) => {
                Ok(StepResult::PushFrame { destination, call_guard, completion: CallCompletion::Promise(root) })
            }
            Some(Err(error)) => {
                let promise = self.release_settled_call_root(root);
                self.reject_settled_call(p, promise, error).map(|()| {
                    self.write(f, destination, promise);
                    StepResult::Continue
                })
            }
            _ => {
                // No frame was pushed: the general call makes its own promise.
                self.release_settled_call_root(root);
                return None;
            }
        })
    }

    /// Suspends the activation `kind` holds on `source`: one `AwaitState` cell holds it and
    /// is both handlers of the await's reaction, so it lives exactly as long as that
    /// reaction. An async generator's await holds no frame, whose continuation stays in its
    /// generator's record (`async_generator_await`, `async_generator_yield`).
    pub(super) fn enqueue_async_resume(
        &mut self,
        p: &ResidualProgram,
        kind: AwaitKind,
        promise: Value,
        source: Value,
    ) -> Result<(), JsError> {
        let generator = match &kind {
            AwaitKind::AsyncFunction(_) => None,
            AwaitKind::AsyncGeneratorAwait { generator }
            | AwaitKind::AsyncGeneratorYield { generator } => Some(*generator),
        };
        let state = self.armed_await_state(kind, promise);
        self.attach_await(p, state, promise, generator, source)
    }

    /// `attach_await_reactions` of `state`, the await state armed for an await of `source`,
    /// with the two call roots only a host that observes promise lifecycles needs: its
    /// observers can run before a reaction holds the state.
    ///
    /// Without one, nothing between here and the installed reaction allocates on the heap,
    /// collects or runs guest code: `attach_await_reactions` materializes no promise and goes
    /// straight to `attach_promise_reaction`, which marks the source observed, captures the
    /// host's job context (a host call that sees no value), and pushes the reaction onto the
    /// source's record or enqueues its job; the error of a non-promise object source is a Rust
    /// string. So `state` and `source`, which the caller holds, stay live and unmoved without
    /// the push and pop of the roots.
    #[inline(always)]
    fn attach_await(
        &mut self,
        p: &ResidualProgram,
        state: Value,
        promise: Value,
        generator: Option<Value>,
        source: Value,
    ) -> Result<(), JsError> {
        if !self.host.observes_promise_lifecycle() {
            return self.attach_promise_reaction(p, source, state, state, Value::UNDEFINED);
        }
        self.with_call_roots([state, source], |vm| vm.attach_await_reactions(p, state, promise, generator, source))
    }

    /// An await state armed with `kind` in the current realm, settling `promise`: the spare
    /// state (`spare_await_state`) where one waits, armed anew as `await_again` arms a spent
    /// one, else a fresh one. The spare is spent and no reaction, job or continuation names
    /// it (`keep_spare_await_state`), so arming it is indistinguishable from allocating.
    fn armed_await_state(&mut self, kind: AwaitKind, promise: Value) -> Value {
        let realm = self.realm.globals;
        let spare = std::mem::replace(&mut self.spare_await_state, Value::UNDEFINED);
        if !spare.is_undefined()
            && let Some(state) = self.heap.await_state_mut(spare)
        {
            debug_assert!(matches!(state, AwaitState::Spent), "only a spent await state is kept spare");
            *state = AwaitState::arm(kind, realm, promise);
            return spare;
        }
        self.heap.alloc_await_state(AwaitState::arm(kind, realm, promise))
    }

    /// Keeps `state`, the await state of an async generator's `yield` whose fulfilled reaction
    /// is running and has spent it (`resume_awaited`), as the spare the next await arms
    /// (`armed_await_state`) instead of allocating, where no spare waits yet.
    ///
    /// Nothing names the state any more: `attach_await_reactions` made it both handlers of
    /// one reaction, the one whose job is running, which reads it no further
    /// (`run_promise_reaction` settles no capability for an await); the yield's frame stays
    /// parked in its generator's record, not in the state; and the job's call roots, which
    /// hold it until the job ends, only keep it alive. A host that observes promise lifecycles
    /// keeps its fresh states, which its resources may be tied to.
    pub(super) fn keep_spare_await_state(&mut self, state: Value) {
        if self.spare_await_state.is_undefined()
            && !self.host.observes_promise_lifecycle()
            && self.heap.await_state(state).is_some_and(|state| matches!(state, AwaitState::Spent))
        {
            self.spare_await_state = state;
        }
    }

    /// Lets the spare await state go (`spare_await_state`), so the collection before a
    /// snapshot frees it: a snapshot holds no await state.
    pub(crate) fn forget_spare_await_state(&mut self) {
        self.spare_await_state = Value::UNDEFINED;
    }

    /// `enqueue_async_resume` of an activation that awaits again from the reaction of its last
    /// await, `resume`: its spent state takes `kind`, as a fresh one would. That reaction was
    /// the state's only one and is running, so nothing else names the state, and an activation
    /// keeps one state for all its awaits. A request's run has no state to re-arm.
    #[inline(always)]
    fn await_again(
        &mut self,
        p: &ResidualProgram,
        resume: AwaitResume,
        kind: AwaitKind,
        source: Value,
    ) -> Result<(), JsError> {
        let Some(spent) = resume.state else {
            return self.enqueue_async_resume(p, kind, resume.promise, source);
        };
        let realm = self.realm.globals;
        let state = self.heap.await_state_mut(spent).expect("a running await reaction keeps its state");
        debug_assert!(matches!(state, AwaitState::Spent), "an await state re-arms only once spent");
        *state = AwaitState::arm(kind, realm, resume.promise);
        self.attach_await(p, spent, resume.promise, resume.role.generator(), source)
    }

    fn attach_await_reactions(
        &mut self,
        p: &ResidualProgram,
        state: Value,
        promise: Value,
        generator: Option<Value>,
        source: Value,
    ) -> Result<(), JsError> {
        let observes = self.host.observes_promise_lifecycle();
        let source = if observes
            && !self.is_promise(source)
            && !self.is_object_like(source)
        {
            self.with_call_roots([source, promise, generator.unwrap_or(Value::UNDEFINED)], |vm| {
                let awaited = vm.fresh_derived_promise(promise)?;
                vm.with_call_roots([awaited], |vm| vm.promise_resolve_value(p, awaited, source))?;
                Ok(awaited)
            })?
        } else {
            source
        };
        // Materialize the await reaction's Promise resource for host lifecycle
        // observation, even though the continuation consumes no result value.
        // The observer read above stands: without one, nothing above ran guest code.
        let next = if observes {
            self.with_call_roots([source], |vm| vm.fresh_derived_promise(source))?
        } else {
            Value::UNDEFINED
        };
        // Init observers can settle the source reentrantly: the reaction reads its state
        // only once the reaction's observable resource exists.
        self.attach_promise_reaction(p, source, state, state, next)
    }
}

mod job_chain;
mod job_resume;
mod requests;
mod settle;
pub(in crate::vm) use settle::StopOwner;
