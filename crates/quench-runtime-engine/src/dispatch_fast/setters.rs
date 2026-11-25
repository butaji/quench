//! Stores that meet an accessor: the setter runs as an activation of the lane.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

use quench_runtime::vm::engine_types::SETTER_SITE_BITS;

/// `LaneStack::setter_sites`' word and bit of the site at cache index `site`.
#[inline(always)]
fn setter_site_bit(site: usize) -> (usize, u64) {
    let bit = site % SETTER_SITE_BITS;
    (bit / u64::BITS as usize, 1 << (bit % u64::BITS as usize))
}

/// Whether a general store called a setter at the site at cache index `site`.
#[inline(always)]
pub(crate) fn met_setter(stack: &LaneStack, site: usize) -> bool {
    let (word, bit) = setter_site_bit(site);
    stack.setter_sites.get(word).is_some_and(|bits| bits & bit != 0)
}

/// The setter a store of `atom` to `object` calls at the store site `site`: the site's cached
/// accessor entry (`Vm::cached_field_accessor`), else the one the general store finds
/// (`Vm::set_field_uncached`): an ordinary object's own or inherited accessor, with no proxy
/// up its chain to mediate an inherited one, which the site then caches. `None` for any other
/// store, a private name's among them.
pub(crate) fn stored_setter(vm: &mut Vm, object: Value, atom: quench_runtime::bytecode::Atom, site: usize) -> Option<Value> {
    if let Some((_, setter)) = vm.cached_field_accessor(object, atom, site) {
        return setter;
    }
    if !vm.is_object_like(object) || vm.is_wasm_gc_object(object) || vm.proxy_parts(object).is_some() || vm.is_private_name(atom) {
        return None;
    }
    if vm.own_property(object, atom).is_none() && vm.prototype_chain_contains_proxy(object) {
        return None;
    }
    let setter = vm.property_accessor(object, atom)?.setter;
    vm.record_accessor_field_cache(object, atom, site, true);
    setter
}

/// What the call of the returned `frame`, which completed with `value` and receiver `this`, yields
/// before a forwarder's or a promise's substitution: [[Construct]] of an ordinary constructor
/// its returned object, otherwise the receiver it allocated; a setter's store
/// (`Completion::Assigns`) the value stored, which the caller's register still holds.
#[inline(always)]
pub(crate) fn completed_value(vm: &Vm, frame: &ActivationView, completion: Completion, this: Value, value: Value) -> Value {
    match completion {
        // The construct family's completion row (`staged::call_construct::construct_value`).
        completion if completion.constructs() =>
            Value::from_bits(quench_runtime::staged::call_construct::concrete_construct_value(value.to_bits(), this.to_bits(), vm.is_object_like(value))),
        // SAFETY: the call record belongs to the caller's live view.
        Completion::Assigns | Completion::AssignsTrapped =>
            quench_runtime::staged::call_complete::concrete_call_result(frame.caller_registers().0.expose_provenance() as u64, unsafe { (*frame.call()).a }),
        _ => value,
    }
}

/// The frame completion of an activation that neither constructs, settles a promise nor
/// steps an array: a setter's store keeps yielding the value stored, a proxy trap's access
/// owes its target's invariants (its side record's `payload`), and a generator's step its iterator
/// result, which a delegate's step makes fresh.
pub(crate) fn frame_completion(completion: Option<Completion>, payload: SidePayload) -> CallCompletion {
    match completion {
        Some(Completion::Assigns) => CallCompletion::Assigned,
        Some(Completion::ReadsTrapped | Completion::AssignsTrapped) => CallCompletion::Trapped(payload.trap()),
        Some(generator) if generator.is_generator() => {
            CallCompletion::Generator(quench_runtime::vm::generator::GeneratorResume::new(generator.reuses_result()))
        }
        _ => CallCompletion::Value,
    }
}

#[quench_macros::ext(DispatchFastSettersVm)]
impl Vm {
    /// The general store's call of `setter` at `site` with `value` on `object`, which marks
    /// the site as one that meets a setter (`LaneStack::setter_sites`).
    pub(crate) fn call_stored_setter(&mut self, p: &ResidualProgram, site: u16, setter: Value, object: Value, value: Value) -> Result<Value, JsError> {
        let (word, bit) = setter_site_bit(self.field_cache_index(site));
        let sites = &mut self.lane_stack.setter_sites;
        if sites.is_empty() {
            sites.resize(SETTER_SITE_BITS / u64::BITS as usize, 0);
        }
        sites[word] |= bit;
        self.call_value(p, setter, object, &[value])
    }

    /// What a pushed call that completed with `value` yields to `destination` of frame
    /// `caller`: [[Construct]] of an ordinary constructor its returned object, otherwise the
    /// receiver it allocated; a setter's store the value stored, which the register holds.
    pub(crate) fn completed_call_value(&self, completion: &CallCompletion, caller: usize, destination: quench_runtime::bytecode::Register, value: Value) -> Value {
        match completion {
            // The construct family's completion row (`staged::call_construct::construct_value`).
            CallCompletion::Constructor(this) =>
                Value::from_bits(quench_runtime::staged::call_construct::concrete_construct_value(value.to_bits(), this.to_bits(), self.is_object_like(value))),
            CallCompletion::Assigned | CallCompletion::Trapped(TrapCheck { access: TrapAccess::Store | TrapAccess::StrictStore, .. }) => self.read(caller, destination),
            #[cfg(feature = "wasm")]
            CallCompletion::WasmExport(result) => self.wasm_export_result(value, *result),
            #[cfg(not(feature = "wasm"))]
            CallCompletion::WasmExport(_) => value,
            _ => value,
        }
    }
}

#[cfg(test)]
#[path = "../setter_tests.rs"]
pub(crate) mod tests;
