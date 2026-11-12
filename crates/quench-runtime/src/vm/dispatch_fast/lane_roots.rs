//! What a lane run is rooted in: the VM frame a general loop entered it from, or, for a promise
//! job, the suspended frame of an async function or async generator that its owner keeps
//! (`Vm::run_lane_job`). A job's lane resumes the activation over its record's register file
//! with no VM frame beneath it; its `await` suspends it back into the record (`lane_await`), its
//! return ends the run, and any other exit makes it a frame the general loop finishes.
use super::*;
use super::generator_windows::{open_window_locals, window_environments};
use super::super::activation::{GeneratorFrame, JobRoot};
use super::super::dispatch_frame::{ExecutingProgram, LaneExit};

/// The frame index of no frame: reads that look a frame up find none, and any other is a state
/// a job's lane excludes while its activations run.
const NO_FRAME: usize = usize::MAX;

/// What a lane run starts from.
#[derive(Clone, Copy)]
pub(super) enum LaneRoot {
    /// The VM frame the run continues, the innermost when it starts.
    Frame(usize),
    /// The activation of a promise job, which the lane runs frameless over its owner's record.
    Job(JobRoot),
}

impl LaneRoot {
    /// The VM frame the run continues, if it has one: a job's has none until its activations
    /// become frames.
    pub(super) fn frame(self) -> Option<usize> {
        match self {
            Self::Frame(frame) => Some(frame),
            Self::Job(_) => None,
        }
    }

    /// The frame the first activation to materialize returns to, given the `frames` on the
    /// stack: the root's own, or, for a job, the index below its first frame, which wraps for an
    /// empty stack. Nothing reads it: that frame enters as the general loop's entry frame, which
    /// no pending call names (`Completion::ResumesJob`).
    pub(super) fn caller_of_first(self, frames: usize) -> usize {
        self.frame().unwrap_or(frames.wrapping_sub(1))
    }
}

impl<H: Host> LaneContext<H> {
    /// The VM frame the lane runs, for the code a frame's run reads; `NO_FRAME` where a job's
    /// activations run.
    #[inline(always)]
    pub(super) fn frame(&self) -> usize {
        self.root.frame().unwrap_or(NO_FRAME)
    }
}

/// Everything a lane run's context starts from (`Vm::lane_context`).
pub(super) struct LaneStart<'a> {
    pub(super) code: &'a ResidualProgram,
    pub(super) program: ProgramId,
    pub(super) root: LaneRoot,
    pub(super) registers: Registers,
    pub(super) base: Ip,
    pub(super) this: Value,
    pub(super) pending: *mut Vec<PendingGeneralCall>,
    pub(super) executing: *mut ExecutingProgram,
    pub(super) inline_calls: bool,
    pub(super) script_calls: bool,
    pub(super) script_constructs: bool,
    pub(super) floor: *const std::sync::atomic::AtomicUsize,
}

/// How a job's lane run ended (`Vm::run_lane_job`).
pub(in crate::vm) enum JobEnd {
    /// The activation returned the value: its owner is finished.
    Returned(Value),
    /// The activation awaited the value in the lane: its owner holds the frame, and the await's
    /// reactions are the job's to attach.
    Awaited(Value),
    /// The lane left the activation to the general path, or did not admit the run
    /// (`Vm::run_job`), which ran the VM frame the activation became: the bottom one stays on
    /// the frame stack, with this outcome.
    Frames(Result<FrameOutcome, JsError>),
}

impl<H: Host> Vm<H> {
    /// The context of a lane run that starts as `start` says; its other state starts empty.
    pub(super) fn lane_context(&mut self, start: LaneStart<'_>) -> LaneContext<H> {
        let LaneStart { code, program, root, registers, base, this, pending, executing, inline_calls, script_calls, script_constructs, floor } = start;
        LaneContext {
            binding: LaneBinding { object_base: self.heap.object_record_base().unwrap_or(std::ptr::null()), resume: resume_lane_binding::<H> },
            vm: self,
            code,
            program,
            root,
            registers,
            base,
            pending,
            executing,
            views: self.programs.lane_views(program).map_or(&[][..], |views| views) as *const _,
            constants: self.programs.constants(program) as *const _,
            inline_calls: inline_calls && code.kind == crate::bytecode::ProgramKind::Wasm,
            script_calls,
            script_constructs,
            this,
            backedges_left: self.lane_credits_backedges(code, program).then_some(TIERS.wasm.backedges_per_entry),
            floor,
            memory: MemoryView::NONE,
            resolved: (Value::UNDEFINED, MemoryView::NONE),
            resumed_acc: Slot::of(Value::UNDEFINED),
            callee_resume: std::ptr::null(),
            callee_acc: Value::UNDEFINED,
            callee_warming: false,
            leaf: NO_LEAF_CALL,
            env: Value::DELETED,
            capture: std::ptr::null(),
            outer: NO_OUTER_WALK,
            frame_storage: NO_FRAME_STORAGE,
            proofs: crate::heap::ArrayStorageProofs::new(),
        }
    }

    /// Runs `context`'s lane from the record `start` enters, until a handler exits, then makes
    /// the activations frames the general path runs, and restores the lane that was running
    /// (`entered`, the floor it raised, was made by `enter_lane_floor`). The record it exited at.
    pub(super) fn drive_lane(
        &mut self,
        context: &mut LaneContext<H>,
        entered: LaneFloor,
        start: impl FnOnce(*mut LaneContext<H>) -> Ip,
    ) -> Ip {
        self.lane_stack.refresh_activation_limit();
        self.lane_stack.enter_generation((self.heap.collection_epoch(), self.realm.globals));
        let cx: *mut LaneContext<H> = context;
        let outer = LANE_CONTEXT.with(|current| current.replace(cx.cast()));
        let _proofs = crate::heap::LaneProofScope::enter();
        let ip = lane_dispatch(cx, start(cx));
        #[cfg(feature = "profile-lane-exits")]
        self.record_lane_exit(cx, ip);
        self.materialize_lane_exit(cx);
        self.leave_lane(outer, entered);
        ip
    }

    /// The census detail of a lane exit at the record `ip` of the innermost activation.
    #[cfg(feature = "profile-lane-exits")]
    fn record_lane_exit(&self, cx: *mut LaneContext<H>, ip: Ip) {
        let Some(activation) = self.lane_stack.activations.last() else {
            return;
        };
        // SAFETY: the context's program outlives the run; `ip` is a record of
        // the innermost activation's view.
        let (code, pc) = unsafe { (&*(*cx).code, ip.offset_from((*cx).base) as usize) };
        let Some(body) = code.function_get(activation.function as usize) else {
            return;
        };
        let instruction = body.instructions().nth(pc);
        super::lane_exits::record_detail(format!(
            "exit in activation {:?} {} {:?} {} @{:?} pc={pc}",
            instruction,
            self.programs.source_name(unsafe { (*cx).program }).unwrap_or_default(),
            instruction
                .filter(|instruction| instruction.op().is_get_field())
                .and_then(|instruction| code.atoms.get(instruction.imm() as usize)),
            body.name.map(|atom| code.atoms[atom as usize].to_string()).unwrap_or_default(),
            body.source_span.map(|span| span.start),
        ));
    }

    /// Makes the lane that was running, whose context was `outer`, the running one again, and
    /// restores the floor this run raised (`entered`).
    fn leave_lane(&mut self, outer: *mut (), entered: LaneFloor) {
        LANE_CONTEXT.with(|current| current.set(outer));
        if !outer.is_null() {
            // SAFETY: every active context has this repr(C) prefix. Its callback owns the
            // actual Host type, which may differ from this nested run's Host.
            let binding = unsafe { &*outer.cast::<LaneBinding>() };
            if binding.object_base.is_null() {
                unsafe { (binding.resume)(outer) };
            }
        }
        self.leave_lane_floor(entered);
    }

    /// The activation at `depth` of a lane run with `root` becomes a frame whose call is in
    /// frame `caller`, which resumes at `pc` after it. A job's bottom activation has no caller
    /// frame.
    pub(super) fn resume_caller_frame(&mut self, root: LaneRoot, depth: usize, caller: usize, pc: usize) {
        if depth > 0 || root.frame().is_some() {
            self.frames[caller].pc = pc;
            self.frames[caller].pc_is_resume = true;
        }
    }

    /// The frame of a windowed activation of `script`, made a VM frame where the activations of
    /// a lane run with `root` materialize: its owner's, a generator's or a job's.
    pub(super) fn push_window_frame(
        &mut self,
        root: LaneRoot,
        script: &ScriptActivation,
        iterators: impl Iterator<Item = super::super::ActiveIterator>,
    ) {
        match root {
            LaneRoot::Job(job) if script.completion == Completion::ResumesJob => self.push_job_window_frame(job, iterators),
            _ => self.push_generator_window_frame(script.callee, iterators),
        }
    }

    /// Resumes the activation `job` names, an async function's or an async generator's that
    /// awaited in the lane or a frame, as the root of a lane run over its frame's register
    /// file, which stays in its owner (`Vm::resume_job_window`): `sent` is the value its
    /// `await` evaluates to. `code` is the program of its function, the active one, and the
    /// realm is the one that awaited. `None`, changing nothing, where the lane does not admit
    /// it (`admit_job`).
    pub(in crate::vm) fn run_lane_job(&mut self, code: &ResidualProgram, job: JobRoot, sent: Value) -> Option<JobEnd> {
        let window = self.admit_job(code, job, sent)?;
        let mut pending = self.pending_call_stacks.pop().unwrap_or_default();
        let mut executing = ExecutingProgram { id: window.entry.program, retained: None };
        let caller_registers = self.active_registers;
        let entered = self.enter_lane_floor(code, window.entry.program);
        let start = LaneStart {
            code,
            program: window.entry.program,
            root: LaneRoot::Job(job),
            registers: Registers(window.registers),
            base: window.base,
            this: window.entry.this,
            pending: &raw mut pending,
            executing: &raw mut executing,
            inline_calls: true,
            script_calls: true,
            script_constructs: code
                .function_get(window.entry.function as usize)
                .is_some_and(|function| function.dispatch == crate::bytecode::DispatchClass::General),
            floor: entered.cell,
        };
        let mut context = self.lane_context(start);
        let ip = self.drive_lane(&mut context, entered, |cx| window.enter(cx));
        let end = self.job_end(code, job, &context, ip, &mut pending, executing);
        pending.clear();
        self.pending_call_stacks.push(pending);
        self.active_registers = caller_registers;
        Some(end)
    }

    /// The window `job`'s activation runs over, once the lane admits the run, and the frame's
    /// owner has started it: no other lane's activations run, the function lays out as a window
    /// (`window_layout`) and has a view, and the stack has room. `None` changes nothing.
    fn admit_job(&mut self, code: &ResidualProgram, job: JobRoot, sent: Value) -> Option<JobWindow> {
        if !self.execution_mode.allows_dynamic_execution()
            || !self.lane_stack.activations.is_empty()
            || self.lane_deferred.is_some()
            || self.with_scope_entered
            || self.direct_eval
            || self.parameter_eval
            || code.kind == crate::bytecode::ProgramKind::Wasm
        {
            return None;
        }
        let frame = self.job_frame(job)?;
        let environment = self.window_layout(frame, code)?;
        let (program, function, env, captured, this, pc) = (frame.program, frame.function, frame.env, frame.captured, frame.this(), frame.pc);
        let view = self.lane_view_any(program, function)?;
        let (parent, locals) = window_environments(self, view, env, captured, environment)?;
        let stack = crate::stack::StackGuard::enter().ok()?;
        self.check_execution_interrupt().ok()?;
        let capture = self.target_capture(code, program, parent);
        let frame = self.resume_job_window(job, sent)?;
        if let Some(locals) = locals {
            open_window_locals(self, frame, env, &locals);
        }
        // SAFETY: the owner keeps the frame, and so its registers, until the run ends.
        let (prefix, registers, size) = unsafe { ((*frame).prefix(), (*frame).registers.as_mut_ptr(), (*frame).registers.len()) };
        // SAFETY: the program store keeps a derived view alive with its program.
        let base = unsafe { (*view).records() };
        let entry = PreparedEntry { view, env: parent, capture, this, registers: size, passed: 0, argc: 0, keeps: false, size, program, function };
        let activation = job_activation(&entry, base, registers, job, prefix, environment);
        Some(JobWindow { frame, registers, base, pc, entry, activation, _stack: stack })
    }

    /// How the lane run of `job`'s activation ended, which left the lane at the record `ip`: it
    /// returned or awaited in the lane, or its activations became frames, which the general
    /// loop finishes down to the bottom one.
    fn job_end(
        &mut self,
        code: &ResidualProgram,
        job: JobRoot,
        context: &LaneContext<H>,
        ip: Ip,
        pending: &mut Vec<PendingGeneralCall>,
        executing: ExecutingProgram,
    ) -> JobEnd {
        if let Some(value) = self.lane_native_returned.take() {
            self.finish_job_window(job);
            return JobEnd::Returned(value);
        }
        if let Some(value) = self.lane_awaited.take() {
            return JobEnd::Awaited(value);
        }
        // SAFETY: handlers only return records of the active view, which is the innermost
        // frame's, the lane's activations having become frames.
        let pc = unsafe { ip.offset_from(context.base) } as usize;
        let frame = context.frame();
        self.frames[frame].pc = pc;
        let raised = self.lane_raised.take();
        JobEnd::Frames(self.run_frames_after_lane_exit(code, frame, pending, LaneExit { executing, raised }))
    }
}

/// A job's activation over its frame's register file, ready to enter (`Vm::admit_job`): the
/// frame the owner keeps, the window and the records of its function's view, where the frame
/// resumes, what the lane is made for it and its first activation. The guest-stack guard lasts
/// the run.
struct JobWindow {
    frame: *mut GeneratorFrame,
    registers: *mut Value,
    base: Ip,
    pc: usize,
    entry: PreparedEntry,
    activation: Activation,
    _stack: crate::stack::StackGuard,
}

impl JobWindow {
    /// Enters the window in `cx`, the context made for it: the activation is the lane stack's
    /// first, the frame's loops resume as its own (`LaneStack::iterators`), and the lane
    /// continues at the record of `pc`, where the frame resumes in its function's view.
    fn enter<H: Host>(&self, cx: *mut LaneContext<H>) -> Ip {
        // SAFETY: the lane stack's limit admits its first activation, which resumes over the
        // frame's registers, nothing else writing them.
        unsafe {
            (*cx).memory = MemoryView::script(cx);
            let stack = &mut vm(cx).lane_stack;
            stack.push_generator_activation(self.activation);
            stack.restore_iterators(0, &mut (*self.frame).active_iterators);
            lane_enter_suspended(cx, &self.entry, self.registers, self.pc)
        }
    }
}

/// The activation of `job`'s frame, the bottom of its lane run, over the window at `window` of
/// the function `entry` names, whose records start at `base`. No call record or caller state is
/// its own: it names the first record as its call and the window as its caller's registers,
/// which its return, the end of the run, restores and nothing reads.
fn job_activation(
    entry: &PreparedEntry,
    base: Ip,
    window: *mut Value,
    job: JobRoot,
    prefix: crate::vm::compiled::ShadowFrame,
    environment: bool,
) -> Activation {
    Activation {
        native: prefix,
        function: entry.function,
        call: base,
        caller_base: base,
        caller_registers: Registers(window),
        caller_program: entry.program,
        kind: ActivationKind::Script(ScriptActivation {
            callee: job.object,
            argc: 0,
            caller_this: Value::UNDEFINED,
            caller_env: Value::DELETED,
            caller_capture: std::ptr::null(),
            completion: Completion::ResumesJob,
            forwards: false,
            environment,
        }),
    }
}
