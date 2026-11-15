//! Reads that meet an accessor: answered without an activation where the getter's call runs
//! no guest code.
use super::*;

/// A read whose site caches an accessor (`Vm::cached_field_getter`) whose getter is a pure
/// native read (`Vm::pure_native_getter_value`) or a frameless read target, answered as the
/// getter's call would return (`lane_call_read`): `[[Get]]` calls it on the base with no
/// arguments, and neither runs guest code. `None` leaves the read to the general path, which
/// calls the getter.
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(never)]
pub(super) unsafe fn lane_getter_read<H: Host>(
    cx: *mut LaneContext<H>,
    object: Value,
    atom: crate::bytecode::Atom,
    site: u16,
) -> Option<Value> {
    let getter = {
        let vm = unsafe { vm(cx) };
        vm.cached_field_getter(object, atom, vm.field_cache_index(site))?
    };
    unsafe { lane_getter_answer(cx, getter, object) }
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
    if target.entry == CallEntry::Reads {
        // A getter only the general path has called has not run its read in the lane yet: it
        // takes the kind its first run would.
        // SAFETY: as `lane_call_read`; the context's program outlives the lane run.
        let (entry, code) = unsafe { ((*target.view).records(), &*(*cx).code) };
        if unsafe { (*entry).handler } == lane_returning_field_first::<H, NOT_LEAF> as *const () {
            requicken::<H>(entry, returning_field_first_kind::<H, NOT_LEAF>(unsafe { vm(cx) }, code, unsafe { &*entry }));
        }
    } else if !target.entry.answered_by_read() {
        return None;
    }
    let this = lane_direct_call_this(unsafe { vm(cx) }, &target, object)?;
    unsafe { lane_call_read(cx, &target, ReadArguments::NONE, this) }
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
    if i.a & crate::bytecode::RETURN_REGISTER == 0 {
        if let Some(value) = unsafe { lane_getter_answer(cx, getter, base) } {
            let r = unsafe { (*cx).registers };
            r.set(i.a, value);
            return Ok((r, following(ip)));
        }
        if let Some(FunctionRecord { kind: FunctionKind::Native(native), .. }) = unsafe { vm(cx) }.heap.function(getter) {
            return lane_native_getter(cx, ip, i, *native, getter, base);
        }
    }
    lane_activate_getter(cx, r, ip, getter, base)
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
