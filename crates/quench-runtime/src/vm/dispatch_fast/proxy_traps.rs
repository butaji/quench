//! Reads of a proxy whose `get` trap runs as an activation of the lane.
use super::*;

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
    let Some((trap, handler, arguments)) = read.and_then(|(base, atom)| get_trap_call(vm, base, atom)) else {
        return Err(exit(ip));
    };
    lane_materialize_running_leaf(cx);
    let Some(target) = lane_script_target(cx, trap).filter(|target| !target.forwards()) else {
        return Err(exit(ip));
    };
    let Some(this) = lane_call_this(vm, &target, handler) else {
        return Err(exit(ip));
    };
    let passed = ActivationArguments::Kept(arguments.as_ptr(), GET_TRAP_ARGUMENTS);
    lane_activate_keeping::<H, false>(cx, r, ip, &target, passed, this, false, None)
}

/// Whether the innermost activation is a strict store's `set` trap (`Completion::AssignsChecked`)
/// returning the falsish `value`, a rejected store.
#[inline(always)]
pub(super) fn set_trap_refuses<H: Host>(vm: &Vm<H>, value: Value) -> bool {
    matches!(vm.lane_stack.activations.last(), Some(Activation { kind: ActivationKind::Script(script), .. }) if script.completion == Completion::AssignsChecked)
        && !vm.truthy(value)
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
    let (trap, handler, arguments) = set_trap_call(vm, object, atom, value)?;
    let target = lane_script_target(cx, trap).filter(|target| !target.forwards())?;
    let this = lane_call_this(vm, &target, handler)?;
    let passed = ActivationArguments::Kept(arguments.as_ptr(), SET_TRAP_ARGUMENTS);
    Some(match lane_activate_keeping::<H, false>(cx, r, ip, &target, passed, this, false, None) {
        Ok((registers, next)) => {
            if let Some(Activation { kind: ActivationKind::Script(script), .. }) = vm.lane_stack.activations.last_mut() {
                script.completion = if strict { Completion::AssignsChecked } else { Completion::Assigns };
            }
            Resumed::At(registers, next, Carried::script(cx))
        }
        Err(next) => Resumed::Exit(next),
    })
}

impl<H: Host> Vm<H> {
    /// The `TypeError` and its instruction of the strict store whose `set` trap's frame returned
    /// the falsish `value` to the pending call `pending` of `frame`
    /// (`CallCompletion::AssignedChecked`); `None` for any other completion or a truthy value.
    pub(in crate::vm) fn refused_store_error(
        &mut self,
        p: &ResidualProgram,
        pending: &PendingGeneralCall,
        frame: usize,
        value: Value,
    ) -> Option<(u32, JsError)> {
        if !pending.completion.is_checked_assignment() || self.truthy(value) {
            return None;
        }
        let body = p.function(self.frames[frame].function as usize);
        let atom = decoded_instruction(body, body.code[pending.call_pc as usize]).atom_index();
        let key = self.atom_string(atom);
        Some((pending.call_pc, self.proxy_set_refused_error(p, key)))
    }
}

#[cfg(test)]
#[path = "../proxy_trap_tests.rs"]
mod tests;
