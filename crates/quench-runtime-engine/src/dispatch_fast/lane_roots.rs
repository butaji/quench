//! What a lane run is rooted in: the VM frame a general loop entered it from, or, for a promise
//! job or a native `next`, the suspended frame of an async function or a generator that its
//! owner keeps (`Vm::run_lane_job`). A job's lane resumes the activation over its record's register file
//! with no VM frame beneath it; its `await` suspends it back into the record (`await_in_lane`), its
//! return ends the run, and any other exit makes it a frame the general loop finishes.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::generator_windows::{open_window_locals, cached_window_plan, store_window_plan};
use quench_runtime::vm::activation::{FrameRecord, JobResume, JobRoot};
use quench_runtime::vm::dispatch_frame::{ExecutingProgram, LaneHandoff};

/// The frame index of no frame: reads that look a frame up find none, and any other is a state
/// a job's lane excludes while its activations run.
const NO_FRAME: usize = usize::MAX;

pub(crate) use quench_runtime::vm::engine_types::LaneRoot;

#[quench_macros::ext(LaneRootOps1)]
impl LaneRoot {
    /// The VM frame the run continues, if it has one: a job's has none until its activations
    /// become frames.
    #[inline]
    pub(crate) fn frame(self) -> Option<usize> {
        match self {
            Self::Frame(frame) => Some(frame),
            Self::Job(_) => None,
        }
    }

    /// The frame the first activation to materialize returns to, given the `frames` on the
    /// stack: the root's own, or, for a job, the index below its first frame, which wraps for an
    /// empty stack. Nothing reads it: that frame enters as the general loop's entry frame, which
    /// no pending call names (`Completion::ResumesJob`).
    pub(crate) fn caller_of_first(self, frames: usize) -> usize {
        self.frame().unwrap_or(frames.wrapping_sub(1))
    }
}

#[quench_macros::ext(LaneContextOps1)]
impl LaneContext {
    /// The VM frame the lane runs, for the code a frame's run reads; `NO_FRAME` where a job's
    /// activations run.
    #[inline(always)]
    pub(crate) fn frame(&self) -> usize {
        self.root.frame().unwrap_or(NO_FRAME)
    }

    /// Whether the lane runs a VM frame, which a region's activation reads (`CompiledOsrRegion::needs_frame`).
    #[inline(always)]
    pub(crate) fn has_frame(&self) -> bool {
        self.root.frame().is_some()
    }
}

use quench_runtime::vm::engine_types::INNERMOST_STATE_LEN;

/// Everything a lane run's context starts from (`Vm::context_for_start`).
pub(crate) struct LaneStart<'a> {
    pub(crate) code: &'a ResidualProgram,
    pub(crate) program: ProgramId,
    pub(crate) root: LaneRoot,
    pub(crate) registers: Registers,
    pub(crate) base: Ip,
    pub(crate) this: Value,
    pub(crate) pending: *mut Vec<PendingGeneralCall>,
    pub(crate) executing: *mut ExecutingProgram,
    pub(crate) inline_calls: bool,
    pub(crate) script_calls: bool,
    pub(crate) script_constructs: bool,
    pub(crate) floor: *const std::sync::atomic::AtomicUsize,
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
            .is_some_and(|function| function.dispatch == quench_runtime::bytecode::DispatchClass::General),
        floor,
    }
}

pub(crate) use quench_runtime::vm::engine_types::JobEnd;

pub(crate) use quench_runtime::vm::engine_types::ParkedLane;



#[quench_macros::ext(DispatchFastLaneRootsVm)]
impl Vm {
    /// The receiver and environment of the innermost activation of the live lane, which its
    /// context holds: the context of the native call it runs beside (`lane_deferred`) or of the
    /// native whose callbacks it runs (`lane_native`). Both `undefined` where no native
    /// does, a direct eval's record then holding them (`lane_eval`).
    pub(crate) fn innermost_lane_state(&self) -> [Value; INNERMOST_STATE_LEN] {
        let cx = self.lane_deferred.map(|call| call.cx).or(self.lane_native.map(|call| call.cx));
        // SAFETY: a native call's lane runs, so its context lives.
        cx.map_or([Value::UNDEFINED; INNERMOST_STATE_LEN], |cx| unsafe {
            let cx = cx.cast::<LaneContext>();
            [(*cx).registers.this(), (*cx).registers.env()]
        })
    }

    /// Runs `run`, which starts lane runs, above the activations of the run that is live, which
    /// it must neither see nor disturb: that run's lane stack parks on the root stack
    /// (`Vm::lane_roots`) with the `innermost` receiver and environment of its context, its
    /// deferred native call and the native it called, while `run` starts over an empty one (`Vm::spare_lane_stacks`), so
    /// every run's materialization takes its own activations alone. A collection `run` makes
    /// traces the parked stacks as the live one (`LaneStack::extend_roots`). The parked run's
    /// call targets name the objects of the generation it left, which a collection retires
    /// (`enter_generation`). A job run (`run_job`) and the code of a direct eval (`lane_eval`)
    /// start nested runs.
    pub(crate) fn nested_lane_run<T>(&mut self, innermost: [Value; 2], run: impl FnOnce(&mut Self) -> T) -> T {
        // The stacks swap boxes, as a stack is large to move.
        let mut stack = self.spare_lane_stacks.pop().unwrap_or_default();
        // Its cached targets admit calls of the program that runs (`LaneStack::enter_program`).
        stack.enter_program(self.active_program);
        std::mem::swap(&mut self.lane_stack, &mut stack);
        self.lane_roots.push(ParkedLane { stack, innermost });
        let (deferred, native, window) = (self.lane_deferred.take(), self.lane_native.take(), self.running_window.take());
        let result = run(self);
        (self.lane_deferred, self.lane_native, self.running_window) = (deferred, native, window);
        let mut stack = self.lane_roots.pop().expect("the lane stack this run parked").stack;
        std::mem::swap(&mut self.lane_stack, &mut stack);
        if !stack.has_frames() {
            self.spare_lane_stacks.push(stack);
        }
        self.lane_stack.enter_generation((self.cache_epochs.gc, self.realm.globals));
        result
    }

    /// The context of a lane run that starts as `start` says; its other state starts empty.
    #[inline(always)]
    pub(crate) fn context_for_start(&mut self, start: LaneStart<'_>) -> LaneContext {
        let LaneStart { code, program, root, registers, base, this, pending, executing, inline_calls, script_calls, script_constructs, floor } = start;
        // The root's receiver is its window's context word (`Registers::this`); its environment
        // and captures are the window's to write where it has its own (a frame's run has none).
        registers.set_this(this);
        registers.set_env(Value::DELETED);
        registers.set_capture(std::ptr::null());
        let tiers_run = self.tiers_run_in_lanes();
        let (views, constants) = self.programs.lane_tables(program);
        let (views, constants) = (std::ptr::from_ref(views), std::ptr::from_ref(constants));
        LaneContext {
            binding: LaneBinding { object_base: self.heap_reads().object_record_base().unwrap_or(std::ptr::null()), resume: resume_lane_binding, retire: retire_lane_binding },
            vm: self,
            code,
            program,
            root,
            registers,
            base,
            pending,
            executing,
            views,
            constants,
            inline_calls: inline_calls && code.kind == quench_runtime::bytecode::ProgramKind::Wasm,
            script_calls,
            script_constructs,
            tiers_run,
            // `script_calls` starts on only where `inline_calls` is.
            returns_script_frame: false,
            instance: Value::DELETED,
            instance_slots: (Value::DELETED, std::ptr::null()),
            indirect: IndirectSlot::NONE,
            instance_address: (Value::DELETED.to_bits(), ProgramId::default(), 0),
            signatures: (ProgramId::default(), None),
            backedges_left: self.credits_backedges_in_lane(code, program).then_some(self.programs.tiers().wasm.backedges_per_entry),
            floor,
            // SAFETY: the VM holds its interrupt state for the whole run.
            fuel: if unsafe { &*floor }.load(std::sync::atomic::Ordering::Relaxed) == COLLECTION_DUE_FLOOR { 0 } else { LANE_FUEL_BATCH },
            memory: MemoryView::NONE,
            resolved: (Value::UNDEFINED, MemoryView::NONE),
            resumed_acc: Slot::of(Value::UNDEFINED),
            callee_resume: std::ptr::null(),
            callee_acc: Value::UNDEFINED,
            body_site: None,
            callee_warming: false,
            leaf: NO_LEAF_CALL,
            outer: NO_OUTER_WALK,
            root_walk: NO_OUTER_WALK,
            frame_storage: NO_FRAME_STORAGE,
            proofs: quench_runtime::tier_surface::ArrayStorageProofs::new(),
            region_payload: 0,
            region_calls: 0,
        }
    }

    /// Runs `context`'s lane from the record `start` enters, until a handler exits (a start that
    /// is itself an exit leaves at once, as a raise at the entry does), then makes the
    /// activations frames the general path runs, and restores the lane that was running
    /// (`entered`, the floor it raised, was made by `enter_lane_floor`). The record it exited at.
    pub(crate) fn drive_lane(
        &mut self,
        context: &mut LaneContext,
        entered: LaneFloor,
        start: impl FnOnce(*mut LaneContext) -> Ip,
    ) -> Ip {
        self.lane_stack.refresh_activation_limit();
        self.lane_stack.enter_generation((self.cache_epochs.gc, self.realm.globals));
        // The tier's diagnostics, which no proved call runs under (`Admission::DIAGNOSED`).
        self.lane_stack.set_diagnosed(crate::compiled::admission::diagnosing(self));
        let epoch = self.cache_epochs.gc;
        let cx: *mut LaneContext = context;
        // The lane's admission mirrors the run's `script_calls` (`LaneStack::set_script_calls`);
        // the outer run's resumes after this one.
        let outer_script_calls = self.lane_stack.script_calls_allowed();
        self.lane_stack.set_script_calls(context.script_calls);
        let outer = LANE_CONTEXT.with(|current| current.replace(cx.cast()));
        // SAFETY: `cx` outlives the run, which restores the outer fuel before it ends.
        let outer_fuel = LANE_FUEL.with(|current| current.replace(unsafe { &raw mut (*cx).fuel }));
        let _proofs = quench_runtime::tier_surface::LaneProofScope::enter();
        let (first, left) = exited(start(cx));
        let ip = if left || !cfg!(feature = "jit") { first } else { dispatch_in_lane(cx, first) };
        #[cfg(feature = "profile-lane-exits")]
        self.record_lane_exit(cx, ip);
        self.materialize_lane_exit(cx);
        LANE_FUEL.with(|current| current.set(outer_fuel));
        // A collection this run made ran under its own context, not the outer one, which
        // resumes now: it drops what it resolved before (`retire_lane_binding`).
        if !outer.is_null() && self.cache_epochs.gc != epoch {
            // SAFETY: every active context has the repr(C) prefix, whose callback owns its Host.
            unsafe { (outer.cast::<LaneBinding>().as_ref().unwrap_unchecked().retire)(outer) };
        }
        self.leave_lane(outer, entered);
        self.lane_stack.set_script_calls(outer_script_calls);
        ip
    }

    /// The census detail of a lane exit at the record `ip` of the innermost activation.
    #[cfg(feature = "profile-lane-exits")]
    fn record_lane_exit(&self, cx: *mut LaneContext, ip: Ip) {
        let Some(activation) = self.lane_stack.top_frame() else {
            return;
        };
        // SAFETY: the context's program outlives the run; `ip` is a record of
        // the innermost activation's view.
        let (code, pc) = unsafe { (&*(*cx).code, record_pc(cx, ip) as usize) };
        let Some(body) = code.function_get(activation.function() as usize) else {
            return;
        };
        let instruction = body.instructions().nth(pc);
        super::lane_exits::record_detail_args(format_args!(
            "exit in activation {:?} {} {:?} {} @{:?} pc={pc}",
            instruction,
            self.programs.source_name(unsafe { (*cx).program }).unwrap_or_default(),
            instruction
                .filter(|instruction| quench_runtime::staged::reads_named_field(instruction.op()))
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
    pub(crate) fn resume_caller_frame(&mut self, root: LaneRoot, depth: usize, caller: usize, pc: usize) {
        if depth > 0 || root.frame().is_some() {
            self.frames[caller].pc = pc;
            self.frames[caller].pc_is_resume = true;
        }
    }

    /// The frame of a windowed activation of `script`, made a VM frame where the activations of
    /// a lane run with `root` materialize: its owner's, a generator's or a job's.
    pub(crate) fn push_window_frame(
        &mut self,
        root: LaneRoot,
        script: &ScriptActivation,
        iterators: impl Iterator<Item = quench_runtime::vm::ActiveIterator>,
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
    pub(crate) fn run_lane_job(&mut self, code: &ResidualProgram, job: JobRoot, frame: *const FrameRecord, resume: JobResume) -> Result<JobEnd, JobResume> {
        let Some(window) = self.admit_job(code, job, frame, resume.sent()) else {
            return Err(resume);
        };
        let mut pending = self.pending_call_stacks.pop().unwrap_or_default();
        let end = self.drive_job_window(code, job, window, resume, &mut pending);
        self.pending_call_stacks.push(pending);
        Ok(end)
    }

    /// `run_lane_job` of a job a chain of await jobs resumes (`Vm::run_job_chain`), over the
    /// pending general calls `pending`, which the chain keeps for all its jobs instead of taking
    /// a stack from `pending_call_stacks` and giving it back for each one. `pending` is empty
    /// when the run starts and when it ends.
    pub(crate) fn run_chained_lane_job(
        &mut self,
        code: &ResidualProgram,
        (job, frame): (JobRoot, *const FrameRecord),
        resume: JobResume,
        pending: &mut Vec<PendingGeneralCall>,
    ) -> Result<JobEnd, JobResume> {
        let Some(window) = self.admit_job(code, job, frame, resume.sent()) else {
            return Err(resume);
        };
        Ok(self.drive_job_window(code, job, window, resume, pending))
    }

    /// The lane run of `job`'s activation over `window`, which `admit_job` admitted, entered
    /// with `resume`, its general calls pending on `pending`, which it leaves empty. How the
    /// run ended (`job_end`).
    fn drive_job_window(&mut self, code: &ResidualProgram, job: JobRoot, window: JobWindow, resume: JobResume, pending: &mut Vec<PendingGeneralCall>) -> JobEnd {
        let mut executing = ExecutingProgram { id: window.entry.program, retained: None };
        let caller_registers = self.active_registers;
        let entered = self.enter_lane_floor(code, window.entry.program);
        let start = job_start(code, job, &window, (std::ptr::from_mut(pending), &raw mut executing), entered.cell);
        let mut context = self.context_for_start(start);
        let ip = self.drive_lane(&mut context, entered, |cx| match resume {
            JobResume::Sends(_) => window.enter(cx),
            JobResume::Throws(error) => window.enter_raising(cx, error),
        });
        let end = self.job_end(code, job, &context, ip, pending, executing);
        pending.clear();
        self.active_registers = caller_registers;
        end
    }

    /// The window `job`'s activation runs over, once the lane admits the run, and the frame's
    /// owner has started it: the function lays out as a window (`window_layout`) and has a
    /// view, and the stack has room. `frame` is the suspended frame of `job`'s owner
    /// (`Vm::job_frame`), looked up by the caller. `None` changes nothing.
    fn admit_job(&mut self, code: &ResidualProgram, job: JobRoot, frame: *const FrameRecord, sent: Value) -> Option<JobWindow> {
        // `code` is the active program's (`run_lane_job`).
        if !self.admits_in_lane(self.active_program, code) || code.kind == quench_runtime::bytecode::ProgramKind::Wasm {
            return None;
        }
        // SAFETY: the owner holds the frame boxed, which nothing moves or drops before the resume
        // below: the caller looked it up with the job rooted and ran no collection since.
        let frame = unsafe { &*frame };
        let (program, function, this, pc) = (frame.program, frame.function, frame.this(), frame.pc);
        let cached = cached_window_plan(self, code, frame)?;
        let (view, parent, locals, capture, environment) = (cached.0.view, cached.0.parent, cached.0.locals(), cached.0.capture, cached.0.environment);
        let this = self.window_this(code.function(function as usize), this, parent)?;
        let stack = quench_runtime::stack::StackGuard::enter().ok()?;
        self.check_execution_interrupt().ok()?;
        let frame = self.resume_job_window(job, sent)?;
        store_window_plan(frame, cached);
        if let Some(locals) = locals {
            open_window_locals(self, frame, locals);
        }
        // SAFETY: the owner keeps the frame, and so its registers, until the run ends.
        let record = unsafe { &mut *frame };
        let (receiver, registers, size) = (record.this().to_bits(), record.registers.as_mut_ptr(), record.registers.len());
        // SAFETY: the program store keeps a derived view alive with its program.
        let base = unsafe { (*view).records() };
        let entry = PreparedEntry { view, env: parent, capture, this, registers: size, passed: 0, argc: 0, keeps: false, size, program, function };
        let activation = job_activation(&entry, base, registers, job, receiver, environment, frame);
        Some(JobWindow { frame, registers, base, pc, entry, activation, _stack: stack })
    }

    /// How the lane run of `job`'s activation ended, which left the lane at the record `ip`: it
    /// returned or awaited in the lane, or its activations became frames, which the general
    /// loop finishes down to the bottom one.
    fn job_end(
        &mut self,
        code: &ResidualProgram,
        job: JobRoot,
        context: &LaneContext,
        ip: Ip,
        pending: &mut Vec<PendingGeneralCall>,
        executing: ExecutingProgram,
    ) -> JobEnd {
        if let Some(value) = self.lane_completion.take_native_returned() {
            self.finish_job_window(job);
            return JobEnd::Returned(value);
        }
        if let Some(value) = self.lane_completion.take_suspended() {
            if self.yielded_async_generator(job) {
                return JobEnd::Yielded(value);
            }
            return JobEnd::Suspended(value);
        }
        // A raise the lane settled left the job's root in place: no activation became a frame.
        if matches!(context.root, LaneRoot::Job(_))
            && let Some(error) = self.lane_completion.take_raised()
        {
            return JobEnd::Threw(error);
        }
        // SAFETY: handlers only return records of the active view, which is the innermost
        // frame's, the lane's activations having become frames.
        let pc = unsafe { pc_in_view(context.base, ip) };
        let frame = context.frame();
        self.frames[frame].pc = pc;
        let raised = self.lane_completion.take_raised();
        JobEnd::Frames(self.run_frames_after_lane_exit(code, frame, pending, LaneHandoff { executing, raised }))
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
    _stack: quench_runtime::stack::StackGuard,
}

impl JobWindow {
    /// Enters the window in `cx`, the context made for it: the activation is the lane stack's
    /// first, the frame's loops and scope chain resume as its own (`LaneStack::iterators`,
    /// `LaneStack::with_objects`), and the lane
    /// continues at the record of `pc`, where the frame resumes in its function's view.
    fn enter(&self, cx: *mut LaneContext) -> Ip {
        // SAFETY: the lane stack's limit admits its first activation, which resumes over the
        // frame's registers, nothing else writing them.
        unsafe {
            (*cx).memory = MemoryView::script(cx);
            let stack = &mut vm(cx).lane_stack;
            stack.push_generator_activation(self.activation);
            stack.restore_iterators(0, &mut (*self.frame).active_iterators);
            stack.restore_with_objects(0, &mut *self.frame);
            vm(cx).record_window_owner(&*(*cx).code, 0);
            enter_suspended_in_lane(cx, &self.entry, self.registers, self.pc)
        }
    }

    /// Enters the window as `enter` does, to raise `error` at the instruction the frame
    /// suspended at, the one before its resume point: a handler of its function covering that
    /// record catches the error, else the activation ends in its owner (`raised_exit_in_lane`).
    /// The record the lane exits at.
    fn enter_raising(&self, cx: *mut LaneContext, error: JsError) -> Ip {
        self.enter(cx);
        let suspended = self.pc.saturating_sub(1);
        // SAFETY: the running lane's context.
        let vm = unsafe { vm(cx) };
        vm.lane_completion.raise(error);
        vm.lane_raise_site = crate::compiled::raise_site(self.entry.function, suspended as u32);
        // SAFETY: the suspended instruction's record lies in its function's view, which `base` starts.
        raised_exit_in_lane(cx, unsafe { self.base.add(suspended) })
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
    receiver: u64,
    environment: bool,
    record: *mut FrameRecord,
) -> Activation {
    let mut activation = Activation::of_parts(
        entry.function,
        base,
        base,
        Registers(window),
        entry.program,
        ActivationKind::Script(ScriptActivation {
            callee: job.object,
            argc: 0,
            completion: Completion::ResumesJob,
            forwards: false,
            kept: 0,
            environment,
            record,
        }),
    );
    activation.this = receiver;
    activation.window = window.addr();
    activation
}
