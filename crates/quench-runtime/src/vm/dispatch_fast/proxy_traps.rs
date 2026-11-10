//! Reads of a proxy whose `get` trap runs as an activation of the lane.
use super::*;

/// The values a `get` trap is called with: the target, the key and the receiver.
const GET_TRAP_ARGUMENTS: usize = 3;

/// What a read of `atom` of the proxy `object` calls where the trap's call is the whole
/// read: the `get` trap, a plain data property of an ordinary handler up its chain (so
/// finding it runs no guest code), the handler it is called on, and the values it is called
/// with. The target's shape leaves the key unconstrained (`Vm::shape_leaves_key_unconstrained`),
/// so no invariant checks the trap's result. `None` for any other read: no proxy, a revoked
/// one, a private name, no trap, or a target that may constrain the key.
fn get_trap_call<H: Host>(vm: &mut Vm<H>, object: Value, atom: crate::bytecode::Atom) -> Option<(Value, Value, [Value; GET_TRAP_ARGUMENTS])> {
    let (target, handler) = vm.proxy_parts(object)?;
    if handler.is_null() || vm.is_private_name(atom) {
        return None;
    }
    let trap = vm.plain_property_lookup(handler, crate::vm::PropertyKey::string(vm.runtime_atoms.get))??;
    let key = vm.atom_string(atom);
    vm.shape_leaves_key_unconstrained(target, key).then_some((trap, handler, [target, key, object]))
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

#[cfg(test)]
#[path = "../proxy_trap_tests.rs"]
mod tests;
