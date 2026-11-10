//! Stores that meet an accessor: the setter runs as an activation of the lane.
use super::*;

/// The store sites a general store has called a setter at, one bit each by the site's cache
/// index modulo the set's width: the lane looks for a setter only at such a site, so a store
/// that meets none pays no lookup. Sites sharing a bit only make that lookup.
const SETTER_SITE_BITS: usize = 1 << 16;

thread_local! {
    static SETTER_SITES: std::cell::RefCell<Vec<u64>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// `SETTER_SITES`' word and bit of the site at cache index `site`.
fn setter_site_bit(site: usize) -> (usize, u64) {
    let bit = site % SETTER_SITE_BITS;
    (bit / u64::BITS as usize, 1 << (bit % u64::BITS as usize))
}

/// Whether a general store called a setter at the site at cache index `site`.
fn met_setter(site: usize) -> bool {
    let (word, bit) = setter_site_bit(site);
    SETTER_SITES.with_borrow(|sites| sites.get(word).is_some_and(|bits| bits & bit != 0))
}

/// The setter a store of `atom` to `object` calls, found as the general store finds it
/// (`Vm::set_field_uncached`): an ordinary object's own or inherited accessor, with no proxy
/// up its chain to mediate an inherited one. `None` for any other store, a private name's
/// among them.
fn stored_setter<H: Host>(vm: &Vm<H>, object: Value, atom: crate::bytecode::Atom) -> Option<Value> {
    if !vm.is_object_like(object) || vm.is_wasm_gc_object(object) || vm.proxy_parts(object).is_some() || vm.is_private_name(atom) {
        return None;
    }
    if vm.own_property(object, atom).is_none() && vm.prototype_chain_contains_proxy(object) {
        return None;
    }
    vm.property_accessor(object, atom)?.setter
}

/// A store of the record `i` (at `ip`) to `object` that meets a setter of the running
/// program the lane activates: its activation with the stored value as its one argument and
/// `object` as its receiver, whose completion leaves the record's registers as they are
/// (`Completion::Assigns`). `None`, before any effect, leaves any other store to its path;
/// an activation the lane has no room for exits at the record, where the general path makes
/// the call.
pub(super) fn lane_store_setter<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, i: &LaneInstruction, object: Value) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    if !met_setter(vm.field_cache_index(i.c)) {
        return None;
    }
    let setter = stored_setter(vm, object, i.imm())?;
    let target = lane_script_target(cx, setter).filter(|target| !target.forwards())?;
    let this = lane_call_this(vm, &target, object)?;
    // SAFETY: the record's value register is one of the running function's.
    let stored = ActivationArguments::Kept(unsafe { r.0.add(usize::from(i.a)) }, 1);
    Some(match lane_activate_keeping::<H, false>(cx, r, ip, &target, stored, this, false, None) {
        Ok((registers, next)) => {
            if let Some(Activation { kind: ActivationKind::Script(script), .. }) = vm.lane_stack.activations.last_mut() {
                script.completion = Completion::Assigns;
            }
            Resumed::At(registers, next, Carried::script(cx))
        }
        Err(next) => Resumed::Exit(next),
    })
}

/// What the call of `activation`, which completed with `value` and receiver `this`, yields
/// before a forwarder's or a promise's substitution: [[Construct]] of an ordinary constructor
/// its returned object, otherwise the receiver it allocated; a setter's store
/// (`Completion::Assigns`) the value stored, which the caller's register still holds.
#[inline(always)]
pub(super) fn completed_value<H: Host>(vm: &Vm<H>, activation: &Activation, completion: Completion, this: Value, value: Value) -> Value {
    match completion {
        completion if completion.constructs() && !vm.is_object_like(value) => this,
        // SAFETY: the call record belongs to the caller's live view.
        Completion::Assigns => activation.caller_registers.get(unsafe { (*activation.call).a }),
        _ => value,
    }
}

/// The frame completion of an activation that neither constructs, settles a promise nor
/// steps an array: a setter's store keeps yielding the value stored.
pub(super) fn frame_completion(completion: Option<Completion>) -> CallCompletion {
    match completion {
        Some(Completion::Assigns) => CallCompletion::Assigned,
        _ => CallCompletion::Value,
    }
}

impl<H: Host> Vm<H> {
    /// The general store's call of `setter` at `site` with `value` on `object`, which marks
    /// the site as one that meets a setter (`SETTER_SITES`).
    pub(in crate::vm) fn call_stored_setter(&mut self, p: &ResidualProgram, site: u16, setter: Value, object: Value, value: Value) -> Result<Value, JsError> {
        let (word, bit) = setter_site_bit(self.field_cache_index(site));
        SETTER_SITES.with_borrow_mut(|sites| {
            if sites.is_empty() {
                sites.resize(SETTER_SITE_BITS / u64::BITS as usize, 0);
            }
            sites[word] |= bit;
        });
        self.call_value(p, setter, object, &[value])
    }

    /// What a pushed call that completed with `value` yields to `destination` of frame
    /// `caller`: [[Construct]] of an ordinary constructor its returned object, otherwise the
    /// receiver it allocated; a setter's store the value stored, which the register holds.
    pub(in crate::vm) fn completed_call_value(&self, completion: &CallCompletion, caller: usize, destination: crate::bytecode::Register, value: Value) -> Value {
        match completion {
            CallCompletion::Constructor(this) if !self.is_object_like(value) => *this,
            CallCompletion::Assigned => self.read(caller, destination),
            _ => value,
        }
    }
}

#[cfg(test)]
#[path = "../setter_tests.rs"]
mod tests;
