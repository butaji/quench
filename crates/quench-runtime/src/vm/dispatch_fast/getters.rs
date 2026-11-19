//! Reads that meet an accessor: answered without an activation where the getter's call runs
//! no guest code.
use super::*;
use crate::vm::AccessorForward;
use crate::vm::accessor_cache::GetterEntryRead;

/// A read whose site caches an accessor (`Vm::cached_field_getter`) whose getter is a pure
/// native read (`Vm::pure_native_getter_value`) or a frameless read target, answered as the
/// getter's call would return (`lane_call_read`): `[[Get]]` calls it on the base with no
/// arguments, and neither runs guest code. `Some(None)` is a cached getter that must be
/// called (`lane_getter_activation`); `None`, a site with no accessor entry for `object`.
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(never)]
pub(super) unsafe fn lane_getter_read<H: Host>(
    cx: *mut LaneContext<H>,
    object: Value,
    atom: crate::bytecode::Atom,
    site: u16,
) -> Option<Option<Value>> {
    let vm = unsafe { vm(cx) };
    let index = vm.field_cache_index(site);
    // One walk of the site's entries: the forward, or the getter to call
    // (`Vm::getter_entry_read`); the fact table only where no entry proves `object`.
    let getter = match vm.getter_entry_read(object, index) {
        GetterEntryRead::Answer(value, _) => return Some(Some(value)),
        GetterEntryRead::Call(getter) => getter,
        GetterEntryRead::Miss => vm.cached_field_getter(object, atom, index)?,
    };
    let Some(value) = (unsafe { lane_getter_answer(cx, getter, object) }) else {
        return Some(None);
    };
    lane_memoize_getter(cx, getter, object, index);
    Some(Some(value))
}

/// Memoizes at the accessor entry of cache index `site` that proves `object` what `getter`,
/// whose call on `object` a read just answered, returns for receivers of its layout
/// (`Vm::memoize_getter_answer`, `getter_forward`).
#[inline(never)]
pub(super) fn lane_memoize_getter<H: Host>(cx: *mut LaneContext<H>, getter: Value, object: Value, site: usize) {
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
unsafe fn getter_forward<H: Host>(vm: &Vm<H>, target: &ScriptCallTarget, object: Value) -> Option<AccessorForward> {
    if let Some(value) = unsafe { constant_body_value::<H>(target) } {
        return (!value.is_heap()).then_some(AccessorForward::Constant(value));
    }
    if lane_direct_call_this(vm, target, object) != Some(object) {
        return None;
    }
    match target.entry {
        CallEntry::ReadsThis => Some(AccessorForward::Receiver),
        // SAFETY: a `Reads` target of the running program holds its returning record first.
        CallEntry::Reads => match unsafe { LeafRead::of::<H>(target) }? {
            LeafRead::Field { base, kind: RETURNING_FIELD_OWN | RETURNING_FIELD_HOLDER, cache } if base == crate::bytecode::FieldBase::THIS.0 => {
                Some(AccessorForward::Field(vm.field_cache_entry(cache)))
            }
            LeafRead::Private { base, site, environment, .. } if base == crate::bytecode::FieldBase::THIS.0 => {
                let answer = vm.site_private_field_entry(site, environment)?;
                let receiver = vm.heap.object_record(object)?;
                (answer.receiver == receiver.exact_shape()).then_some(AccessorForward::Field(answer))
            }
            _ => None,
        },
        _ => None,
    }
}

/// The constant the body of `target` returns where it is `LoadConst` and `Return` of it
/// (`specialize_constant_returns`'s `lane_return_constant`, a plain leaf's form included):
/// a call of it is that constant, whatever its receiver and arguments.
///
/// # Safety
/// `target` is a cached target of the running program.
#[inline(always)]
unsafe fn constant_body_value<H: Host>(target: &ScriptCallTarget) -> Option<Value> {
    if !target.entry.activates() {
        return None;
    }
    // SAFETY: the program store keeps a derived view alive with its program; its first
    // record is the body's first instruction.
    let first = unsafe { &*(*target.view).records() };
    let constant = first.handler == lane_return_constant::<H, NOT_LEAF> as Handler as *const ()
        || first.handler == lane_return_constant::<H, PLAIN_LEAF> as Handler as *const ();
    constant.then(|| first.constant())
}

/// What `getter` returns for a read of `object` where its call runs no guest code: a pure
/// native read, or a frameless read target (`lane_getter_read`).
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(never)]
unsafe fn lane_getter_answer<H: Host>(cx: *mut LaneContext<H>, getter: Value, object: Value) -> Option<Value> {
    if let Some(value) = (unsafe { vm(cx) }).pure_native_getter_value(getter, object) {
        return Some(value);
    }
    let target = lane_script_target(cx, getter)?;
    unsafe { lane_script_getter_answer(cx, &target, object) }
}

/// What the frameless script getter `target` returns for a read of `object` where a read
/// answers its call (`CallEntry::answered_by_read`); `None` for any other getter.
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
unsafe fn lane_script_getter_answer<H: Host>(cx: *mut LaneContext<H>, target: &ScriptCallTarget, object: Value) -> Option<Value> {
    if target.entry == CallEntry::Reads {
        // A getter only the general path has called has not run its read in the lane yet: it
        // takes the kind its first run would.
        // SAFETY: as `lane_call_read`; the context's program outlives the lane run.
        let (entry, code) = unsafe { ((*target.view).records(), &*(*cx).code) };
        if unsafe { (*entry).handler } == lane_returning_field_first::<H, NOT_LEAF> as *const () {
            requicken::<H>(entry, returning_field_first_kind::<H, NOT_LEAF>(unsafe { vm(cx) }, code, unsafe { &*entry }));
        }
    } else if let Some(value) = unsafe { constant_body_value::<H>(target) } {
        // `return k`: no receiver to bind, nothing to read.
        return Some(value);
    } else if !target.entry.answered_by_read() {
        return None;
    }
    let this = lane_direct_call_this(unsafe { vm(cx) }, target, object)?;
    unsafe { lane_call_read(cx, target, ReadArguments::NONE, this) }
}

/// A lane read of `base` at the record `i` (at `ip`) that met `getter`: its value where a read
/// answers the getter's call (`lane_getter_answer`), else the getter's activation
/// (`lane_activate_getter`), whose completion is the read's value. Only a read that writes its
/// register is answered in place: a returning read (`RETURN_REGISTER`) completes its
/// activation, which only the getter's activation does. A native getter is called in place
/// (`lane_call_native`); `Err` after such a call that raised leaves the error in
/// `Vm::lane_raised`. The answer's read may have made the activations frames, so the
/// registers are the context's again.
pub(super) fn lane_read_getter<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    getter: Value,
    base: Value,
) -> Result<(Registers, Ip), Ip> {
    let reads = i.a & crate::bytecode::RETURN_REGISTER == 0;
    // A native getter is answered or called in place; a script getter's target, resolved
    // once, is answered by its read or activated.
    if let Some(FunctionRecord { kind: FunctionKind::Native(native), .. }) = unsafe { vm(cx) }.heap.function(getter) {
        let native = *native;
        if reads {
            if let Some(value) = unsafe { vm(cx) }.pure_native_getter_value(getter, base) {
                let r = unsafe { (*cx).registers };
                r.set(i.a, value);
                return Ok((r, following(ip)));
            }
            return lane_native_getter(cx, ip, i, native, getter, base);
        }
        return lane_activate_getter(cx, r, ip, getter, base, None);
    }
    let target = lane_script_target(cx, getter);
    if reads
        && let Some(target) = &target
        && let Some(value) = unsafe { lane_script_getter_answer(cx, target, base) }
    {
        if i.b != crate::bytecode::FieldBase::NESTED {
            let site = unsafe { vm(cx) }.field_cache_index(i.c);
            lane_memoize_getter(cx, getter, base, site);
        }
        let r = unsafe { (*cx).registers };
        r.set(i.a, value);
        return Ok((r, following(ip)));
    }
    lane_activate_getter(cx, r, ip, getter, base, target)
}

/// What a getter site's `GetField` record does after one walk of its site's accessor entries
/// (`lane_getter_step`).
pub(super) enum GetterStep {
    /// A trivial getter's forward (`Vm::getter_entry_read`): the read's value, no call.
    Answer(Value),
    /// The getter's read or call (`lane_read_getter`), found without the fact table's probe
    /// and the second walk `lane_getter_activation` makes.
    Read(Result<(Registers, Ip), Ip>),
    /// No entry proves the base: `lane_getter_activation`'s lookups decide.
    Miss,
}

/// `lane_get_field_getter`'s step for a `GetField` record `i` (at `ip`) of a register or
/// `this`: the entry of the base's layout gives the forward, which only a read that writes its
/// register takes in place (a returning read completes its activation, which only the call
/// does), or the getter, which a lane allowed script calls reads or calls as
/// `lane_getter_activation` would. A nested read, and any base no entry proves, is `Miss`.
#[inline(always)]
pub(super) fn lane_getter_step<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, i: &LaneInstruction) -> GetterStep {
    if i.b == crate::bytecode::FieldBase::NESTED {
        return GetterStep::Miss;
    }
    // SAFETY: the running lane's context and registers.
    let Some(base) = (unsafe { lane_field_base(cx, r, crate::bytecode::FieldBase(i.b)) }) else {
        return GetterStep::Miss;
    };
    let vm = unsafe { vm(cx) };
    let reads = i.a & crate::bytecode::RETURN_REGISTER == 0;
    match vm.getter_entry_read(base, vm.field_cache_index(i.c)) {
        GetterEntryRead::Answer(value, _) if reads => GetterStep::Answer(value),
        // `lane_getter_activation` declines a lane without script calls before any lookup.
        GetterEntryRead::Answer(_, getter) | GetterEntryRead::Call(getter) if unsafe { (*cx).script_calls } => {
            GetterStep::Read(lane_read_getter(cx, r, ip, i, getter, base))
        }
        _ => GetterStep::Miss,
    }
}

/// The getter call `lane_activate_kept` makes for `target`, of the running program, with
/// receiver `this` and no arguments, as the plain call it is (getters-as-calls.md): the
/// header goes below the window straight from the read's words (`lane_enter_plain_window`),
/// with no `Activation` value, no `LaneStack::header_flags` test and no keeping-record words. Its
/// completion is `Plain`: the return writes the read record's register (`write_call_result`
/// of the call record's `a`, which a `GetField` names as its destination). `None`, which
/// changes nothing, for any call that is not such a plain call: a returning read (whose
/// admission the kept path tests), a `with` scope, an entry that does not activate its
/// registers alone (keeping, environment, settling), another program's target, or no room
/// by the plain compare; `lane_activate_kept` takes those.
#[inline(always)]
pub(super) fn lane_push_getter<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    this: Value,
) -> Option<(Registers, Ip)> {
    let vm = unsafe { vm(cx) };
    // SAFETY: `ip` is a record of the running view.
    let returns = unsafe { (*ip).a } & crate::bytecode::RETURN_REGISTER != 0;
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
    debug_assert_eq!(vm.lane_stack.plain_room(depth, target.room), lane_activation_room(vm, target));
    if !vm.lane_stack.plain_room(depth, target.room) {
        return None;
    }
    // No values passed: the fill leaves every parameter `undefined`, as `lane_push_passed`'s
    // of an empty slice does (a dangling, aligned pointer, never read for a count of 0).
    let none: &[Value] = &[];
    let callee_registers = lane_fill_window(vm, target, none.as_ptr(), 0);
    let entered = lane_enter_plain_window(cx, vm, r, ip, target, callee_registers, depth, 0, this);
    // `lane_enter_window_of`'s `COUNTED`: a getter's call is no proved call, so the tier's
    // diagnostics still count its entry.
    vm.programs.record_js_jit_lane_entry(target.program(), target.function);
    Some(entered)
}

/// `lane_read_getter` of a native getter: called as a lane `Call` of it with no arguments is.
/// Out of line, so script getters' activations keep their code.
#[cold]
#[inline(never)]
fn lane_native_getter<H: Host>(
    cx: *mut LaneContext<H>,
    ip: Ip,
    i: &LaneInstruction,
    native: Native,
    getter: Value,
    base: Value,
) -> Result<(Registers, Ip), Ip> {
    match lane_native_complete(cx, ip, i, lane_call_native(cx, ip, native, getter, base, &[])) {
        Resumed::At(r, next, _) => Ok((r, next)),
        Resumed::Exit(next) => Err(next),
    }
}

script_handler! {
    /// `lane_get_field_getter` at a site whose getter returns an own field of the receiver
    /// (`lane_hold_getter_hint`): the record's hint (`Vm::holder_hint_hit`), whose shape, link and
    /// watch tests stand for the getter's call and its read. Any other read takes
    /// `lane_get_field_getter_refresh`.
    pub(super) fn lane_get_field_getter_hinted<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let value = unsafe { lane_field_base(cx, r, crate::bytecode::FieldBase(i.b)) }
            .and_then(|base| vm.getter_hint_hit(i.call_link.get(), i.imm(), base));
        let Some(value) = value else {
            sibling!(lane_get_field_getter_refresh::<H>, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_get_field_getter_hinted` past its hint: the read the getter's answer makes
    /// (`lane_getter_answer`), whose getter's own field becomes the hint
    /// (`lane_hold_getter_hint`). A getter the answer declines takes `lane_get_field_getter`.
    #[inline(never)]
    fn lane_get_field_getter_refresh<>(cx, r, ip, i, view) {
        let base = unsafe { lane_field_base(cx, r, crate::bytecode::FieldBase(i.b)) };
        let Some(value) = base.and_then(|base| unsafe { lane_hold_getter_hint(cx, ip, i, base) }) else {
            requicken::<H>(ip, lane_get_field_getter::<H>);
            sibling!(lane_get_field_getter::<H>, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// What the getter the site cached answers for `base` (`lane_getter_answer`), made the record's
/// hint where the getter returns an own field of its receiver; any other getter moves the record
/// to `lane_get_field_getter` for good.
///
/// # Safety
/// `cx` is the running lane's context and `ip` the `GetField` record `i`.
unsafe fn lane_hold_getter_hint<H: Host>(cx: *mut LaneContext<H>, ip: Ip, i: &LaneInstruction, base: Value) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    let site = vm.field_cache_index(i.c);
    let getter = vm.cached_field_getter(base, i.imm(), site)?;
    let value = unsafe { lane_getter_answer(cx, getter, base) }?;
    let slot = lane_getter_field_slot(cx, getter, base)
        .filter(|_| !vm.is_private_name(i.imm()) && vm.cached_accessor_on_prototype(base, i.imm(), site));
    if !slot.is_some_and(|slot| set_getter_hint(vm, ip, i.imm(), base, slot)) {
        requicken::<H>(ip, lane_get_field_getter::<H>);
    }
    Some(value)
}

/// The `FieldOperand::shape` of a record whose hint is a getter's own-field return
/// (`Vm::getter_hint_hit`), as `HOLDER_HINT` no object's exact shape.
const GETTER_HINT: u32 = super::super::NO_FIELD_RECEIVER - 3;

/// Makes the own field `slot` of `base`, which the record's getter returns, the hint of the
/// `GetField` record at `ip`, in place of the one it holds; whether the record holds it.
fn set_getter_hint<H: Host>(vm: &mut Vm<H>, ip: Ip, atom: Atom, base: Value, slot: u16) -> bool {
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

/// The own slot of `base` that `getter`, a frameless read body, returns (`return this.field`),
/// by the field cache entry its record answers with.
fn lane_getter_field_slot<H: Host>(cx: *mut LaneContext<H>, getter: Value, base: Value) -> Option<u16> {
    let target = lane_script_target(cx, getter).filter(|target| target.entry == CallEntry::Reads)?;
    let vm = unsafe { vm(cx) };
    (lane_direct_call_this(vm, &target, base)? == base).then_some(())?;
    // SAFETY: a `CallEntry::Reads` target of the running program.
    let LeafRead::Field { base: field_base, kind: RETURNING_FIELD_OWN, cache } = unsafe { LeafRead::of::<H>(&target) }? else {
        return None;
    };
    let entry = vm.field_cache_entry(cache);
    let receiver = vm.heap.object_record_by_id(base.direct_object_index()?);
    (crate::bytecode::FieldBase(field_base) == crate::bytecode::FieldBase::THIS
        && entry.holder == NO_FIELD_HOLDER
        && entry.receiver == receiver.exact_shape())
    .then_some(entry.slot)
}
