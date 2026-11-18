//! The staged operations the lane runs over a `LaneFrame`: each builds the frame of the running
//! record, steps the operation on the concrete stage, and answers the edge or the written value.
use super::*;

/// The edge a staged jump's step takes, or `None` where the interpreter's own path decides it.
#[inline(always)]
fn jump_edge(step: Step<u64, Edge>) -> Option<Edge> {
    match step {
        Step::Control(edge) => Some(edge),
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

/// Whether the staged RequireIteratorResult accepts `register` as it is (a direct object or
/// array record); false where the full operation decides.
#[inline(always)]
pub(in crate::vm::dispatch_fast) fn staged_require_iterator_result(r: Registers, view: Carried, register: u16) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::new(r, view);
    matches!(crate::staged::step(&mut stage, &mut frame, Operation::RequireIteratorResult { register }), Step::Value(_))
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

