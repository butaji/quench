//! Stores that meet an accessor: the setter runs as an activation of the lane.
use super::*;
use quench_runtime::vm::AccessorForward;
use quench_runtime::vm::accessor_cache::SetterForward;

/// The store sites a general store has called a setter at, one bit each by the site's cache
/// index modulo the set's width: the lane looks for a setter only at such a site, so a store
/// that meets none pays no lookup. Sites sharing a bit only make that lookup.
/// The set is the lane stack's own (`LaneStack::setter_sites`): a plain load per store, no
/// thread-local access or borrow.
const SETTER_SITE_BITS: usize = 1 << 16;

/// `LaneStack::setter_sites`' word and bit of the site at cache index `site`.
#[inline(always)]
fn setter_site_bit(site: usize) -> (usize, u64) {
    let bit = site % SETTER_SITE_BITS;
    (bit / u64::BITS as usize, 1 << (bit % u64::BITS as usize))
}

/// Whether a general store called a setter at the site at cache index `site`.
#[inline(always)]
pub(super) fn met_setter(stack: &LaneStack, site: usize) -> bool {
    let (word, bit) = setter_site_bit(site);
    stack.setter_sites.get(word).is_some_and(|bits| bits & bit != 0)
}

/// The setter a store of `atom` to `object` calls at the store site `site`: the site's cached
/// accessor entry (`Vm::cached_field_accessor`), else the one the general store finds
/// (`Vm::set_field_uncached`): an ordinary object's own or inherited accessor, with no proxy
/// up its chain to mediate an inherited one, which the site then caches. `None` for any other
/// store, a private name's among them.
pub(super) fn stored_setter(vm: &mut Vm, object: Value, atom: quench_runtime::bytecode::Atom, site: usize) -> Option<Value> {
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

/// A store of the record `i` (at `ip`) to `object` that meets a setter of the running
/// program the lane activates: its activation with the stored value as its one argument and
/// `object` as its receiver, whose completion leaves the record's registers as they are
/// (`Completion::Assigns`). `None`, before any effect, leaves any other store to its path;
/// an activation the lane has no room for exits at the record, where the general path makes
/// the call.
pub(super) fn lane_store_setter(cx: *mut LaneContext, r: Registers, ip: Ip, i: &LaneInstruction, object: Value) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    let site = vm.field_cache_index(i.c);
    if !met_setter(&vm.lane_stack, site) {
        return None;
    }
    let setter = stored_setter(vm, object, i.imm(), site)?;
    let target = script_target_in_lane(cx, setter).filter(|target| !target.forwards())?;
    // A trivial setter (`this.f = v`) is the store it makes (getters-as-calls.md): no call.
    if lane_forwarded_setter_store(vm, &target, object, site, setter, r.get(i.a)) {
        return Some(Resumed::At(r, following(ip), Carried::script(cx)));
    }
    let this = call_this_in_lane(vm, &target, object)?;
    // SAFETY: the record's value register is one of the running function's.
    let stored = unsafe { std::slice::from_raw_parts(r.0.add(usize::from(i.a)).cast_const(), 1) };
    Some(match activate_kept_in_lane::<false>(cx, r, ip, &target, stored, this) {
        Ok((registers, next)) => {
            vm.lane_stack.recomplete_top(Completion::Assigns);
            Resumed::At(registers, next, Carried::script(cx))
        }
        Err(next) => Resumed::Exit(next),
    })
}

/// Whether the store of `value` to `object` at cache index `site` was made as the call of
/// `setter` (`target`) would make it, without the call: the site's accessor entry for
/// `object`'s layout recorded that the setter's body is a forward store
/// (`trivial_setter_store`), and that store's own field entry holds for `object`
/// (`Vm::hinted_own_field_store_hit`, which makes the logged store or nothing). A setter
/// the entry knows nothing of is recognized once and recorded either way, so a setter that
/// forwards nothing costs one entry walk per store after its first.
fn lane_forwarded_setter_store(
    vm: &mut Vm,
    target: &ScriptCallTarget,
    object: Value,
    site: usize,
    setter: Value,
    value: Value,
) -> bool {
    let (cache, atom) = match vm.setter_entry_forward(object, site, setter) {
        SetterForward::Store(cache, atom) => (cache, atom),
        SetterForward::Declined => return false,
        SetterForward::Unknown => match trivial_setter_store(vm, target, object) {
            SetterStore::Store(cache, atom) => {
                vm.memoize_getter_answer(object, site, setter, AccessorForward::Store { cache, atom });
                (cache, atom)
            }
            SetterStore::Never => {
                vm.memoize_getter_answer(object, site, setter, AccessorForward::None);
                return false;
            }
            // The body's own store site has no entry for this receiver yet: the call fills it.
            SetterStore::NotYet => return false,
        },
    };
    vm.hinted_own_field_store_hit(cache, object, atom, value)
}

/// What `trivial_setter_store` found of a setter's body.
enum SetterStore {
    /// It stores its first parameter to the own field `atom` of its receiver, whose store
    /// entry for the receiver's exact shape is this one.
    Store(quench_runtime::vm::FieldCache, quench_runtime::bytecode::Atom),
    /// It is no such body, nor binds the receiver as its `this`: it never forwards.
    Never,
    /// It is such a body, but its store site holds no own entry for this receiver yet.
    NotYet,
}

/// Recognizes a setter `target` whose whole effect is `this.f = v` of its first parameter,
/// then returning (`Function::initializer_stores` of exactly one store of parameter 0 as
/// passed): a call of it on `object`, which it binds as `this`, is that store. The store
/// is exact where its entry, the setter's own store site's, is an own data field of
/// `object`'s exact shape: such a store finds no setter, can't throw and runs no guest code;
/// what the setter returns, an assignment ignores (`Completion::Assigns`). A private name's
/// store is left to the call (its home is the setter's).
fn trivial_setter_store(vm: &Vm, target: &ScriptCallTarget, object: Value) -> SetterStore {
    if !target.entry.activates() || target.params == 0 || direct_call_this_in_lane(vm, target, object) != Some(object) {
        return SetterStore::Never;
    }
    let Some(code) = vm.programs.residual(target.program()) else {
        return SetterStore::Never;
    };
    let Some(body) = code.function_get(target.function as usize) else {
        return SetterStore::Never;
    };
    let Some([store]) = body.initializer_stores(&code.constants) else {
        return SetterStore::Never;
    };
    let parameter = quench_runtime::bytecode::InitializerValue::Parameter { parameter: 0, fallback: None };
    if store.value != parameter || vm.is_private_name(store.atom) {
        return SetterStore::Never;
    }
    // The setter is of the running program (`script_target_in_lane`), whose sites its store's are.
    let cache = vm.field_cache_entry(store.site);
    let holds = vm.heap.object_record(object).is_some_and(|receiver| cache.holder == quench_runtime::vm::NO_FIELD_HOLDER && cache.receiver == receiver.exact_shape());
    if holds { SetterStore::Store(cache, store.atom) } else { SetterStore::NotYet }
}

/// What the call of the returned `frame`, which completed with `value` and receiver `this`, yields
/// before a forwarder's or a promise's substitution: [[Construct]] of an ordinary constructor
/// its returned object, otherwise the receiver it allocated; a setter's store
/// (`Completion::Assigns`) the value stored, which the caller's register still holds.
#[inline(always)]
pub(super) fn completed_value(vm: &Vm, frame: &ActivationView, completion: Completion, this: Value, value: Value) -> Value {
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
pub(super) fn frame_completion(completion: Option<Completion>, payload: SidePayload) -> CallCompletion {
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
            CallCompletion::WasmExport(result) => self.wasm_export_result(value, *result),
            _ => value,
        }
    }
}

#[cfg(test)]
#[path = "../setter_tests.rs"]
mod tests;
