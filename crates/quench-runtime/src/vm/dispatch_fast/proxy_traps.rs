//! Reads of a proxy whose `get` trap runs as an activation of the lane.
use super::*;
use crate::vm::object_get::ordinary_chain::ChainRead;

/// The values a `get` trap is called with: the target, the key and the receiver.
const GET_TRAP_ARGUMENTS: usize = 3;

/// The values a `set` trap is called with: the target, the key, the value and the receiver.
const SET_TRAP_ARGUMENTS: usize = 4;

/// What a read of `atom` of the proxy `object` calls where the trap's call is the whole
/// read: the `get` trap, a plain data property of an ordinary handler up its chain (so
/// finding it runs no guest code), the handler it is called on, and the values it is called
/// with. The target's shape leaves the key unconstrained (`Vm::shape_leaves_atom_unconstrained`),
/// so no invariant checks the trap's result. `None` for any other read: no proxy, a revoked
/// one, a private name, no trap, or a target that may constrain the key.
fn get_trap_call<H: Host>(vm: &mut Vm<H>, object: Value, atom: crate::bytecode::Atom) -> Option<(Value, Value, [Value; GET_TRAP_ARGUMENTS])> {
    let (target, handler) = vm.proxy_parts(object)?;
    if handler.is_null() || vm.is_private_name(atom) {
        return None;
    }
    let trap = vm.plain_property_lookup(handler, crate::vm::PropertyKey::string(vm.runtime_atoms.get))??;
    vm.shape_leaves_atom_unconstrained(target, atom).then_some((trap, handler, [target, vm.atom_string(atom), object]))
}

/// A lane read of `atom` from the proxy `base` at the record `ip` whose handler has no `get`
/// trap: the read of its target (through further trapless proxies) along the ordinary chain
/// (`Vm::ordinary_chain_read`), a data value or an absent key completing the record in place
/// and a getter running as an activation with the proxy as receiver. `Err` carries the
/// record to continue at: the general path for any other read, a returning or nested record
/// among them.
fn lane_read_forwarded<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, base: Value, atom: crate::bytecode::Atom) -> Result<(Registers, Ip), Ip> {
    let vm = unsafe { vm(cx) };
    // SAFETY: `ip` is a record of the running view.
    let i = unsafe { &*ip };
    if i.a & crate::bytecode::RETURN_REGISTER != 0 || i.b == crate::bytecode::FieldBase::NESTED || vm.is_private_name(atom) {
        return Err(exit(ip));
    }
    let mut target = base;
    while let Some((inner, handler)) = vm.proxy_parts(target) {
        if !vm.handler_lacks_trap(handler, ProxyTrap::Get) {
            return Err(exit(ip));
        }
        target = inner;
    }
    match vm.ordinary_chain_read(target, atom) {
        Some(ChainRead::Value(value)) => lane_complete_read(r, ip, i, value),
        Some(ChainRead::Absent) => lane_complete_read(r, ip, i, Value::UNDEFINED),
        Some(ChainRead::Getter(Some(getter))) if !getter.is_undefined() => lane_activate_getter(cx, r, ip, getter, base),
        _ => Err(exit(ip)),
    }
}

/// The read record `i` at `ip` completed in place with `value`.
fn lane_complete_read(r: Registers, ip: Ip, i: &LaneInstruction, value: Value) -> Result<(Registers, Ip), Ip> {
    r.set(i.a, value);
    Ok((r, following(ip)))
}

/// A lane read at the record `ip` of `read`'s base and name that no cached getter answers:
/// a proxy's `get` trap of the running program activated with the handler as its receiver
/// (`get_trap_call`), whose completion is the read's value. `Err` carries the record to
/// continue at: the general path for any other read, or for an activation the lane has no
/// room for.
pub(super) fn lane_read_get_trap<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    read: Option<(Value, crate::bytecode::Atom)>,
) -> Result<(Registers, Ip), Ip> {
    let vm = unsafe { vm(cx) };
    let Some((base, atom)) = read else {
        return Err(exit(ip));
    };
    let Some((trap, handler, arguments)) = get_trap_call(vm, base, atom) else {
        return lane_read_forwarded(cx, r, ip, base, atom);
    };
    lane_materialize_running_leaf(cx);
    let Some(target) = lane_script_target(cx, trap).filter(|target| !target.forwards()) else {
        return Err(exit(ip));
    };
    let Some(this) = lane_call_this(vm, &target, handler) else {
        return Err(exit(ip));
    };
    let passed = ActivationArguments::Kept(arguments.as_ptr(), GET_TRAP_ARGUMENTS);
    let activated = lane_activate_keeping::<H, false>(cx, r, ip, &target, passed, this, false, None)?;
    owe_invariants(vm, Completion::ReadsTrapped, TrapCheck { target: arguments[0], atom, access: TrapAccess::Read });
    Ok(activated)
}

/// Records that the trap activation `lane_activate_keeping` just pushed completes as
/// `completion` says and owes `check` its target's invariants.
fn owe_invariants<H: Host>(vm: &mut Vm<H>, completion: Completion, check: TrapCheck) {
    let depth = vm.lane_stack.activations.len() - 1;
    if let Some(Activation { kind: ActivationKind::Script(script), .. }) = vm.lane_stack.activations.last_mut() {
        script.completion = completion;
    }
    vm.lane_stack.trap_checks.push((depth, check));
}

/// Whether `activation` is a trap's (`Completion::ReadsTrapped`, `AssignsTrapped`) whose
/// result `value` the general path must check: the target's shape no longer leaves the key
/// unconstrained, which the lane's pre-check found it did before the trap ran, or a strict
/// store's trap returns a falsish value, which is the store's `TypeError`. It returns from
/// its frame, so that its pending call checks it (`CallCompletion::Trapped`).
#[inline(always)]
pub(super) fn trap_result_needs_general_path<H: Host>(vm: &Vm<H>, activation: Option<&Activation>, value: Value) -> bool {
    let Some(Activation { kind: ActivationKind::Script(script), .. }) = activation else {
        return false;
    };
    if !matches!(script.completion, Completion::ReadsTrapped | Completion::AssignsTrapped) {
        return false;
    }
    let Some((_, check)) = vm.lane_stack.trap_checks.last() else {
        return false;
    };
    match check.access {
        TrapAccess::StrictStore if !vm.truthy(value) => true,
        TrapAccess::Store | TrapAccess::StrictStore if !vm.truthy(value) => false,
        _ => !vm.shape_leaves_atom_unconstrained(check.target, check.atom),
    }
}

/// The `set` trap's call for a store of `value` to the name `atom` of the proxy `object`, as
/// `get_trap_call` finds a read's: the trap, the handler it is called on and its values.
fn set_trap_call<H: Host>(vm: &mut Vm<H>, object: Value, atom: crate::bytecode::Atom, value: Value) -> Option<(Value, Value, [Value; SET_TRAP_ARGUMENTS])> {
    let (target, handler) = vm.proxy_parts(object)?;
    if handler.is_null() || vm.is_private_name(atom) {
        return None;
    }
    let trap = vm.plain_property_lookup(handler, crate::vm::PropertyKey::string(vm.runtime_atoms.set))??;
    vm.shape_leaves_atom_unconstrained(target, atom).then_some((trap, handler, [target, vm.atom_string(atom), value, object]))
}

/// A lane store of `value` to the name `atom` of the proxy `object` at the record `ip` whose
/// handlers (it may be nested) have no `set`, `getOwnPropertyDescriptor` or `defineProperty`
/// trap: the ordinary set of the target with the proxy as receiver, which writes the target's
/// writable data property or adds one (`Vm::set_plain_key_property`). `None`, before any
/// effect, leaves any other store, an accessor, a read-only property or a non-extensible
/// target among them, to the general path.
fn lane_store_forwarded<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, object: Value, atom: crate::bytecode::Atom, value: Value) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    vm.proxy_parts(object)?;
    let mut target = object;
    while let Some((inner, handler)) = vm.proxy_parts(target) {
        if !vm.handler_lacks_trap(handler, ProxyTrap::Set) {
            return None;
        }
        target = inner;
    }
    if target != vm.descriptor_forwarding_target(object) || vm.is_private_name(atom) {
        return None;
    }
    vm.set_plain_key_property(target, atom, value, false)?;
    note_allocation(vm);
    Some(Resumed::At(r, following(ip), Carried::script(cx)))
}

/// A lane store of `value` to the name `atom` of `object` at the record `ip` that no setter
/// answers: a proxy's `set` trap of the running program activated with the handler as its
/// receiver (`set_trap_call`), whose completion leaves the record's registers as they are
/// (`Completion::Assigns`, or `AssignsChecked` for a strict store, whose rejection the
/// general path raises). `None`, before any effect, leaves any other store to its path; an
/// activation the lane has no room for exits at the record.
pub(super) fn lane_store_set_trap<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    object: Value,
    atom: crate::bytecode::Atom,
    value: Value,
    strict: bool,
) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    let Some((trap, handler, arguments)) = set_trap_call(vm, object, atom, value) else {
        return lane_store_forwarded(cx, r, ip, object, atom, value);
    };
    let target = lane_script_target(cx, trap).filter(|target| !target.forwards())?;
    let this = lane_call_this(vm, &target, handler)?;
    let passed = ActivationArguments::Kept(arguments.as_ptr(), SET_TRAP_ARGUMENTS);
    Some(match lane_activate_keeping::<H, false>(cx, r, ip, &target, passed, this, false, None) {
        Ok((registers, next)) => {
            let access = if strict { TrapAccess::StrictStore } else { TrapAccess::Store };
            owe_invariants(vm, Completion::AssignsTrapped, TrapCheck { target: arguments[0], atom, access });
            Resumed::At(registers, next, Carried::script(cx))
        }
        Err(next) => Resumed::Exit(next),
    })
}

impl<H: Host> Vm<H> {
    /// The `TypeError` and its instruction of the access whose trap the lane activated and
    /// whose frame returned `value` to the pending call `pending` of `frame`
    /// (`CallCompletion::Trapped`): the trap's result broke an invariant of its target, which
    /// the trap itself may have changed since the lane's pre-check, or a strict store's trap
    /// returned a falsish value. `None` for any other completion or an accepted result.
    pub(in crate::vm) fn trap_result_error(
        &mut self,
        p: &ResidualProgram,
        pending: &PendingGeneralCall,
        frame: usize,
        value: Value,
    ) -> Option<(u32, JsError)> {
        let CallCompletion::Trapped(check) = pending.completion else {
            return None;
        };
        let key = self.atom_string(check.atom);
        let stored = match check.access {
            TrapAccess::Read => Value::UNDEFINED,
            TrapAccess::Store | TrapAccess::StrictStore => self.read(frame, pending.destination),
        };
        let outcome = self.with_call_roots([check.target, key, stored, value], |vm| match check.access {
            TrapAccess::Read => vm.validate_proxy_get(p, check.target, key, value),
            TrapAccess::Store => vm.validate_proxy_store(p, check.target, key, stored, false, value),
            TrapAccess::StrictStore => vm.validate_proxy_store(p, check.target, key, stored, true, value),
        });
        outcome.err().map(|error| (pending.call_pc, error))
    }
}

#[cfg(test)]
#[path = "../proxy_trap_tests.rs"]
mod tests;
