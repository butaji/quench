//! What a lane run is rooted in: the VM frame a general loop entered it from, or, for a promise
//! job or a native `next`, the suspended frame of an async function or a generator that its
//! owner keeps (`Vm::run_lane_job`). A job's lane resumes the activation over its record's register file
//! with no VM frame beneath it; its `await` suspends it back into the record (`lane_await`), its
//! return ends the run, and any other exit makes it a frame the general loop finishes.
use super::*;
use super::generator_windows::{open_window_locals, window_environments};
use super::super::activation::{FrameRecord, JobResume, JobRoot};
use super::super::dispatch_frame::{ExecutingProgram, LaneExit};

/// The frame index of no frame: reads that look a frame up find none, and any other is a state
/// a job's lane excludes while its activations run.
const NO_FRAME: usize = usize::MAX;

/// What a lane run starts from.
#[derive(Clone, Copy)]
pub(super) enum LaneRoot {
    /// The VM frame the run continues, the innermost when it starts.
    Frame(usize),
    /// The activation of a promise job or a native generator step, which the lane runs
    /// frameless over its owner's record.
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

/// The lane start of `job`'s run over `window`, whose root is the job: its function's dispatch class
/// decides whether the lane constructs script callees.
fn job_start<'a>(code: &'a ResidualProgram, job: JobRoot, window: &JobWindow, (pending, executing): (*mut Vec<PendingGeneralCall>, *mut ExecutingProgram), floor: *const std::sync::atomic::AtomicUsize) -> LaneStart<'a> {
    LaneStart {
        code,
        program: window.entry.program,
        root: LaneRoot::Job(job),
        registers: Registers(window.registers),
        base: window.base,
        this: window.entry.this,
        pending,
        executing,
        inline_calls: true,
        script_calls: true,
        script_constructs: code
            .function_get(window.entry.function as usize)
            .is_some_and(|function| function.dispatch == crate::bytecode::DispatchClass::General),
        floor,
    }
}

/// How a job's lane run ended (`Vm::run_lane_job`).
pub(in crate::vm) enum JobEnd {
    /// The activation returned the value: its owner is finished.
    Returned(Value),
    /// The activation awaited the value in the lane, its owner holding the frame and the
    /// await's reactions the job's to attach, or a sync generator's root yielded it, its record
    /// suspended.
    Suspended(Value),
    /// The activation raised an exception no handler of it covers, which ended it in its owner
    /// (`raised_windows`): the promise or request the job settles rejects with the error.
    Threw(JsError),
    /// The lane left the activation to the general path, or did not lay its function out as a
    /// window (`Vm::run_job`), which ran the VM frame the activation became: the bottom one
    /// stays on the frame stack, with this outcome.
    Frames(Result<FrameOutcome, JsError>),
}

/// A lane stack a nested run parked (`Vm::nested_lane_run`) with the receiver and environment of
/// its innermost activation, which the lane's context held.
pub(in crate::vm) struct ParkedLane {
    pub(in crate::vm) stack: LaneStack,
    innermost: [Value; 2],
}

impl ParkedLane {
    /// Appends to `out` every value the parked run holds.
    pub(in crate::vm) fn extend_roots(&self, programs: &crate::vm::program_store::ProgramStore, out: &mut Vec<Value>) {
        self.stack.extend_roots(programs, out);
        out.extend(self.innermost);
    }
}

impl<H: Host> Vm<H> {
    /// The receiver and environment of the innermost activation of the live lane, which its
    /// context holds: the context of the native call it runs beside (`lane_deferred`) or of the
    /// native whose callbacks it runs (`lane_native`). Both `undefined` where no native
    /// does, a direct eval's record then holding them (`lane_eval`).
    pub(in crate::vm) fn innermost_lane_state(&self) -> [Value; 2] {
        let cx = self.lane_deferred.map(|call| call.cx).or(self.lane_native.map(|call| call.cx));
        // SAFETY: a native call's lane runs, so its context lives.
        cx.map_or([Value::UNDEFINED; 2], |cx| unsafe {
            let cx = cx.cast::<LaneContext<H>>();
            [(*cx).this, (*cx).env]
        })
    }

    /// Runs `run`, which starts lane runs, above the activations of the run that is live, which
    /// it must neither see nor disturb: that run's lane stack parks on the root stack
    /// (`Vm::lane_roots`) with the `innermost` receiver and environment of its context, its
    /// deferred native call and the native it called, while `run` starts over a fresh one, so
    /// every run's materialization takes its own activations alone. A collection `run` makes
    /// traces the parked stacks as the live one (`LaneStack::extend_roots`). The parked run's
    /// call targets name the objects of the generation it left, which a collection retires
    /// (`enter_generation`). A job run (`run_job`) and the code of a direct eval (`lane_eval`)
    /// start nested runs.
    pub(in crate::vm) fn nested_lane_run<T>(&mut self, innermost: [Value; 2], run: impl FnOnce(&mut Self) -> T) -> T {
        let stack = std::mem::take(&mut self.lane_stack);
        self.lane_roots.push(ParkedLane { stack, innermost });
        let (deferred, native) = (self.lane_deferred.take(), self.lane_native.take());
        let result = run(self);
        (self.lane_deferred, self.lane_native) = (deferred, native);
        self.lane_stack = self.lane_roots.pop().expect("the lane stack this run parked").stack;
        self.lane_stack.enter_generation((self.heap.collection_epoch(), self.realm.globals));
        result
    }

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
            region_payload: 0,
        }
    }

    /// Runs `context`'s lane from the record `start` enters, until a handler exits (a start that
    /// is itself an exit leaves at once, as a raise at the entry does), then makes the
    /// activations frames the general path runs, and restores the lane that was running
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
        let (first, left) = exited(start(cx));
        let ip = if left { first } else { lane_dispatch(cx, first) };
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
        super::lane_exits::record_detail_args(format_args!(
            "exit in activation {:?} {} {:?} {} @{:?} pc={pc}",
            instruction,
            self.programs.source_name(unsafe { (*cx).program }).unwrap_or_default(),
            instruction
                .filter(|instruction| instruction.op().is_get_field())
                .and_then(|instruction| code.atoms.get(instruction.imm() as usize)),
            body.name.map_or("", |atom| &code.atoms[atom as usize]),
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
        with_objects: impl Iterator<Item = Value>,
    ) {
        match root {
            LaneRoot::Job(job) if script.completion == Completion::ResumesJob => {
                self.push_job_window_frame(job, iterators, with_objects)
            }
            _ => self.push_generator_window_frame(script.callee, iterators, with_objects),
        }
    }

    /// Resumes the activation `job` names, an async function's or an async generator's that
    /// awaited in the lane or a frame, or a generator's, as the root of a lane run over its
    /// frame's register file, which stays in its owner (`Vm::resume_job_window`): `resume` is
    /// the value its `await` or `yield` evaluates to, or the error it raises there, which a
    /// handler of its function catches or which ends it in its owner (`JobEnd::Threw`). `code`
    /// is the program of its function, the active one, and the realm is the one that resumed
    /// it, starting over an empty lane stack (`Vm::nested_lane_run`). `resume` back, changing nothing, where the lane does not admit it (`admit_job`).
    pub(in crate::vm) fn run_lane_job(&mut self, code: &ResidualProgram, job: JobRoot, resume: JobResume) -> Result<JobEnd, JobResume> {
        let Some(window) = self.admit_job(code, job, resume.sent()) else {
            return Err(resume);
        };
        let mut pending = self.pending_call_stacks.pop().unwrap_or_default();
        let mut executing = ExecutingProgram { id: window.entry.program, retained: None };
        let caller_registers = self.active_registers;
        let entered = self.enter_lane_floor(code, window.entry.program);
        let start = job_start(code, job, &window, (&raw mut pending, &raw mut executing), entered.cell);
        let mut context = self.lane_context(start);
        let ip = self.drive_lane(&mut context, entered, |cx| match resume {
            JobResume::Sends(_) => window.enter(cx),
            JobResume::Throws(error) => window.enter_raising(cx, error),
        });
        let end = self.job_end(code, job, &context, ip, &mut pending, executing);
        pending.clear();
        self.pending_call_stacks.push(pending);
        self.active_registers = caller_registers;
        Ok(end)
    }

    /// The window `job`'s activation runs over, once the lane admits the run, and the frame's
    /// owner has started it: the function lays out as a window (`window_layout`) and has a
    /// view, and the stack has room. `None` changes nothing.
    fn admit_job(&mut self, code: &ResidualProgram, job: JobRoot, sent: Value) -> Option<JobWindow> {
        if !self.execution_mode.allows_dynamic_execution() || code.kind == crate::bytecode::ProgramKind::Wasm
        {
            return None;
        }
        let frame = self.job_frame(job)?;
        let environment = self.window_layout(frame, code)?;
        let (program, function, env, captured, this, pc) = (frame.program, frame.function, frame.env, frame.captured, frame.this(), frame.pc);
        let view = self.lane_view_any(program, function)?;
        let (parent, locals) = window_environments(self, view, env, captured, environment)?;
        let this = self.window_this(code.function(function as usize), this, parent)?;
        let stack = crate::stack::StackGuard::enter().ok()?;
        self.check_execution_interrupt().ok()?;
        let capture = self.target_capture(code, program, parent);
        let frame = self.resume_job_window(job, sent)?;
        if let Some(locals) = locals {
            open_window_locals(self, frame, locals);
        }
        // SAFETY: the owner keeps the frame, and so its registers, until the run ends.
        let (prefix, registers, size) = unsafe { ((*frame).prefix(), (*frame).registers.as_mut_ptr(), (*frame).registers.len()) };
        // SAFETY: the program store keeps a derived view alive with its program.
        let base = unsafe { (*view).records() };
        let entry = PreparedEntry { view, env: parent, capture, this, registers: size, passed: 0, argc: 0, keeps: false, size, program, function };
        let activation = job_activation(&entry, base, registers, job, prefix, environment, frame);
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
        if let Some(value) = self.lane_suspended.take() {
            return JobEnd::Suspended(value);
        }
        // A raise the lane settled left the job's root in place: no activation became a frame.
        if matches!(context.root, LaneRoot::Job(_))
            && let Some(error) = self.lane_raised.take()
        {
            return JobEnd::Threw(error);
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
    frame: *mut FrameRecord,
    registers: *mut Value,
    base: Ip,
    pc: usize,
    entry: PreparedEntry,
    activation: Activation,
    _stack: crate::stack::StackGuard,
}

impl JobWindow {
    /// Enters the window in `cx`, the context made for it: the activation is the lane stack's
    /// first, the frame's loops and scope chain resume as its own (`LaneStack::iterators`,
    /// `LaneStack::with_objects`), and the lane
    /// continues at the record of `pc`, where the frame resumes in its function's view.
    fn enter<H: Host>(&self, cx: *mut LaneContext<H>) -> Ip {
        // SAFETY: the lane stack's limit admits its first activation, which resumes over the
        // frame's registers, nothing else writing them.
        unsafe {
            (*cx).memory = MemoryView::script(cx);
            let stack = &mut vm(cx).lane_stack;
            stack.push_generator_activation(self.activation);
            stack.restore_iterators(0, &mut (*self.frame).active_iterators);
            stack.restore_with_objects(0, &mut (*self.frame).with_objects);
            lane_enter_suspended(cx, &self.entry, self.registers, self.pc)
        }
    }

    /// Enters the window as `enter` does, to raise `error` at the instruction the frame
    /// suspended at, the one before its resume point: a handler of its function covering that
    /// record catches the error, else the activation ends in its owner (`lane_raised_exit`).
    /// The record the lane exits at.
    fn enter_raising<H: Host>(&self, cx: *mut LaneContext<H>, error: JsError) -> Ip {
        self.enter(cx);
        let suspended = self.pc.saturating_sub(1);
        // SAFETY: the running lane's context.
        let vm = unsafe { vm(cx) };
        vm.lane_raised = Some(error);
        vm.lane_raise_site = super::super::compiled::raise_site(self.entry.function, suspended as u32);
        // SAFETY: the suspended instruction's record lies in its function's view, which `base` starts.
        lane_raised_exit(cx, unsafe { self.base.add(suspended) })
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
    record: *mut FrameRecord,
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
            record,
        }),
    }
}
