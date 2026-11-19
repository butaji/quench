//! The staged operations the lane runs over a `LaneFrame`: each builds the frame of the running
//! record, steps the operation on the concrete stage, and answers the edge or the written value.
use super::*;

/// The edge a staged jump test takes, or `None` where the value's heap cell decides it.
#[inline(always)]
pub(in crate::vm::dispatch_fast) fn staged_jump_if<const TAKEN: bool>(r: Registers, view: Carried, register: u16) -> Option<Edge> {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::new(r, view);
    jump_edge(crate::staged::step(&mut stage, &mut frame, Operation::JumpIf { register, taken: TAKEN }))
}

/// The edge a staged jump's step takes, or `None` where the interpreter's own path decides it.
#[inline(always)]
fn jump_edge(step: Step<u64, LaneControl>) -> Option<Edge> {
    match step {
        Step::Control(LaneControl::Edge(edge)) => Some(edge),
        Step::Control(LaneControl::Resumed(_) | LaneControl::Suspend(_)) => unreachable!("a jump suspends nothing"),
        Step::Unresolved => None,
        Step::Value(_) => unreachable!("a jump produces no value"),
    }
}

/// The `typeof` name index of `source` into `destination` by the staged operation; false,
/// writing nothing, where a heap value's cell decides the name.
#[inline(always)]
pub(in crate::vm::dispatch_fast) fn staged_typeof_index(r: Registers, view: Carried, destination: u16, source: u16) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::new(r, view);
    match crate::staged::step(&mut stage, &mut frame, Operation::TypeofIndex { destination, source }) {
        Step::Value(_) => true,
        Step::Unresolved => false,
        Step::Control(_) => unreachable!("a typeof transfers no control"),
    }
}

/// Whether `register` is coercible to an object (neither `null` nor `undefined`), by the
/// staged RequireObjectCoercible.
#[inline(always)]
pub(in crate::vm::dispatch_fast) fn staged_require_object_coercible(r: Registers, view: Carried, register: u16) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::new(r, view);
    match crate::staged::step(&mut stage, &mut frame, Operation::RequireObjectCoercible { register }) {
        Step::Value(_) => true,
        Step::Unresolved => false,
        Step::Control(_) => unreachable!("a coercibility test transfers no control"),
    }
}

/// Whether the staged RequireIteratorResult accepts `register` as it is (a direct object or
/// array record); false where the full operation decides.
#[inline(always)]
pub(in crate::vm::dispatch_fast) fn staged_require_iterator_result(r: Registers, view: Carried, register: u16) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::new(r, view);
    matches!(crate::staged::step(&mut stage, &mut frame, Operation::RequireIteratorResult { register }), Step::Value(_))
}

/// ToNumeric of a Number register `source` into `destination`, by the staged operation;
/// false, writing nothing, where `source` is not a Number.
#[inline(always)]
pub(in crate::vm::dispatch_fast) fn staged_to_numeric(r: Registers, view: Carried, destination: u16, source: u16) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::new(r, view);
    match crate::staged::step(&mut stage, &mut frame, Operation::Number(NumberOperation::ToNumeric { destination, source })) {
        Step::Value(_) => true,
        Step::Unresolved => false,
        Step::Control(_) => unreachable!("a conversion transfers no control"),
    }
}

/// ToPropertyKey of the direct string in `source` into `destination`, by the staged operation;
/// false, writing nothing, where the value is no direct string.
#[inline(always)]
pub(in crate::vm::dispatch_fast) fn staged_property_key(r: Registers, view: Carried, destination: u16, source: u16) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::new(r, view);
    matches!(crate::staged::step(&mut stage, &mut frame, Operation::PropertyKey { destination, source }), Step::Value(_))
}

/// The activation's `this` into `destination` by the staged LoadThis; false, writing
/// nothing, where the lane does not hold it (`this` is `Value::DELETED`).
#[inline(always)]
pub(in crate::vm::dispatch_fast) fn staged_load_this(r: Registers, view: Carried, this: Value, destination: u16) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::new(r, view);
    frame.this = this;
    match crate::staged::step(&mut stage, &mut frame, Operation::LoadThis { destination }) {
        Step::Value(_) => true,
        Step::Unresolved => false,
        Step::Control(_) => unreachable!("a load transfers no control"),
    }
}

/// `LoadLocalPlain` of `slot` into `destination` by the staged operation; false, writing
/// nothing, where the activation has no such local slot.
#[inline(always)]
pub(in crate::vm::dispatch_fast) fn staged_load_local_plain<H: Host, const ACC: u8>(cx: *mut LaneContext<H>,
    r: Registers, view: Carried, destination: u16, slot: u16) -> bool {
    let vm = unsafe { vm(cx) };
    let frame = &mut vm.frames[unsafe { (*cx).frame() }];
    let (locals, len) = (frame.locals.as_mut_ptr(), frame.locals.len());
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC>::with_locals(r, view, Value::DELETED, locals, len);
    matches!(crate::staged::step(&mut stage, &mut frame,
        Operation::LoadLocalPlain { destination, slot }), Step::Value(_))
}

/// `StoreLocalPlain` of `source` to `slot`, and to `result` where the instruction has one,
/// by the staged operation; false, writing nothing, where the activation has no such slot.
#[inline(always)]
pub(in crate::vm::dispatch_fast) fn staged_store_local_plain<H: Host, const ACC: u8>(cx: *mut LaneContext<H>,
    r: Registers, view: Carried, source: u16, slot: u16, result: Option<u16>) -> bool {
    let vm = unsafe { vm(cx) };
    let frame = &mut vm.frames[unsafe { (*cx).frame() }];
    let (locals, len) = (frame.locals.as_mut_ptr(), frame.locals.len());
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC>::with_locals(r, view, Value::DELETED, locals, len);
    matches!(crate::staged::step(&mut stage, &mut frame,
        Operation::StoreLocalPlain { source, slot, result }), Step::Value(_))
}

/// `StoreCapture` of `source` to the capture at `depth` and `slot` past the running function's
/// dense slots, by the staged operation and the lane's own store (`lane_capture_store`); false,
/// storing nothing, where the binding does not take the direct store.
#[inline(always)]
pub(in crate::vm::dispatch_fast) fn staged_store_capture<H: Host>(cx: *mut LaneContext<H>, r: Registers, view: Carried,
    source: u16, (depth, slot): (u16, u16)) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::new(r, view);
    frame.captures = Some(CaptureStores::of(cx));
    matches!(crate::staged::step(&mut stage, &mut frame, Operation::StoreCapture { source, depth, slot }), Step::Value(_))
}

/// The edge the staged comparison jump of registers `left` and `right` takes, or `None` where the
/// interpreter's own path decides it. The jump's backward poll is `lane_jump`'s, the one copy
/// every jump takes through `jump_on_edge!`.
#[inline(always)]
pub(in crate::vm::dispatch_fast) fn staged_compare_jump<H: Host>(cx: *mut LaneContext<H>, r: Registers, view: Carried,
    comparison: Comparison, [left, right]: [u16; 2]) -> Option<Edge> {
    staged_comparison_jump::<H>(cx, r, view, comparison, [RecordOperand::Register(left), RecordOperand::Register(right)])
}

/// The comparison `operator` makes of the observed `values`: two Numbers decide a relation as
/// binary64 (the static arm the handler selects), and any other relation is no Number relation,
/// which the handler's cold path decides.
#[inline(always)]
fn observed(operator: u32, [left, right]: [Value; 2]) -> Comparison {
    match Comparison::of_operator(operator).by_vm() {
        Comparison::Relation { .. } if left.as_number().is_none() || right.as_number().is_none() => Comparison::Operator(operator),
        comparison => comparison,
    }
}

/// The edge the staged jump of `operator` over the record's `operands` takes: the comparison the
/// observed operands select (`observed`), or `None` where the interpreter's cold path decides it.
#[inline(always)]
pub(in crate::vm::dispatch_fast) fn staged_record_jump<H: Host>(cx: *mut LaneContext<H>, r: Registers, view: Carried, operator: u32,
    operands: [RecordOperand; 2]) -> Option<Edge> {
    let values = [operands[0].value(cx, r)?, operands[1].value(cx, r)?];
    staged_comparison_jump::<H>(cx, r, view, observed(operator, values), operands)
}

/// The edge the staged jump of `comparison` over the record's `operands` takes, or `None` where
/// the interpreter's own path decides it.
#[inline(always)]
pub(in crate::vm::dispatch_fast) fn staged_comparison_jump<H: Host>(cx: *mut LaneContext<H>, r: Registers, view: Carried, comparison: Comparison,
    operands: [RecordOperand; 2]) -> Option<Edge> {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::new(r, view);
    frame.decisions = Some(Decisions::of(cx));
    let ([left, right], reads) = RecordOperand::staged_pair::<H>(cx, operands);
    frame.reads = reads;
    let operation = Operation::CompareJump { comparison, left, right, negate: true };
    jump_edge(crate::staged::step(&mut stage, &mut frame, operation))
}

/// `LoadCapture` of the capture at `depth` and `slot` into `destination` (of the root's projected
/// binding where `root`), by the staged operation: the running function's own slots, else the lane's
/// remembered walks; false, writing nothing, where they hold no such value.
/// The row runs first on a frame without the lane's reads: an own slot needs none, and with no
/// reads to call nothing of the frame escapes, so it stays in registers. The remembered walks
/// run the row again with them (`staged_load_capture_walked`), out of line.
#[inline(always)]
pub(in crate::vm::dispatch_fast) fn staged_load_capture<H: Host>(cx: *mut LaneContext<H>, r: Registers, view: Carried,
    destination: u16, at: (u16, u16), root: bool) -> bool {
    load_capture_with::<H>(cx, r, view, (destination, at, root), Reads::NONE)
        || staged_load_capture_walked::<H>(cx, r, view, destination, at, root)
}

/// `staged_load_capture` of a slot of the running function's own (depth 0, not the root's),
/// which the remembered walks never answer (`lane_capture_read`): the row alone, so a miss
/// leaves for the caller's walk with nothing spilled for a second run.
#[inline(always)]
pub(in crate::vm::dispatch_fast) fn staged_load_own_capture<H: Host>(cx: *mut LaneContext<H>, r: Registers, view: Carried,
    destination: u16, slot: u16) -> bool {
    load_capture_with::<H>(cx, r, view, (destination, (0, slot), false), Reads::NONE)
}

/// `staged_load_capture` through the lane's remembered walks (`Reads::of_captures`).
#[cold]
#[inline(never)]
fn staged_load_capture_walked<H: Host>(cx: *mut LaneContext<H>, r: Registers, view: Carried,
    destination: u16, at: (u16, u16), root: bool) -> bool {
    load_capture_with::<H>(cx, r, view, (destination, at, root), Reads::of_captures::<H>(cx))
}

/// The `LoadCapture` row of `destination`, capture `(depth, slot)` and `root` over the running
/// function's own slots and `reads`.
#[inline(always)]
fn load_capture_with<H: Host>(cx: *mut LaneContext<H>, r: Registers, view: Carried,
    (destination, (depth, slot), root): (u16, (u16, u16), bool), reads: Reads) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::new(r, view);
    frame.capture_slots = Some(capture_reads(unsafe { (*cx).registers }.capture()));
    frame.reads = reads;
    matches!(crate::staged::step(&mut stage, &mut frame, Operation::LoadCapture { destination, depth, slot, root }), Step::Value(_))
}

/// `LoadLocal` and `LoadEnvLocal` of `slot` into `destination` by `staged::read_initialized_local`, over the binding
/// the frame owns (`Vm::owner_binding_mut`: its environment's slot once the environment is captured, else its local)
/// as a one-slot window; false, writing nothing, for a root body's frame, a slot outside the frame or a binding in
/// its temporal dead zone, which the general path raises. A slot a mapped `arguments` aliases reads the argument.
#[inline(always)]
pub(in crate::vm::dispatch_fast) fn staged_load_local_checked<H: Host, const ACC: u8>(cx: *mut LaneContext<H>,
    r: Registers, view: Carried, destination: u16, slot: usize) -> bool {
    let vm = unsafe { vm(cx) };
    let (code, frame) = (unsafe { &*(*cx).code }, unsafe { (*cx).frame() });
    let function = code.function(vm.frames[frame].function as usize);
    if vm.frames[frame].function == super::super::ROOT_FUNCTION_ID {
        return false;
    }
    let Some(binding) = vm.owner_binding_mut(code, VarOwner::Frame(frame), slot).map(std::ptr::from_mut) else { return false };
    let mut stage = Concrete::new(std::ptr::null());
    let mut window = LaneFrame::<ACC>::with_locals(r, view, Value::DELETED, binding, 1);
    let Some(word) = read_initialized_local(&mut stage, &mut window, destination, 0, ()) else { return false };
    if crate::vm::arguments::mapped_arguments_slot(function, slot).is_some() {
        r.set(destination, vm.mapped_argument_load(code, VarOwner::Frame(frame), slot, Value::from_bits(word)));
    }
    true
}

/// `StoreLocal` and `StoreEnvLocal` of `source` to `slot` over the binding the frame owns, by
/// `staged::write_initialized_local` (`staged::write_local` for the store that initializes its binding), mirrored
/// into a mapped `arguments` (`Vm::mapped_argument_store`); false, writing nothing, for a root
/// body's frame, a self binding, a slot outside the frame or an assignment to a binding in its temporal dead zone.
#[inline(always)]
pub(in crate::vm::dispatch_fast) fn staged_store_local_checked<H: Host, const ACC: u8>(cx: *mut LaneContext<H>,
    r: Registers, view: Carried, (source, slot): (u16, usize), initializing: bool) -> bool {
    let vm = unsafe { vm(cx) };
    let (code, frame) = (unsafe { &*(*cx).code }, unsafe { (*cx).frame() });
    let function = code.function(vm.frames[frame].function as usize);
    if vm.frames[frame].function == super::super::ROOT_FUNCTION_ID || function.is_self_binding_slot(slot) {
        return false;
    }
    let Some(binding) = vm.owner_binding_mut(code, VarOwner::Frame(frame), slot).map(std::ptr::from_mut) else { return false };
    let mut stage = Concrete::new(std::ptr::null());
    let mut window = LaneFrame::<ACC>::with_locals(r, view, Value::DELETED, binding, 1);
    let stored = if initializing { write_local(&mut stage, &mut window, source, 0, Some(())) } else { write_initialized_local(&mut stage, &mut window, source, 0, ()) };
    let Some(word) = stored else { return false };
    vm.mapped_argument_store(code, VarOwner::Frame(frame), slot, Value::from_bits(word));
    true
}

/// The edge a staged unconditional jump takes.
#[inline(always)]
pub(in crate::vm::dispatch_fast) fn staged_jump(r: Registers, view: Carried) -> Edge {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::new(r, view);
    match crate::staged::step(&mut stage, &mut frame, Operation::Jump) {
        Step::Control(LaneControl::Edge(edge)) => edge,
        _ => unreachable!("a jump produces no value"),
    }
}

/// What the staged `await` conversion (`staged::await_source`) asks the lane context, erased of
/// its host so the frame names no `H`: whether a value is an object, and PromiseResolve where it
/// observes nothing (`Vm::lane_promise_resolve`).
#[derive(Clone, Copy)]
pub(in crate::vm::dispatch_fast) struct AwaitSource {
    object_like: fn(*mut (), Value) -> bool,
    resolve: fn(*mut (), Value) -> Option<Value>,
}

impl AwaitSource {
    /// The conversion of a lane context of host `H`.
    pub(in crate::vm::dispatch_fast) fn of<H: Host>() -> Self {
        Self {
            object_like: |context, value| unsafe { vm(context.cast::<LaneContext<H>>()) }.is_object_like(value),
            resolve: |context, value| {
                let cx = context.cast::<LaneContext<H>>();
                let vm = unsafe { vm(cx) };
                let promise = vm.intrinsic_promise_constructor();
                // SAFETY: the context's program outlives the lane run.
                vm.lane_promise_resolve(unsafe { &*(*cx).code }, promise, value)
            },
        }
    }
}

impl<const ACC: u8> crate::staged::CallFrame<Concrete> for LaneFrame<ACC> {
    /// Whether `value` is an object, by the lane context's VM; `None` without a context.
    fn object_like(&mut self, _stage: &mut Concrete, value: u64) -> Option<bool> {
        // A value outside the heap is no object; only a cell asks the lane context.
        if !Value::from_bits(value).is_heap() {
            return Some(false);
        }
        let host = self.host?;
        Some((host.awaits.object_like)(host.cx, Value::from_bits(value)))
    }

    /// PromiseResolve(%Promise%, `value`) where it observes nothing; `None` leaves a resolution
    /// that could run guest code, or throw into the frame's handlers, to the general path.
    fn promise_resolve(&mut self, _stage: &mut Concrete, value: u64) -> Option<u64> {
        let host = self.host?;
        (host.awaits.resolve)(host.cx, Value::from_bits(value)).map(Value::to_bits)
    }

    /// Whether the activation that suspends in place is async; `None` on a frame that suspends nothing.
    fn asynchronous(&self) -> Option<bool> {
        self.suspends.map(LaneSuspends::asynchronous)
    }

    /// The row's suspension, which the handler makes in place by the suspender it chose before
    /// the row ran (`lane_suspension`), so it never declines after the row's effects.
    fn suspend(&mut self, _stage: &mut Concrete, suspending: crate::staged::Suspending<u64>) -> Option<LaneControl> {
        self.suspends.map(|_| LaneControl::Suspend(suspending))
    }

    /// The delegation's reads the lane finishes without guest code (`LaneSuspends::delegation`).
    fn delegation(&mut self, _stage: &mut Concrete, step: crate::staged::DelegationStep<u64>) -> Option<(u64, ())> {
        self.suspends?.delegation(step).map(|word| (word, ()))
    }

    /// A delegate generator's step made in place (`LaneSuspends::in_place`).
    fn delegation_in_place(&mut self, _stage: &mut Concrete, step: crate::staged::DelegationStep<u64>, _state: Register)
        -> Option<LaneControl> {
        self.suspends?.in_place(self.registers, step).map(LaneControl::Resumed)
    }
}
