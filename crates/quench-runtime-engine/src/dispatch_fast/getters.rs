//! Reads that meet an accessor: answered without an activation where the getter's call runs
//! no guest code. The answers and activations are `AccessorCall`'s (`accessor_protocol.rs`).
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::vm::AccessorForward;
use quench_runtime::vm::accessor_cache::GetterEntryRead;

/// Memoizes at the accessor entry of cache index `site` that proves `object` what `getter`,
/// whose call on `object` a read just answered, returns for receivers of its layout
/// (`Vm::memoize_getter_answer`, `getter_forward`).
#[inline(never)]
pub(crate) fn lane_memoize_getter(cx: *mut LaneContext, getter: Value, object: Value, site: usize) {
    let vm = unsafe { vm(cx) };
    // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target or collects.
    let Some(target) = vm.script_call_entry(getter).map(|entry| unsafe { *entry }) else {
        return;
    };
    // SAFETY: a cached target of the running program.
    if let Some(answer) = unsafe { getter_forward(vm, &target, object) } {
        vm.memoize_getter_answer(object, site, getter, answer);
    }
}

/// What the getter `target` returns for `object` and every receiver of its layout, as a
/// forward (getters-as-calls.md, trivial accessors), recognized from its body:
/// - `return k` of a primitive constant that is no heap value (`constant_body_value`);
/// - `return this` (`CallEntry::ReadsThis`) of a getter that binds `object` itself;
/// - `return this.f` (`CallEntry::Reads`) whose returning `GetField` of `this` hits an own or
///   holder entry: that entry, which proves the receiver's shape itself;
/// - `return this.#f`: the private field's own slot its site recorded for `object`'s exact
///   shape under the getter's home (`Vm::site_private_field_entry`): the getter's identity
///   fixes its environment and so its home, and the shape fixes where the field is.
///
/// None of these can throw or run guest code for a receiver the forward admits: the data
/// field reads have no getter to call, so the call's absence is not observable.
///
/// # Safety
/// `target` is a cached target of the running program.
unsafe fn getter_forward(vm: &Vm, target: &ScriptCallTarget, object: Value) -> Option<AccessorForward> {
    if let Some(value) = unsafe { constant_body_value(target) } {
        return (!value.is_heap()).then_some(AccessorForward::Constant(value));
    }
    if direct_call_this_in_lane(vm, target, object) != Some(object) {
        return None;
    }
    match target.entry {
        CallEntry::ReadsThis => Some(AccessorForward::Receiver),
        // SAFETY: a `Reads` target of the running program holds its returning record first.
        CallEntry::Reads => match unsafe { LeafRead::of(target) }? {
            LeafRead::Field { base, kind: RETURNING_FIELD_OWN | RETURNING_FIELD_HOLDER, cache } if base == quench_runtime::bytecode::FieldBase::THIS.0 => {
                Some(AccessorForward::Field(vm.field_cache_entry(cache)))
            }
            LeafRead::Private { base, site, environment, .. } if base == quench_runtime::bytecode::FieldBase::THIS.0 => {
                let answer = vm.site_private_field_entry(site, environment)?;
                let receiver = vm.heap_reads().object_record(object)?;
                (answer.receiver == receiver.exact_shape()).then_some(AccessorForward::Field(answer))
            }
            _ => None,
        },
        _ => None,
    }
}

/// The constant the body of `target` returns where it is `LoadConst` and `Return` of it
/// (`specialize_constant_returns`'s `return_constant_miss` row, a plain leaf's form included):
/// a call of it is that constant, whatever its receiver and arguments.
///
/// # Safety
/// `target` is a cached target of the running program.
#[inline(always)]
unsafe fn constant_body_value(target: &ScriptCallTarget) -> Option<Value> {
    if !target.entry.activates() {
        return None;
    }
    // SAFETY: the program store keeps a derived view alive with its program; its first
    // record is the body's first instruction.
    let first = unsafe { &*(*target.view).records() };
    let constant = first.handler == return_row!(quench_runtime::bytecode::RETURN_ROW, returning_code(CONSTANT_KIND, NOT_LEAF)) as Handler as *const ()
        || first.handler == return_row!(quench_runtime::bytecode::RETURN_ROW, returning_code(CONSTANT_KIND, PLAIN_LEAF)) as Handler as *const ();
    constant.then(|| first.constant())
}

/// What the frameless script getter `target` returns for a read of `object` where a read
/// answers its call (`CallEntry::answered_by_read`); `None` for any other getter.
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
unsafe fn lane_script_getter_answer(cx: *mut LaneContext, target: &ScriptCallTarget, object: Value) -> Option<Value> {
    if target.entry == CallEntry::Reads {
        // A getter only the general path has called has not run its read in the lane yet: it
        // takes the kind its first run would.
        // SAFETY: as `call_read_in_lane`; the context's program outlives the lane run.
        let (entry, code) = unsafe { ((*target.view).records(), &*(*cx).code) };
        if unsafe { (*entry).handler } == row_returning_field_first::<NOT_LEAF> as *const () {
            requicken(entry, returning_field_first_kind::<NOT_LEAF>(unsafe { vm(cx) }, code, unsafe { &*entry }));
        }
    } else if let Some(value) = unsafe { constant_body_value(target) } {
        // `return k`: no receiver to bind, nothing to read.
        return Some(value);
    } else if !target.entry.answered_by_read() {
        return None;
    }
    let this = direct_call_this_in_lane(unsafe { vm(cx) }, target, object)?;
    unsafe { call_read_in_lane(cx, target, ReadArguments::NONE, this) }
}

/// A lane read of `base` at the record `i` (at `ip`) that met `getter`: its value where a read
/// answers the getter's call, else the getter's activation
/// (`activate_getter_in_lane`), whose completion is the read's value. Only a read that writes its
/// register is answered in place: a returning read (`RETURN_REGISTER`) completes its
/// activation, which only the getter's activation does. A native getter is called in place
/// (`call_native_in_lane`); `Err` after such a call that raised leaves the error in
/// `Vm::lane_raised`. The answer's read may have made the activations frames, so the
/// registers are the context's again.
pub(crate) fn lane_read_getter(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    getter: Value,
    base: Value,
) -> Result<(Registers, Ip), Ip> {
    let reads = i.a & quench_runtime::bytecode::RETURN_REGISTER == 0;
    // A native getter is answered or called in place; a script getter's target, resolved
    // once, is answered by its read or activated.
    if let Some(FunctionRecord { kind: FunctionKind::Native(native), .. }) = unsafe { vm(cx) }.function_record(getter) {
        let native = *native;
        if reads {
            if let Some(value) = unsafe { vm(cx) }.pure_native_getter_value(getter, base) {
                let r = unsafe { (*cx).registers };
                r.set(i.a, value);
                return Ok((r, following(ip)));
            }
            return lane_native_getter(cx, ip, i, native, getter, base);
        }
        return activate_getter_in_lane(cx, r, ip, getter, base, None);
    }
    let target = script_target_in_lane(cx, getter);
    if reads
        && let Some(target) = &target
        && let Some(value) = unsafe { lane_script_getter_answer(cx, target, base) }
    {
        // A capture read (`() => binding`) has no forward (`getter_forward` declines it after
        // a target probe), so its memo attempt is skipped: no change but the probe's cost.
        if i.b != quench_runtime::bytecode::FieldBase::NESTED && target.entry != CallEntry::ReadsCapture {
            let site = unsafe { vm(cx) }.field_cache_index(i.c);
            lane_memoize_getter(cx, getter, base, site);
        }
        let r = unsafe { (*cx).registers };
        r.set(i.a, value);
        return Ok((r, following(ip)));
    }
    activate_getter_in_lane(cx, r, ip, getter, base, target)
}

/// What a getter site's `GetField` record does after one walk of its site's accessor entries
/// (`lane_getter_step`).
pub(crate) enum GetterStep {
    /// A trivial getter's forward (`Vm::getter_entry_read`): the read's value, no call.
    Answer(Value),
    /// The getter's read or call (`lane_read_getter`), found without the fact table's probe
    /// and the second walk `getter_activation_in_lane` makes.
    Read(Result<(Registers, Ip), Ip>),
    /// No entry proves the base: `getter_activation_in_lane`'s lookups decide.
    Miss,
}

/// `lane_get_field_getter`'s step for a `GetField` record `i` (at `ip`) of a register or
/// `this`: the entry of the base's layout gives the forward, which only a read that writes its
/// register takes in place (a returning read completes its activation, which only the call
/// does), or the getter, which a lane allowed script calls reads or calls as
/// `getter_activation_in_lane` would. A nested read, and any base no entry proves, is `Miss`.
#[inline(always)]
pub(crate) fn lane_getter_step(cx: *mut LaneContext, r: Registers, ip: Ip, i: &LaneInstruction) -> GetterStep {
    if i.b == quench_runtime::bytecode::FieldBase::NESTED {
        return GetterStep::Miss;
    }
    // SAFETY: the running lane's context and registers.
    let Some(base) = (unsafe { field_base_in_lane(cx, r, quench_runtime::bytecode::FieldBase(i.b)) }) else {
        return GetterStep::Miss;
    };
    let vm = unsafe { vm(cx) };
    let reads = i.a & quench_runtime::bytecode::RETURN_REGISTER == 0;
    match vm.getter_entry_read(base, vm.field_cache_index(i.c)) {
        GetterEntryRead::Answer(value, _) if reads => GetterStep::Answer(value),
        // `getter_activation_in_lane` declines a lane without script calls before any lookup.
        GetterEntryRead::Answer(_, getter) | GetterEntryRead::Call(getter) if unsafe { (*cx).script_calls } => {
            GetterStep::Read(lane_read_getter(cx, r, ip, i, getter, base))
        }
        _ => GetterStep::Miss,
    }
}

/// The getter call `activate_kept_in_lane` makes for `target`, of the running program, with
/// receiver `this` and no arguments, as the plain call it is (getters-as-calls.md): the
/// header goes below the window straight from the read's words (`enter_plain_window`),
/// with no `Activation` value, no `LaneStack::header_flags` test and no keeping-record words. Its
/// completion is `Plain`: the return writes the read record's register (`write_call_result`
/// of the call record's `a`, which a `GetField` names as its destination). `None`, which
/// changes nothing, for any call that is not such a plain call: a returning read (whose
/// admission the kept path tests), a `with` scope, an entry that does not activate its
/// registers alone (keeping, environment, settling), another program's target, or no room
/// by the plain compare; `activate_kept_in_lane` takes those. A leaf getter that binds its
/// receiver runs as a leaf instead, with no header (`lane_run_leaf_getter`).
#[inline(always)]
pub(crate) fn lane_push_getter(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    this: Value,
) -> Option<(Registers, Ip)> {
    let vm = unsafe { vm(cx) };
    // SAFETY: `ip` is a record of the running view.
    let returns = unsafe { (*ip).a } & quench_runtime::bytecode::RETURN_REGISTER != 0;
    if returns
        || vm.with_scope_entered
        || !target.entry.activates()
        || target.forwards()
        || target.settles()
        || target.program() != unsafe { (*cx).program }
    {
        return None;
    }
    // The plain depth the push writes past, loaded once; at depth 0 the room test refreshes
    // the bound for the pushes above (`LaneStack::plain_room`).
    let depth = vm.lane_stack.plain.depth.get();
    debug_assert_eq!(vm.lane_stack.plain_room(depth, target.room), activation_room_in_lane(vm, target));
    if !vm.lane_stack.plain_room(depth, target.room) {
        return None;
    }
    // A leaf getter (`get isGP() { return this._type == GP }`) runs as the plain call of a leaf
    // runs it: on a window at the stack's top with no header (`lane_run_leaf_getter`).
    if leaf_binds_this(target.entry.leaf_form())
        && let Some(entered) = unsafe { lane_run_leaf_getter(cx, r, ip, target, this) }
    {
        vm.programs.record_js_jit_lane_entry(target.program(), target.function);
        return Some(entered);
    }
    // No values passed: the fill leaves every parameter `undefined`, as `push_passed_in_lane`'s
    // of an empty slice does (a dangling, aligned pointer, never read for a count of 0).
    let none: &[Value] = &[];
    let callee_registers = fill_window_in_lane(vm, target, none.as_ptr(), 0);
    let entered = enter_plain_window(cx, vm, r, ip, target, callee_registers, depth, 0, this);
    // `enter_window_of_in_lane`'s `COUNTED`: a getter's call is no proved call, so the tier's
    // diagnostics still count its entry.
    vm.programs.record_js_jit_lane_entry(target.program(), target.function);
    Some(entered)
}

/// `lane_push_getter`'s call of a leaf getter `target` that binds its receiver (`THIS_LEAF`,
/// `CONTEXT_LEAF`), as `script_call_leaf_miss` makes a leaf call (`lane_run_leaf`) with no
/// values: the window at the lane stack's top filled, the leaf record entered with the read
/// record at `ip` as its call. The leaf's return (`lane_return_leaf_header`,
/// `complete_leaf_in_lane` → `lane_resume_leaf_caller`) writes the call record's `a`, which a
/// `GetField` names as its destination, and resumes at `following(ip)`: what the plain
/// header's return does (`write_call_result`). A materialization (`materialize_leaf_in_lane`)
/// pushes the activation `lane_push_getter`'s plain push would have: the read record as its
/// call, `Completion::Plain`, no values. Only receiver-binding forms are taken: the others'
/// materialization rebinds `this` from the call record's `c`, which a `GetField` holds its site
/// in, not a register. `None`, which changes nothing, where the cache does not hold the
/// target's entry in place (the leaf record names its cache entry, which must stay put while
/// the leaf runs, so a resolved copy is not enough).
///
/// # Safety
/// `cx` is the running lane's context, with no leaf running; `target` passed
/// `lane_push_getter`'s plain tests (running program, no `with` scope, room by `plain_room`);
/// `this` is its binding of the read's base (`call_this_in_lane`).
#[inline(always)]
unsafe fn lane_run_leaf_getter(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    this: Value,
) -> Option<(Registers, Ip)> {
    let vm = unsafe { vm(cx) };
    debug_assert!(unsafe { (*cx).leaf.call }.is_null(), "a getter's read materializes a running leaf first");
    let form = target.entry.leaf_form();
    debug_assert!(leaf_binds_this(form));
    // SAFETY: as `Vm::script_call_entry`; read in place before anything touches the cache, and
    // nothing in the leaf resolves a target or collects.
    let entry = unsafe { &*vm.script_call_entry(target.callee)? };
    if entry.entry != target.entry || entry.function != target.function || !std::ptr::eq(entry.view, target.view) {
        return None;
    }
    // SAFETY: as `lane_run_leaf`'s window: `plain_room` proved the admitted room, which the
    // stack's length covers only once the leaf is materialized; no collection runs before.
    let registers = unsafe {
        let registers = vm.lane_stack.values.top();
        fill_admitted_window(registers, usize::from(entry.fill), usize::from(entry.registers));
        Registers(registers)
    };
    // SAFETY: the program store keeps a derived view alive with its program; the tier claims
    // no leaf's view, so its run starts at its first record.
    let base = unsafe { (*entry.view).records() };
    // SAFETY: the running lane's context; the window is the callee's, and `ip`, the read
    // record, belongs to the caller's live view.
    unsafe {
        lane_enter_leaf(cx, registers, ip, r, entry, form, 0, this, None);
        (*cx).registers = registers;
        (*cx).base = base;
    }
    Some((registers, base))
}

/// `lane_read_getter` of a native getter: called as a lane `Call` of it with no arguments is.
/// Out of line, so script getters' activations keep their code.
#[cold]
#[inline(never)]
fn lane_native_getter(
    cx: *mut LaneContext,
    ip: Ip,
    i: &LaneInstruction,
    native: Native,
    getter: Value,
    base: Value,
) -> Result<(Registers, Ip), Ip> {
    match native_complete_in_lane(cx, ip, i, call_native_in_lane(cx, ip, native, getter, base, &[])) {
        Resumed::At(r, next, _) => Ok((r, next)),
        Resumed::Exit(next) => Err(next),
    }
}

script_handler! {
    /// The generated getter read (`GETTER_THIS`, `GETTER_REGISTER`) past its hint: the read the
    /// getter's answer makes (`AccessorCall::getter_answer`), whose getter's own field becomes the
    /// hint (`AccessorCall::hold_getter_hint`). A getter the answer declines takes `get_field_getter_miss`.
    #[inline(never)]
    pub(crate) fn row_get_field_getter_refresh<>(cx, r, ip, i, view) {
        let base = unsafe { field_base_in_lane(cx, r, quench_runtime::bytecode::FieldBase(i.b)) };
        let Some(value) = base.and_then(|base| unsafe { AccessorCall::of(cx).hold_getter_hint(ip, i, base) }) else {
            requicken(ip, protocol_handler::<{ quench_runtime::bytecode::FIELD_READ as u16 }, ACC_NONE, { protocol_family::ELEMENT | protocol_misses::element::GET_FIELD_GETTER as u16 }>());
            sibling!(get_field_getter_miss, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// The `FieldOperand::shape` of a record whose hint is a getter's own-field return
/// (`Vm::getter_hint_operands`), as `HOLDER_HINT` no object's exact shape.
const GETTER_HINT: u32 = quench_runtime::vm::NO_FIELD_RECEIVER - 3;

/// Makes the own field `slot` of `base`, which the record's getter returns, the hint of the
/// `GetField` record at `ip`, in place of the one it holds; whether the record holds it.
pub(crate) fn set_getter_hint(vm: &mut Vm, ip: Ip, atom: Atom, base: Value, slot: u16) -> bool {
    if holds_loop_site(ip) {
        return false;
    }
    // SAFETY: `ip` is a `GetField` record of a live view.
    let i = unsafe { &*ip };
    let owned = (unsafe { i.operand.field.shape } == GETTER_HINT).then(|| i.call_link.get());
    let Some(index) = vm.hold_getter_hint(owned, atom, base, slot, NO_CALL_LINK) else {
        return false;
    };
    set_record_hint(ip, GETTER_HINT, index);
    true
}
