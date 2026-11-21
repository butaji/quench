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

impl ActivationView {
    /// The dynamic bindings its record holds (implicit ones, a direct eval's), which the
    /// environment its window makes for its first closure takes over, as a frame's promotion
    /// takes its own (`Vm::promote_frame_environment`); none once taken, or for an activation
    /// without a record (a plain header's is its side record's, `frame::OWNS_FLAG`).
    pub(super) fn take_record_bindings(&self) -> Vec<(crate::bytecode::Atom, Value)> {
        let record = self.script().map_or(std::ptr::null_mut(), |script| script.record);
        // SAFETY: a record outlives the activation that runs over it, and nothing else accesses it.
        unsafe { record.as_mut() }.map(FrameRecord::take_dynamic_bindings).unwrap_or_default()
    }
}

impl LaneStack {
    /// Pushes `activation` over the register file of a suspended frame, which its window names
    /// (the record's `registers`): the window is marked as no lane stack's (`HEAP_WINDOW`), so
    /// its return frees nothing there.
    ///
    /// # Safety
    /// The activations' room holds one more.
    pub(super) unsafe fn push_generator_activation(&mut self, mut activation: Activation) {
        activation.window |= HEAP_WINDOW;
        // A heap window stays a record (side record R waits for the window's own header words);
        // the plain headers below it are sealed, not spilled (one-frame-one-call.md F4 (3)).
        self.seal_plain();
        // SAFETY: as the caller guarantees; a script activation holds no Wasm storage.
        unsafe { self.records_mut().push_unchecked(activation) };
    }

    /// The iterators a resumed generator's loops left open, now the loops of the activation at
    /// `depth` (`LaneStack::iterators`).
    #[inline(always)]
    pub(super) fn restore_iterators(&mut self, depth: usize, open: &mut Vec<super::super::ActiveIterator>) {
        if !open.is_empty() {
            self.iterators.extend(open.drain(..).map(|iterator| (depth, iterator)));
        }
    }

    /// The iterators the activation at `depth`, the innermost, pushed (its entries, the last),
    /// moved to the end of `open`, the suspending frame's (`FrameRecord::active_iterators`).
    #[inline(always)]
    pub(in crate::vm) fn take_iterators(&mut self, depth: usize, open: &mut Vec<super::super::ActiveIterator>) {
        if self.iterators.last().is_some_and(|(owner, _)| *owner >= depth) {
            let first = self.iterators.partition_point(|(owner, _)| *owner < depth);
            open.extend(self.iterators.drain(first..).map(|(_, iterator)| iterator));
        }
    }
}

/// Whether the running activation is a generator over its record's register file.
#[inline(always)]
pub(super) fn running_generator(vm: &Vm) -> bool {
    vm.lane_stack.top_frame().and_then(|frame| frame.script()).is_some_and(|script| script.completion.is_generator())
}

/// `generator_step_miss` of a for-of step of a generator's `next` that the lane resumes as an
/// activation (`resume_generator_in_lane`); `None` leaves any other step, and a step that returns
/// its result from the frame, to the general ones.
#[inline(never)]
pub(super) fn resume_generator_step_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    method: Value,
    generator: Value,
    completion: Completion,
) -> Option<(Registers, Ip)> {
    if !crate::staged::call_iteration::resumes_at_site(i.a) {
        return None;
    }
    resume_generator_in_lane(cx, r, ip, generator, method, Value::UNDEFINED, completion)
}

/// A `Call` record `i` of `native`, the callee in `b` and the receiver in `c`, that is this realm's
/// `Generator.prototype.next` of a generator that is not running (`Vm::steps_as_generator`): the
/// generator resumes as an activation over its record (`resume_generator_in_lane`), sent the call's
/// first argument, and its yield or return writes a fresh iterator result to the call's register
/// (`Completion::Steps`), as the native's call would. No frame is made of the caller's
/// activations. `None`, before any effect, leaves the call to the native: another callee, a call
/// that returns its result from the frame, a run that makes no script calls, or a generator with
/// no window.
#[inline(never)]
pub(super) fn resume_called_generator(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    native: Native,
) -> Option<(Registers, Ip)> {
    let (method, generator) = (r.get(i.b), r.get(i.c));
    if native != Native::GeneratorNext
        || !crate::staged::call_iteration::resumes_at_site(i.a)
        || !unsafe { (*cx).script_calls }
        || !unsafe { vm(cx) }.steps_as_generator(generator, method)
    {
        return None;
    }
    // SAFETY: the call's window is registers of the running function.
    let sent = unsafe { call_window_in_lane(r, i) }.first().copied().unwrap_or(Value::UNDEFINED);
    resume_generator_in_lane(cx, r, ip, generator, method, sent, Completion::Steps)
}

/// `frame_call_in_lane` of a generator function of the running program whose parameters run no code
/// of their own (`Function::parameter_end_pc` 0): the generator object, made as the general call
/// makes it (`Vm::call_generator`) beside the caller's activations (`in_place_in_lane`), since making
/// it runs no guest code, and the call completed with it (`native_complete_in_lane`). No frame is made
/// of the caller's activations. `None`, before any effect, leaves the call to the general path.
#[inline(never)]
pub(super) fn start_called_generator(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    callee: Value,
) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    let returns = i.a & crate::bytecode::RETURN_REGISTER != 0;
    if !unsafe { (*cx).script_calls } || vm.with_scope_entered || returns && !returning_call_admitted(cx) {
        return None;
    }
    let (program, function, env) = vm.same_realm_user_target(callee)?;
    // SAFETY: the context's program outlives the lane run.
    let body = unsafe { &*(*cx).code }.function_get(function as usize)?;
    if program != unsafe { (*cx).program } || !body.is_generator || body.parameter_end_pc != 0 {
        return None;
    }
    // SAFETY: the call's window is registers of the running function, copied before the
    // generator's frame takes them.
    let arguments = super::super::call_arguments::CallArguments::from_slice(unsafe { call_window_in_lane(r, i) });
    let (this, context) = (r.get(i.c), CallContext::user_function(function, callee));
    match unwound_in_place(cx, ip, |vm, code| vm.call_generator(code, function, env, this, arguments.as_slice(), context)) {
        Ok(value) => Some(native_complete_in_lane(cx, ip, i, Some(value))),
        Err(next) => Some(Resumed::Exit(next)),
    }
}

/// How a resumed generator's window carries its locals (`FrameRecord::open_environment_window`):
/// its function's captured-slot mask and, for a frame that made its environment, the slot storage
/// of that environment.
#[derive(Clone, Copy)]
pub(super) struct WindowLocals {
    captured: u64,
    own_slots: Option<*const crate::heap::EnvironmentSlots>,
}

/// The collection epoch (the capture's remembered environment), program-store epoch (its
/// view) and environment, made or not, a `WindowPlan` holds under.
type PlanKey = (u64, u64, Value, bool);

/// What a suspended frame's windowed resumes derive from its function and environment alone,
/// kept on its record (`FrameRecord::window_plan`) while its key holds (`PlanKey`). Each resume
/// of a generator or async body re-derived them. The key and the locals lie flat, their flags
/// together: as a tuple and an option they took each record 88 bytes, flat 72.
#[derive(Clone, Copy)]
pub(in crate::vm) struct WindowPlan {
    collection_epoch: u64,
    context_epoch: u64,
    key_env: Value,
    pub(super) view: *const LaneView,
    pub(super) parent: Value,
    pub(super) capture: *const usize,
    /// The window's locals (`locals`): their captured-slot mask and own slot storage.
    locals_captured: u64,
    own_slots: *const crate::heap::EnvironmentSlots,
    key_captured: bool,
    has_locals: bool,
    has_own_slots: bool,
    /// Whether the window carries an environment after the registers (`Vm::window_layout`).
    pub(super) environment: bool,
}

const _: () = assert!(size_of::<Option<WindowPlan>>() <= 72);

impl WindowPlan {
    fn new(key: PlanKey, view: *const LaneView, parent: Value, locals: Option<WindowLocals>, capture: *const usize, environment: bool) -> Self {
        let (collection_epoch, context_epoch, key_env, key_captured) = key;
        let WindowLocals { captured: locals_captured, own_slots } = locals.unwrap_or(WindowLocals { captured: 0, own_slots: None });
        Self {
            collection_epoch, context_epoch, key_env, view, parent, capture, locals_captured,
            own_slots: own_slots.unwrap_or(std::ptr::null()),
            key_captured, has_locals: locals.is_some(), has_own_slots: own_slots.is_some(), environment,
        }
    }

    /// Whether it was derived under `key`.
    #[inline(always)]
    fn holds(&self, key: PlanKey) -> bool {
        (self.collection_epoch, self.context_epoch, self.key_env, self.key_captured) == key
    }

    /// How the window carries its locals, where it carries an environment.
    #[inline(always)]
    pub(super) fn locals(&self) -> Option<WindowLocals> {
        self.has_locals.then_some(WindowLocals { captured: self.locals_captured, own_slots: self.has_own_slots.then_some(self.own_slots) })
    }
}

/// The plan a windowed resume of `frame`, laid out with an environment window when
/// `environment`, runs under: its record's own while it still holds, else derived anew (the
/// caller stores it back on the record it resumes). `None`, before any effect, where the
/// function has no view or environment layout.
pub(super) fn window_plan(vm: &mut Vm, code: &ResidualProgram, frame: &FrameRecord, environment: bool) -> Option<WindowPlan> {
    let (program, function, env, captured) = (frame.program, frame.function, frame.env, frame.captured);
    let key = (vm.heap.collection_epoch(), vm.programs.context_epoch(), env, captured);
    if let Some(plan) = frame.window_plan.filter(|plan| plan.holds(key)) {
        return Some(plan);
    }
    let view = vm.view_any_in_lane(program, function)?;
    let (parent, locals) = window_environments(vm, view, env, captured, environment)?;
    let capture = vm.target_capture(code, program, parent);
    Some(WindowPlan::new(key, view, parent, locals, capture, environment))
}

/// `window_plan` of a suspended `frame`, of the active program: its record's own while it
/// holds, else the last one derived for its function under the same environment
/// (`LaneStack::last_window_plan`), else laid out as its function's plan says
/// (`Vm::window_layout`). Whether the record needs it stored (`store_window_plan`): only a
/// plan it does not hold already.
#[inline(always)]
pub(super) fn cached_window_plan(vm: &mut Vm, code: &ResidualProgram, frame: &FrameRecord) -> Option<(WindowPlan, bool)> {
    let key = (vm.heap.collection_epoch(), vm.programs.context_epoch(), frame.env, frame.captured);
    if let Some(plan) = frame.window_plan.filter(|plan| plan.holds(key)) {
        return Some((plan, false));
    }
    shared_window_plan(vm, code, frame, key).map(|plan| (plan, true))
}

/// The plan a windowed resume of `frame` runs under, kept on its record: the record's own while
/// its key holds, else the one `cached_window_plan` finds or derives, stored there first, so a
/// resume reads the fields it needs in place and copies no plan. `None`, before any effect, where
/// the function has no view or environment layout.
///
/// # Safety
/// The owner keeps `frame` boxed, and nothing else accesses it, while the reference lives.
#[inline(always)]
pub(super) unsafe fn record_window_plan<'a>(vm: &mut Vm, code: &ResidualProgram, frame: *mut FrameRecord) -> Option<&'a WindowPlan> {
    // SAFETY: as the caller guarantees.
    let record = unsafe { &mut *frame };
    let key = (vm.heap.collection_epoch(), vm.programs.context_epoch(), record.env, record.captured);
    if !matches!(&record.window_plan, Some(plan) if plan.holds(key)) {
        record.window_plan = Some(shared_window_plan(vm, code, record, key)?);
    }
    record.window_plan.as_ref()
}

/// Plans `shared_window_plan` keeps, one slot per function index modulo this.
pub(in crate::vm) const SHARED_PLANS: usize = 8;

/// `cached_window_plan` of a frame whose record holds no plan under `key`.
#[inline(never)]
fn shared_window_plan(vm: &mut Vm, code: &ResidualProgram, frame: &FrameRecord, key: PlanKey) -> Option<WindowPlan> {
    let shared = (vm.active_program, frame.function);
    let slot = frame.function as usize % SHARED_PLANS;
    if frame.program == vm.active_program
        && let Some((.., plan)) = vm.lane_stack.last_window_plan[slot].filter(|(program, function, plan)| (*program, *function) == shared && plan.holds(key))
    {
        return Some(plan);
    }
    let environment = vm.window_layout(frame, code)?;
    let plan = window_plan(vm, code, frame, environment)?;
    if !frame.captured {
        vm.lane_stack.last_window_plan[slot] = Some((shared.0, shared.1, plan));
    }
    Some(plan)
}

/// Keeps `plan` on the record of `frame` when `cached_window_plan` derived it anew.
#[inline(always)]
pub(super) fn store_window_plan(frame: *mut FrameRecord, (plan, fresh): (WindowPlan, bool)) {
    if fresh {
        // SAFETY: the owner keeps the frame boxed for the run.
        unsafe { (*frame).window_plan = Some(plan) };
    }
}

/// The environment a generator's closures and captures run under, and its window's locals when
/// `environment` (`Vm::window_layout`) says the window carries them: the frame's
/// `env` is its own environment once it made one (`captured`), whose parent the activation
/// runs under, else the closure's. `None`, before any effect, where `view` has no environment
/// window or the environment is no cell.
pub(super) fn window_environments(
    vm: &Vm,
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
/// effect, where the lane has no room or no view for it. Inlined into each caller, a for-of
/// step's above all (`resume_generator_step_in_lane`), so a step makes one call.
#[inline(always)]
pub(super) fn resume_generator_in_lane(
    cx: *mut LaneContext,
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
    let (record, suspended) = resumable_generator_frame(vm, generator)?;
    let frame = suspended.0;
    let depth = crate::staged::call_iteration::concrete_admit_resume(vm.lane_stack.activation_count(), vm.lane_stack.activation_limit.get())?;
    // SAFETY: the record keeps the frame boxed, and the reference ends with the statement.
    let plan = *unsafe { record_window_plan(vm, code, frame) }?;
    let (view, parent, locals, capture, environment) = (plan.view, plan.parent, plan.locals(), plan.capture, plan.environment);
    vm.resume_generator_window((generator, record), suspended, method, sent, depth);
    if let Some(locals) = locals {
        open_window_locals(vm, frame, locals);
    }
    // SAFETY: the record keeps the frame, and so its registers, until the activation ends; the
    // window, laid out above, moves the registers no more.
    let (window, size) = unsafe { ((&mut (*frame).registers).as_mut_ptr(), (&(*frame).registers).len()) };
    let (this, pc, function, program) = unsafe { ((*frame).this(), (*frame).pc, (*frame).function, (*frame).program) };
    push_generator_window(cx, vm, (r, ip, depth), GeneratorWindow { this: this.to_bits(), window: window.addr(), function, generator, completion, environment, record: frame });
    let entry = PreparedEntry { view, env: parent, capture, this, registers: size, passed: 0, argc: 0, keeps: false, size, program, function };
    // SAFETY: the window holds the suspended registers, which nothing else writes while it runs,
    // and `pc` is where the frame resumes in its function's view.
    let start = unsafe { enter_suspended_in_lane(cx, &entry, window, pc) };
    Some((Registers(window), start))
}

/// `generator`'s record and its suspended frame, with the register that takes the value the
/// resume sends, when the frame resumes in this realm and program. The record found is the one
/// `Vm::generator_record_mut` keeps for the next lookups (`LaneStack::last_generator`).
/// SAFETY of the result: the generator's cell keeps its record, and the record its frame, boxed:
/// nothing moves or frees either before the resume.
#[inline(always)]
fn resumable_generator_frame(vm: &mut Vm, generator: Value)
    -> Option<(*mut GeneratorRecord, (*mut FrameRecord, Option<u16>))> {
    let (realm, active) = (vm.realm.globals, vm.active_program);
    let record = vm.generator_record_mut(generator)?;
    let suspended = record.suspended_frame()?;
    // SAFETY: the record keeps the frame boxed.
    let resumable = record.realm == realm && unsafe { (*suspended.0).program } == active;
    resumable.then_some((std::ptr::from_mut(record), suspended))
}

/// Lays `frame`'s environment window after its registers (`FrameRecord::open_environment_window`).
/// A frame holding implicit bindings keeps them in its record until a closure makes the window's
/// environment, which takes them over (`Activation::take_record_bindings`). An environment the
/// frame made already is written through the window's stores without the barrier a heap write
/// takes, so it joins the remembered set once, as a capture store's target does
/// (`Vm::target_capture`).
pub(super) fn open_window_locals(vm: &mut Vm, frame: *mut FrameRecord, locals: WindowLocals) {
    // SAFETY: the record owns the box, which nothing moves or drops until the run ends.
    let frame = unsafe { &mut *frame };
    if locals.own_slots.is_some() {
        vm.heap.remember_cell(frame.env);
    }
    frame.open_environment_window(locals.captured, locals.own_slots);
}

/// What the activation of a resumed generator runs (`generator_activation`): its frame's
/// receiver and window, its function, the generator, how its caller takes the step, whether its window
/// carries an environment (`ScriptActivation::environment`) and the record it runs over.
struct GeneratorWindow {
    this: u64,
    window: usize,
    function: u32,
    generator: Value,
    completion: Completion,
    environment: bool,
    record: *mut FrameRecord,
}

/// Pushes `window`'s activation as the lane's innermost, at `depth` below the call record at `ip` of
/// the running function with registers `r`, and resumes the frame's loops and scope chain as its own.
#[inline(always)]
fn push_generator_window(cx: *mut LaneContext, vm: &mut Vm, (r, ip, depth): (Registers, Ip, usize), window: GeneratorWindow) {
    let (frame, environment) = (window.record, window.environment);
    let activation = generator_activation(cx, r, ip, window);
    // SAFETY: the caller's limit check leaves room for the activation, and the frame's loops and
    // scope chain resume as the new activation's (`LaneStack::iterators`, `LaneStack::with_objects`).
    unsafe {
        vm.lane_stack.push_generator_activation(activation);
        vm.lane_stack.restore_iterators(depth, &mut (*frame).active_iterators);
        vm.lane_stack.restore_with_objects(depth, &mut *frame);
        // Only an environment window holds an `arguments` object that rides it.
        if environment {
            vm.record_window_owner(&*(*cx).code, depth);
        }
    }
}

/// The activation of `window`'s generator for the call record at `ip`, made by the running
/// function with registers `r`.
#[inline(always)]
fn generator_activation(cx: *mut LaneContext, r: Registers, ip: Ip, window: GeneratorWindow) -> Activation {
    // SAFETY: the running lane's context, which nothing else accesses.
    let mut activation = Activation::of_parts(
        window.function,
        ip,
        unsafe { (*cx).base },
        r,
        unsafe { (*cx).program },
        ActivationKind::Script(ScriptActivation {
            callee: window.generator,
            argc: 0,
            completion: window.completion,
            forwards: false,
            kept: 0,
            environment: window.environment,
            record: window.record,
        }),
    );
    activation.this = window.this;
    activation.window = window.window;
    activation
}

/// `yield` (or the pass-on of a `yield*`) of the running generator activation: its frame, resuming
/// at `pc`, suspends into its record, where the value the generator receives next goes to
/// `destination`, and the activation pops. The caller resumes with the step's result: the
/// iterator result of a yield of `value`, or `passed_on`, a delegate's own result object.
#[inline(never)]
pub(super) fn suspend_generator_in_lane(
    cx: *mut LaneContext,
    pc: usize,
    destination: crate::bytecode::Register,
    value: Value,
    passed_on: Option<Value>,
) -> Resumed {
    let vm = unsafe { vm(cx) };
    let depth = vm.lane_stack.activation_count() - 1;
    // Read where it is (no spill): a generator's frame is a heap-window record. It is read once,
    // as it pops, after its frame took back what it held.
    let running = vm.lane_stack.top_frame().and_then(|frame| frame.script()).expect("a generator runs as a script activation");
    vm.suspend_generator_window((running.callee, running.record), pc, destination, value, depth);
    let frame = vm.lane_stack.pop_activation().expect("a running generator's activation");
    let (memory, _) = restore_caller_in_lane(cx, &frame);
    resume_generated_in_lane(vm, &frame, memory, (value, false), passed_on)
}

/// The return of the generator activation (`frame`, its return words), popped with its caller
/// restored (`memory`), with `value`: the generator completes, and its caller resumes with the
/// step's final result.
pub(super) fn generator_returned_in_lane(
    cx: *mut LaneContext,
    frame: &ActivationView,
    memory: MemoryView,
    value: Value,
) -> Resumed {
    let vm = unsafe { vm(cx) };
    vm.finish_generator_window(frame.callee());
    resume_generated_in_lane(vm, frame, memory, (value, true), None)
}

/// The caller of a generator `frame` it left (`suspend_generator_in_lane`,
/// `generator_returned_in_lane`) resumes with the step's `value` and `done`, or the delegate's own
/// result `passed_on`: a for-of step's reads take them in their registers and the caller goes
/// on past them (`Completion::StepReads`), as `iterator_step_reads_miss` makes an array's step;
/// any other caller takes the step's result object after its call, in the call's register, or,
/// for a delegate's step, at the `yield*` itself in its state register.
fn resume_generated_in_lane(vm: &mut Vm, frame: &ActivationView, memory: MemoryView, (value, done): (Value, bool), passed_on: Option<Value>) -> Resumed {
    let completion = frame.completion();
    let (call, caller) = (frame.call(), frame.caller_registers());
    if let (Some(Completion::StepReads(form)), None) = (completion, passed_on) {
        // SAFETY: the step's records follow its call in the caller's live view (`for_of_step_in_lane`).
        let (done_read, value_read) = unsafe { (&*call.add(FOR_OF_DONE_READ), &*call.add(form.value_read())) };
        caller.set(done_read.a, if done { Value::TRUE } else { Value::FALSE });
        if done {
            return resume_at_in_lane(frame, memory, unsafe { call.add(form.exit()) }, None);
        }
        caller.set(value_read.a, value);
        return resume_at_in_lane(frame, memory, unsafe { call.add(form.records()) }, None);
    }
    let result = passed_on.unwrap_or_else(|| vm.generator_step_result(value, done, completion.is_some_and(Completion::reuses_result)));
    // The result may be a fresh allocation, which the caller's safepoints collect after.
    note_allocation(vm);
    if completion == Some(Completion::Delegates) {
        // SAFETY: the `yield*` record belongs to the caller's live view.
        let (state, _) = ImmediateLayout::register_pair(unsafe { (*call).imm() });
        return resume_at_in_lane(frame, memory, call, Some((state, result)));
    }
    resume_after_call_in_lane(frame, memory, result)
}

/// The `next` step of the delegate `delegate` by `method` with `input`, a `yield*` at `ip` of
/// the running generator activation with registers `r` makes in place (the lane frame's answer to
/// `CallFrame::delegation_in_place`): a delegate generator resumes as an activation whose fresh
/// result waits in the record's state register for this record, run again
/// (`Completion::Delegates`, `resume_generated_in_lane`). `None`, before any effect, where the
/// delegate is no generator `method` steps, or the lane has no room or view for it.
pub(super) fn delegate_in_place_in_lane(cx: *mut LaneContext, r: Registers, ip: Ip, (delegate, method, input): (Value, Value, Value))
    -> Option<Resumed> {
    if !unsafe { vm(cx) }.steps_as_generator(delegate, method) {
        return None;
    }
    let (registers, start) = resume_generator_in_lane(cx, r, ip, delegate, method, input, Completion::Delegates)?;
    Some(Resumed::At(registers, start, Carried::script(cx)))
}
