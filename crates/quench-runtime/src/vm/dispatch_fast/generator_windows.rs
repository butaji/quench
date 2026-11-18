//! A generator the lane resumes as an activation over its suspended frame's register file
//! (`FrameRecord`, `Vm::resume_generator_window`): the record keeps the frame and its
//! registers, which the activation's window is, so a resume allocates no `Box<Frame>` and
//! copies no register. A `yield` suspends the frame in place and pops the activation, and a
//! return completes the generator.
use super::*;
use super::super::activation::{FrameRecord, GeneratorRecord};

impl Activation {
    /// What its caller's frame resumes with when the activation becomes a frame, as the register
    /// that takes its result and the pc: after the call record into the record's result register.
    /// A delegate generator's step (`Completion::Delegates`) is the `yield*` itself, which resumes
    /// at that record to read the step's result from its state register.
    pub(super) fn caller_resume(&self, call_pc: usize) -> (crate::bytecode::Register, usize) {
        // SAFETY: the call record belongs to the caller's live view.
        let call = unsafe { &*self.call };
        match self.script() {
            Some(script) if script.completion == Completion::Delegates => {
                (ImmediateLayout::register_pair(call.imm()).0, call_pc)
            }
            _ => (call.a, call_pc + 1),
        }
    }
}

impl Activation {
    /// The dynamic bindings its record holds (implicit ones, a direct eval's), which the
    /// environment its window makes for its first closure takes over, as a frame's promotion
    /// takes its own (`Vm::promote_frame_environment`); none once taken, or for an activation
    /// without a record.
    pub(super) fn take_record_bindings(&self) -> Vec<(crate::bytecode::Atom, Value)> {
        let record = self.script().map_or(std::ptr::null_mut(), |script| script.record);
        // SAFETY: a record outlives the activation that runs over it, and nothing else accesses it.
        unsafe { record.as_mut() }.map(|record| std::mem::take(&mut record.dynamic_bindings)).unwrap_or_default()
    }
}

impl LaneStack {
    /// Pushes `activation` over the register file of a suspended frame, which its prefix names
    /// (`FrameRecord::prefix`): the window is marked as no lane stack's (`HEAP_WINDOW`), so
    /// its return frees nothing there.
    ///
    /// # Safety
    /// The activations' room holds one more.
    pub(super) unsafe fn push_generator_activation(&mut self, mut activation: Activation) {
        activation.native.window |= HEAP_WINDOW;
        // SAFETY: as the caller guarantees; a script activation holds no Wasm storage.
        unsafe { self.activations.push_unchecked(activation) };
    }

    /// The iterators a resumed generator's loops left open, now the loops of the activation at
    /// `depth` (`LaneStack::iterators`).
    pub(super) fn restore_iterators(&mut self, depth: usize, open: &mut Vec<super::super::ActiveIterator>) {
        if !open.is_empty() {
            self.iterators.extend(open.drain(..).map(|iterator| (depth, iterator)));
        }
    }

    /// The iterators the activation at `depth`, the innermost, pushed (its entries, the last),
    /// moved to the end of `open`, the suspending frame's (`FrameRecord::active_iterators`).
    pub(in crate::vm) fn take_iterators(&mut self, depth: usize, open: &mut Vec<super::super::ActiveIterator>) {
        if self.iterators.last().is_some_and(|(owner, _)| *owner >= depth) {
            let first = self.iterators.partition_point(|(owner, _)| *owner < depth);
            open.extend(self.iterators.drain(first..).map(|(_, iterator)| iterator));
        }
    }
}

/// Whether the running activation is a generator over its record's register file.
#[inline(always)]
pub(super) fn running_generator<H: Host>(vm: &Vm<H>) -> bool {
    vm.lane_stack.activations.last().and_then(Activation::script).is_some_and(|script| script.completion.is_generator())
}

/// `lane_generator_step` of a for-of step of a generator's `next` that the lane resumes as an
/// activation (`lane_resume_generator`); `None` leaves any other step, and a step that returns
/// its result from the frame, to the general ones.
#[inline(never)]
pub(super) fn lane_resume_generator_step<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    method: Value,
    generator: Value,
    completion: Completion,
) -> Option<(Registers, Ip)> {
    if i.a & crate::bytecode::RETURN_REGISTER != 0 {
        return None;
    }
    lane_resume_generator(cx, r, ip, generator, method, Value::UNDEFINED, completion)
}

/// How a resumed generator's window carries its locals (`FrameRecord::open_environment_window`):
/// its function's captured-slot mask and, for a frame that made its environment, the slot storage
/// of that environment.
#[derive(Clone, Copy)]
pub(super) struct WindowLocals {
    captured: u64,
    own_slots: Option<*const crate::heap::EnvironmentSlots>,
}

/// What a suspended frame's windowed resumes derive from its function and environment alone,
/// kept on its record (`FrameRecord::window_plan`) while `key` holds: the same collection
/// epoch (the capture's remembered environment), program-store epoch (its view) and
/// environment, made or not. Each resume of a generator or async body re-derived them.
#[derive(Clone, Copy)]
pub(in crate::vm) struct WindowPlan {
    key: (u64, u64, Value, bool),
    pub(super) view: *const LaneView,
    pub(super) parent: Value,
    pub(super) locals: Option<WindowLocals>,
    pub(super) capture: *const usize,
    /// Whether the window carries an environment after the registers (`Vm::window_layout`).
    pub(super) environment: bool,
}

/// The plan a windowed resume of `frame`, laid out with an environment window when
/// `environment`, runs under: its record's own while it still holds, else derived anew (the
/// caller stores it back on the record it resumes). `None`, before any effect, where the
/// function has no view or environment layout.
pub(super) fn window_plan<H: Host>(vm: &mut Vm<H>, code: &ResidualProgram, frame: &FrameRecord, environment: bool) -> Option<WindowPlan> {
    let (program, function, env, captured) = (frame.program, frame.function, frame.env, frame.captured);
    let key = (vm.heap.collection_epoch(), vm.programs.context_epoch(), env, captured);
    if let Some(plan) = frame.window_plan.filter(|plan| plan.key == key) {
        return Some(plan);
    }
    let view = vm.lane_view_any(program, function)?;
    let (parent, locals) = window_environments(vm, view, env, captured, environment)?;
    let capture = vm.target_capture(code, program, parent);
    Some(WindowPlan { key, view, parent, locals, capture, environment })
}

/// `window_plan` of a suspended `frame`, of the active program: its record's own while it
/// holds, else the last one derived for its function under the same environment
/// (`LaneStack::last_window_plan`), else laid out as its function's plan says
/// (`Vm::window_layout`).
#[inline(always)]
pub(super) fn cached_window_plan<H: Host>(vm: &mut Vm<H>, code: &ResidualProgram, frame: &FrameRecord) -> Option<WindowPlan> {
    let key = (vm.heap.collection_epoch(), vm.programs.context_epoch(), frame.env, frame.captured);
    if let Some(plan) = frame.window_plan.filter(|plan| plan.key == key) {
        return Some(plan);
    }
    let shared = (vm.active_program, frame.function);
    if frame.program == vm.active_program
        && let Some((.., plan)) = vm.lane_stack.last_window_plan.filter(|(program, function, plan)| (*program, *function) == shared && plan.key == key)
    {
        return Some(plan);
    }
    let environment = vm.window_layout(frame, code)?;
    let plan = window_plan(vm, code, frame, environment)?;
    if !frame.captured {
        vm.lane_stack.last_window_plan = Some((shared.0, shared.1, plan));
    }
    Some(plan)
}

/// The environment a generator's closures and captures run under, and its window's locals when
/// `environment` (`Vm::window_layout`) says the window carries them: the frame's
/// `env` is its own environment once it made one (`captured`), whose parent the activation
/// runs under, else the closure's. `None`, before any effect, where `view` has no environment
/// window or the environment is no cell.
pub(super) fn window_environments<H: Host>(
    vm: &Vm<H>,
    view: *const LaneView,
    env: Value,
    captured: bool,
    environment: bool,
) -> Option<(Value, Option<WindowLocals>)> {
    let own = if captured {
        let Some(crate::heap::EnvironmentRecord { parent, slots, .. }) = vm.heap.environment(env) else {
            return None;
        };
        Some((*parent, std::ptr::from_ref(slots)))
    } else {
        None
    };
    let parent = own.map_or(env, |(parent, _)| parent);
    if !environment {
        return Some((parent, None));
    }
    // SAFETY: the program store keeps a derived view alive with its program.
    let layout = unsafe { &*view }.environment_window?;
    Some((parent, Some(WindowLocals { captured: layout.captured, own_slots: own.map(|(_, slots)| slots) })))
}

/// The activation of `generator`, a suspended generator the lane admits
/// (`Vm::window_layout`) that `method` steps (`steps_as_generator`), made over its
/// record's register file for the call record at `ip` of the running function, whose registers
/// are `r`: `sent` is the value its `yield` evaluates to, `completion` how its caller takes the
/// step. The window's registers, and for a generator with locals the environment window after
/// them, are the record's own, bound as a call binds a fresh one; the frame's receiver and
/// captures run with it. The registers and record the lane continues at, or `None`, before any
/// effect, where the lane has no room or no view for it.
fn lane_resume_generator<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    generator: Value,
    method: Value,
    sent: Value,
    completion: Completion,
) -> Option<(Registers, Ip)> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let (record, frame) = resumable_generator_frame(vm, generator)?;
    let (program, function, this, pc, size) = (frame.program, frame.function, frame.this(), frame.pc, frame.registers.len());
    let depth = Some(vm.lane_stack.activations.len()).filter(|&depth| depth < vm.lane_stack.activation_limit.get())?;
    let plan = cached_window_plan(vm, code, frame)?;
    let WindowPlan { view, parent, locals, capture, environment, .. } = plan;
    let frame = vm.resume_generator_window((generator, record), method, sent, depth)?;
    unsafe { (*frame).window_plan = Some(plan) };
    if let Some(locals) = locals {
        open_window_locals(vm, frame, locals);
    }
    // SAFETY: the record keeps the frame, and so its registers, until the activation ends.
    let (prefix, window) = unsafe { ((*frame).prefix(), (*frame).registers.as_mut_ptr()) };
    push_generator_window(cx, vm, (r, ip, depth), GeneratorWindow { prefix, function, generator, completion, environment, record: frame });
    let entry = PreparedEntry { view, env: parent, capture, this, registers: size, passed: 0, argc: 0, keeps: false, size, program, function };
    // SAFETY: the window holds the suspended registers, which nothing else writes while it runs,
    // and `pc` is where the frame resumes in its function's view.
    let start = unsafe { lane_enter_suspended(cx, &entry, window, pc) };
    Some((Registers(window), start))
}

/// `generator`'s record and its suspended frame, when the frame resumes in this realm and program.
/// SAFETY of the result: the generator's cell keeps its record, and the record its frame, boxed:
/// nothing moves or frees either before the resume.
fn resumable_generator_frame<'a, H: Host>(vm: &mut Vm<H>, generator: Value) -> Option<(*mut GeneratorRecord, &'a FrameRecord)> {
    let record = std::ptr::from_mut(vm.generator_record_mut(generator)?);
    let frame = unsafe { &*std::ptr::from_ref(&*(*record).suspended_continuation()?.frame) };
    let resumable = unsafe { (*record).realm } == vm.realm.globals && frame.program == vm.active_program;
    resumable.then_some((record, frame))
}

/// Lays `frame`'s environment window after its registers (`FrameRecord::open_environment_window`).
/// A frame holding implicit bindings keeps them in its record until a closure makes the window's
/// environment, which takes them over (`Activation::take_record_bindings`). An environment the
/// frame made already is written through the window's stores without the barrier a heap write
/// takes, so it joins the remembered set once, as a capture store's target does
/// (`Vm::target_capture`).
pub(super) fn open_window_locals<H: Host>(vm: &mut Vm<H>, frame: *mut FrameRecord, locals: WindowLocals) {
    // SAFETY: the record owns the box, which nothing moves or drops until the run ends.
    let frame = unsafe { &mut *frame };
    if locals.own_slots.is_some() {
        vm.heap.remember_cell(frame.env);
    }
    frame.open_environment_window(locals.captured, locals.own_slots);
}

/// What the activation of a resumed generator runs (`generator_activation`): its frame's
/// prefix and function, the generator, how its caller takes the step, whether its window
/// carries an environment (`ScriptActivation::environment`) and the record it runs over.
struct GeneratorWindow {
    prefix: crate::vm::compiled::ShadowFrame,
    function: u32,
    generator: Value,
    completion: Completion,
    environment: bool,
    record: *mut FrameRecord,
}

/// Pushes `window`'s activation as the lane's innermost, at `depth` below the call record at `ip` of
/// the running function with registers `r`, and resumes the frame's loops and scope chain as its own.
fn push_generator_window<H: Host>(cx: *mut LaneContext<H>, vm: &mut Vm<H>, (r, ip, depth): (Registers, Ip, usize), window: GeneratorWindow) {
    let frame = window.record;
    let activation = generator_activation(cx, r, ip, window);
    // SAFETY: the caller's limit check leaves room for the activation, and the frame's loops and
    // scope chain resume as the new activation's (`LaneStack::iterators`, `LaneStack::with_objects`).
    unsafe {
        vm.lane_stack.push_generator_activation(activation);
        vm.lane_stack.restore_iterators(depth, &mut (*frame).active_iterators);
        vm.lane_stack.restore_with_objects(depth, &mut (*frame).with_objects);
        vm.record_window_owner(&*(*cx).code, depth);
    }
}

/// The activation of `window`'s generator for the call record at `ip`, made by the running
/// function with registers `r`.
fn generator_activation<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, window: GeneratorWindow) -> Activation {
    Activation {
        native: window.prefix,
        function: window.function,
        call: ip,
        // SAFETY: the running lane's context, which nothing else accesses.
        caller_base: unsafe { (*cx).base },
        caller_registers: r,
        caller_program: unsafe { (*cx).program },
        kind: ActivationKind::Script(ScriptActivation {
            callee: window.generator,
            argc: 0,
            completion: window.completion,
            forwards: false,
            kept: 0,
            environment: window.environment,
            record: window.record,
        }),
    }
}

/// `yield` (or the pass-on of a `yield*`) of the running generator activation: its frame, resuming
/// at `pc`, suspends into its record, where the value the generator receives next goes to
/// `destination`, and the activation pops. The caller resumes with the step's result: the
/// iterator result of a yield of `value`, or `passed_on`, a delegate's own result object.
#[inline(never)]
fn lane_suspend_generator<H: Host>(
    cx: *mut LaneContext<H>,
    pc: usize,
    destination: crate::bytecode::Register,
    value: Value,
    passed_on: Option<Value>,
) -> Resumed {
    let vm = unsafe { vm(cx) };
    let depth = vm.lane_stack.activations.len() - 1;
    let activation = *vm.lane_stack.activations.last().expect("a running generator's activation");
    let ActivationKind::Script(script) = activation.kind else {
        unreachable!("a generator runs as a script activation");
    };
    vm.suspend_generator_window(script.callee, pc, destination, value, depth);
    vm.lane_stack.pop_activation();
    let (memory, _) = lane_restore_caller(cx, &activation);
    lane_resume_generated(vm, &activation, memory, (value, false), passed_on)
}

/// The return of the generator activation, popped with its caller restored (`memory`), with
/// `value`: the generator completes, and its caller resumes with the step's final result.
pub(super) fn lane_generator_returned<H: Host>(
    cx: *mut LaneContext<H>,
    activation: &Activation,
    memory: MemoryView,
    value: Value,
) -> Resumed {
    let vm = unsafe { vm(cx) };
    let ActivationKind::Script(script) = activation.kind else {
        unreachable!("a generator runs as a script activation");
    };
    vm.finish_generator_window(script.callee);
    lane_resume_generated(vm, activation, memory, (value, true), None)
}

/// The caller of a generator `activation` it left (`lane_suspend_generator`,
/// `lane_generator_returned`) resumes with the step's `value` and `done`, or the delegate's own
/// result `passed_on`: a for-of step's reads take them in their registers and the caller goes
/// on past them (`Completion::StepReads`), as `lane_iterator_step_reads` makes an array's step;
/// any other caller takes the step's result object after its call, in the call's register, or,
/// for a delegate's step, at the `yield*` itself in its state register.
fn lane_resume_generated<H: Host>(vm: &mut Vm<H>, activation: &Activation, memory: MemoryView, (value, done): (Value, bool), passed_on: Option<Value>) -> Resumed {
    let completion = activation.script().map(|script| script.completion);
    if let (Some(Completion::StepReads(form)), None) = (completion, passed_on) {
        // SAFETY: the step's records follow its call in the caller's live view (`lane_for_of_step`).
        let (done_read, value_read) = unsafe { (&*activation.call.add(FOR_OF_DONE_READ), &*activation.call.add(form.value_read())) };
        activation.caller_registers.set(done_read.a, if done { Value::TRUE } else { Value::FALSE });
        if done {
            return lane_resume_at(activation, memory, unsafe { activation.call.add(form.exit()) });
        }
        activation.caller_registers.set(value_read.a, value);
        return lane_resume_at(activation, memory, unsafe { activation.call.add(form.records()) });
    }
    let result = passed_on.unwrap_or_else(|| vm.generator_step_result(value, done, completion.is_some_and(Completion::reuses_result)));
    // The result may be a fresh allocation, which the caller's safepoints collect after.
    note_allocation(vm);
    if completion == Some(Completion::Delegates) {
        // SAFETY: the `yield*` record belongs to the caller's live view.
        let (state, _) = ImmediateLayout::register_pair(unsafe { (*activation.call).imm() });
        activation.caller_registers.set(state, result);
        return lane_resume_at(activation, memory, activation.call);
    }
    lane_resume_after_call(activation, memory, result)
}

/// `yield` of the running generator activation (`running_generator`): the frame suspends in
/// place, resuming after this record, and its caller continues with the step's result.
pub(super) fn lane_yield_in_window<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, i: &LaneInstruction) -> Resumed {
    // SAFETY: the record lies in the running view.
    let pc = unsafe { ip.offset_from((*cx).base) } as usize;
    // The generator receives the next value in this record's A register.
    lane_suspend_generator(cx, pc + 1, i.a & crate::bytecode::REGISTER_MASK, r.get(i.b), None)
}

/// `yield*` of the running generator activation (`running_generator`): a step of a delegate
/// generator made in place as an activation whose fresh result waits in the state register for
/// this record, made again; that result taken back, the delegate's completion goes to the
/// destination, and any other step passes on as this generator's own yield of the delegate's
/// result object. Anything else takes the general step.
pub(super) fn lane_yield_star_in_window<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, i: &LaneInstruction) -> Resumed {
    let vm = unsafe { vm(cx) };
    let (state, next_method) = ImmediateLayout::register_pair(i.imm());
    let destination = i.a & crate::bytecode::REGISTER_MASK;
    let waiting = r.get(state);
    if waiting.is_undefined() {
        let (delegate, method) = (r.get(i.c), r.get(next_method));
        if vm.steps_as_generator(delegate, method)
            && let Some((registers, start)) = lane_resume_generator(cx, r, ip, delegate, method, r.get(destination), Completion::Delegates)
        {
            return Resumed::At(registers, start, Carried::script(cx));
        }
        return Resumed::Exit(exit(ip));
    }
    let Some((value, done)) = vm.own_iterator_result_parts(waiting) else {
        return Resumed::Exit(exit(ip));
    };
    r.set(state, Value::UNDEFINED);
    if done {
        r.set(destination, value);
        return Resumed::At(r, following(ip), Carried::script(cx));
    }
    // The generator resumes at this record, which takes the value its caller sends next.
    // SAFETY: the record lies in the running view.
    let pc = unsafe { ip.offset_from((*cx).base) } as usize;
    lane_suspend_generator(cx, pc, destination, Value::UNDEFINED, Some(waiting))
}
