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
        Step::Unresolved => None,
        Step::Control(LaneControl::Resumed(..)) => unreachable!("a jump activates nothing"),
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

/// ToPropertyKey of the direct string in `source` into `destination`, by the staged operation;
/// false, writing nothing, where the value is no direct string.
#[inline(always)]
pub(in crate::vm::dispatch_fast) fn staged_property_key(r: Registers, view: Carried, destination: u16, source: u16) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::new(r, view);
    matches!(crate::staged::step(&mut stage, &mut frame, Operation::PropertyKey { destination, source }), Step::Value(_))
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

/// The call hooks of a call row's frame (`protocol_row`, `LaneCall`): the proved plain call
/// (`script_call_miss`'s proven arm, `lane_plain_activation!`) and the proved leaf call
/// (`script_call_leaf_miss`) as the staged call protocol's rows (`staged::invoke`), in its order:
///
/// - `call_entry`: the site's entry, the link masked to the cache (no bounds test), tested whole by
///   one admission compare (link-proofs.md: generation, program, the run's conditions, `DIAGNOSED`,
///   `UNPROVEN`), the argument count against its parameters, and a leaf row's form; the callee's
///   identity is the plan's guard (`Identity::Guarded`), the one compare `invoke` makes of it;
/// - `this`: the plan binds as `lane_direct_call_this` does: an arrow's lexical `this` (and a plain
///   receiver's, `PLAIN`) as the plan's bound `this`, a sloppy callee's through `bind_this` with the
///   realm's global object, any other the receiver as it is;
/// - `call_room`: the one `plain_room` compare, at the plain depth it loads once;
/// - `call_activate`: the window filled and the plain header pushed (`lane_enter_plain_window`,
///   `protocol::push_plain_at`) or the leaf entered (`lane_run_leaf`), handed back as
///   `LaneControl::Resumed`.
///
/// Anything else (another form, a returning call, a failed compare or binding, no room) answers
/// `CallEntry::Helper` or leaves at the guard before any word is written, so the row is
/// `Step::Unresolved` and its miss arm (`protocol_miss`) runs the full handler.
impl<const ACC: u8> crate::staged::CallFrame<Concrete> for LaneFrame<ACC> {
    #[inline(always)]
    fn call_entry(&self, site: &crate::staged::Invocation) -> crate::staged::CallEntry {
        use crate::staged::{Arguments, Callee, Receiver};
        let Some(call) = &self.call else { return crate::staged::CallEntry::Helper };
        // The form the proved calls make: a callee and a receiver register and a window, a
        // result defined, not returned (a returning call's admission is the run's,
        // `lane_returning_call_admitted`, which the miss arm makes).
        let (Callee::Register(_), Receiver::Register(_), Arguments::Window { count, .. }) = (site.callee, site.receiver, site.arguments) else {
            return crate::staged::CallEntry::Helper;
        };
        if site.returns || site.forwards {
            return crate::staged::CallEntry::Helper;
        }
        // SAFETY: the running lane's stack, which nothing below changes.
        let stack = unsafe { &*call.stack };
        let entries = &stack.script_targets.entries;
        // SAFETY: the call record the row runs.
        let index = usize::from(unsafe { (*call.record).call_link.get() }) & (entries.len() - 1);
        // SAFETY: the cache's length is a power of two (`SCRIPT_CALL_TARGETS_INITIAL`), so the masked
        // index lies within it; nothing before the hand-off resolves a target or collects.
        let target = unsafe { entries.get_unchecked(index) };
        // A static count is the row's (`call_form`), so its tests fold as `script_call_miss`'s `ARGC`.
        let argc = if call.form.argc == ANY_ARGC { count } else { call.form.argc };
        let leaf = call.form.leaf;
        if target.admission != stack.admission
            || (leaf == NOT_LEAF && argc > FIXED_ARGUMENT_COPIES)
            || (leaf != NOT_LEAF && target.entry != CallEntry::leaf(leaf))
            || !crate::staged::call::concrete_fits(u64::from(target.params), u64::from(argc))
        {
            return crate::staged::CallEntry::Helper;
        }
        debug_assert!(leaf != NOT_LEAF || target.entry.activates(), "a proved plain call's entry activates");
        call.target.set(std::ptr::from_ref(target));
        let lexical = || Some(crate::staged::plan::Bound { this: target.lexical_this.to_bits(),
            arguments: crate::staged::plan::WordList { start: 0, count: 0 } });
        let (sloppy, bound) = if call.form.plain {
            // A plain call's receiver is `undefined` by its record, whose `this` is its target's.
            (None, lexical())
        } else {
            match target.this_binding {
                ThisBinding::Lexical => (None, lexical()),
                // SAFETY: the realm's global object, read only for a sloppy callee.
                ThisBinding::Sloppy => (Some(unsafe { *call.globals }.to_bits()), None),
                _ => (None, None),
            }
        };
        crate::staged::CallEntry::Direct(crate::staged::DirectPlan {
            kind: crate::staged::CalleeKind::Script,
            identity: crate::staged::Identity::Guarded(target.callee.to_bits()),
            sloppy,
            registers: target.registers,
            returned: 0,
            bound,
        })
    }

    #[inline(always)]
    fn call_registers(&mut self, _stage: &mut Concrete) -> Option<u64> {
        self.call.as_ref()?;
        Some(self.registers.0.expose_provenance() as u64)
    }

    /// A method site's list (`Arguments::List`, `CallMethod`/`CallThisMethod` only): no lane
    /// handler runs those records (`other_handler` names neither), so no call row reaches here. A
    /// method call `o.m(x)` is a register `Call` whose receiver is `c`, which `call_entry` admits.
    #[inline(always)]
    fn call_list(&self, _list: crate::staged::RegisterList) -> Option<&[Register]> {
        None
    }

    /// The constants of the plan `call_entry` made: only the empty list of its lexical `this`
    /// (`Bound`), which prefixes no argument.
    #[inline(always)]
    fn call_constants(&self, list: crate::staged::plan::WordList) -> Option<&[u64]> {
        (self.call.is_some() && list.count == 0).then_some(&[][..])
    }

    /// A method site's load (`Callee::Method`): as `call_list`, no call row runs a method site.
    #[inline(always)]
    fn call_method(&mut self, _stage: &mut Concrete, _receiver: u64, _atom: u32, _site: u32, _miss: ()) -> Option<u64> {
        None
    }

    /// A construction's receiver: no call row runs a construction (`Construct` takes
    /// `lane_script_construct`, which requickens to the planned constructions; `CONSTRUCT` has no
    /// protocol row), and `call_entry` answers `Helper` for `Receiver::Allocated`.
    #[inline(always)]
    fn call_receiver(&mut self, _stage: &mut Concrete, _constructor: u64) -> Option<u64> {
        None
    }

    #[inline(always)]
    fn call_room(&mut self, _stage: &mut Concrete, _miss: ()) -> Option<crate::staged::Room<u64>> {
        let call = self.call.as_mut()?;
        // SAFETY: the running lane's stack and the entry `call_entry` admitted.
        let (stack, target) = unsafe { (&*call.stack, &*call.target.get()) };
        // The activation count and the values' room in one compare, the depth loaded once for the
        // push past it (`LaneStack::plain_room`); a leaf pushes none, so the bound stands for it too.
        let depth = stack.plain.depth.get();
        if !stack.plain_room(depth, target.room) {
            return None;
        }
        call.depth = depth;
        let top = stack.values.top().expose_provenance() as u64;
        Some(crate::staged::Room { cell: 0, top, next: top, shadow: 0 })
    }

    #[inline(always)]
    fn call_activate(&mut self, _stage: &mut Concrete, this: u64, _room: crate::staged::Room<u64>) -> Option<LaneControl> {
        let call = self.call.as_ref()?;
        // SAFETY: the hand-off's contract (`CallActivation`): the context, record and registers are
        // the running call's, and the entry, its room at `depth` and `this` what the hooks before
        // admitted and bound.
        let (registers, start) = unsafe {
            (call.activate)(call.context, self.registers, call.ip, call.record, call.target.get(), call.depth, Value::from_bits(this))
        };
        Some(LaneControl::Resumed(registers, start))
    }
}
