//! Reads of a proxy whose `get` trap runs as an activation of the lane. The activations are
//! `AccessorCall`'s (`accessor_protocol.rs`); this file decides which trap a read calls.
use super::*;
use quench_runtime::vm::object_get::ordinary_chain::ChainRead;

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
pub(crate) fn get_trap_call(vm: &mut Vm, object: Value, atom: quench_runtime::bytecode::Atom) -> Option<(Value, Value, [Value; GET_TRAP_ARGUMENTS])> {
    let (target, handler) = vm.proxy_parts(object)?;
    if handler.is_null() || vm.is_private_name(atom) {
        return None;
    }
    // The handler's `get` through the fact table (`Vm::ordinary_chain_read_proved`), else
    // the plain walk, which declines an accessor.
    let trap = match vm.ordinary_chain_read_proved(handler, vm.runtime_atoms.get) {
        Some(ChainRead::Value(trap)) => trap,
        Some(ChainRead::Absent) => return None,
        _ => vm.plain_property_lookup(handler, quench_runtime::vm::PropertyKey::string(vm.runtime_atoms.get))??,
    };
    vm.shape_leaves_atom_unconstrained(target, atom).then_some((trap, handler, [target, vm.atom_string(atom), object]))
}

/// The atom of a computed key a proxy's trap receives as a string: a string's own, or the
/// canonical decimal text of an array index. `None` for any other key (a symbol, a negative
/// or fractional number, an object whose ToPropertyKey runs guest code).
pub(crate) fn keyed_trap_atom(vm: &mut Vm, key: Value) -> Option<quench_runtime::bytecode::Atom> {
    if let Some(index) = key.as_int().and_then(|index| usize::try_from(index).ok()) {
        return Some(vm.index_key_atom(index));
    }
    vm.string_key_atom(key)
}

/// Records that the trap activation `activate_keeping_in_lane` just pushed completes as
/// `completion` says and owes `target` its invariants for `atom` (`TrapCheck`).
pub(crate) fn owe_invariants(vm: &mut Vm, completion: Completion, target: Value, atom: quench_runtime::bytecode::Atom, access: TrapAccess) {
    // An array's `length` and elements are answered by its element facts too, which its
    // layout does not name; any other name by its shape, which its layout does.
    let by_layout = vm.heap.array(target).is_none() || (atom != vm.length_atom && !vm.atom_class(atom).contains(quench_runtime::vm::object::AtomClass::ARRAY_INDEX));
    let layout = if by_layout { vm.object_data(target).map_or(0, |data| data.layout_id()) } else { 0 };
    let check = TrapCheck { target, atom, access, layout };
    vm.lane_stack.recomplete_top(completion);
    vm.lane_stack.attach_payload(SidePayload::Trapped(check));
}

/// Whether `activation` is a trap's (`Completion::ReadsTrapped`, `AssignsTrapped`) whose
/// result `value` the general path must check: the target's shape no longer leaves the key
/// unconstrained, which the lane's pre-check found it did before the trap ran, or a strict
/// store's trap returns a falsish value, which is the store's `TypeError`. It returns from
/// its frame, so that its pending call checks it (`CallCompletion::Trapped`).
#[inline(always)]
pub(crate) fn trap_result_needs_general_path(vm: &Vm, payload: SidePayload, value: Value) -> bool {
    // A trap's activation carries its check in its side record (one-frame-one-call.md F4 (3)).
    let SidePayload::Trapped(check) = payload else {
        return false;
    };
    match check.access {
        TrapAccess::StrictStore if !vm.truthy(value) => true,
        TrapAccess::Store | TrapAccess::StrictStore if !vm.truthy(value) => false,
        // A target whose layout the trap left as it was leaves the key unconstrained still.
        _ => !(check.layout != 0 && vm.object_data(check.target).is_some_and(|data| data.layout_id() == check.layout))
            && !vm.shape_leaves_atom_unconstrained(check.target, check.atom),
    }
}

/// The `set` trap's call for a store of `value` to the name `atom` of the proxy `object`, as
/// `get_trap_call` finds a read's: the trap, the handler it is called on and its values.
pub(crate) fn set_trap_call(vm: &mut Vm, object: Value, atom: quench_runtime::bytecode::Atom, value: Value) -> Option<(Value, Value, [Value; SET_TRAP_ARGUMENTS])> {
    let (target, handler) = vm.proxy_parts(object)?;
    if handler.is_null() || vm.is_private_name(atom) {
        return None;
    }
    let trap = vm.plain_property_lookup(handler, quench_runtime::vm::PropertyKey::string(vm.runtime_atoms.set))??;
    vm.shape_leaves_atom_unconstrained(target, atom).then_some((trap, handler, [target, vm.atom_string(atom), value, object]))
}

#[quench_macros::ext(DispatchFastProxyTrapsVm)]
impl Vm {
    /// The `TypeError` and its instruction of the access whose trap the lane activated and
    /// whose frame returned `value` to the pending call `pending` of `frame`
    /// (`CallCompletion::Trapped`): the trap's result broke an invariant of its target, which
    /// the trap itself may have changed since the lane's pre-check, or a strict store's trap
    /// returned a falsish value. `None` for any other completion or an accepted result.
    pub(crate) fn trap_result_error(
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
pub(crate) mod tests;
