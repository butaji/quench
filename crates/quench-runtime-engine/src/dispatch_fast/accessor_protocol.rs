//! The host protocol of the accessor and proxy call-outs a generated row makes: a read or store
//! that meets a getter, a setter or a proxy trap answers it or runs it as an activation of the
//! lane. The rows call these through `AccessorCall`.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::getters::{lane_read_getter, set_getter_hint, LaneGetterAnswer};
use quench_runtime::staged::call_getter::step_getter_answer_of;
use quench_runtime::staged::Concrete;
use super::proxy_traps::{get_trap_call, keyed_trap_atom, owe_invariants, set_trap_call};
use super::setters::{met_setter, stored_setter};
use quench_runtime::vm::object_get::ordinary_chain::ChainRead;

/// The lane context of one accessor call: the activations a getter, setter or trap makes run in
/// it. A row builds it with `of` where it meets the accessor.
#[derive(Clone, Copy)]
pub(crate) struct AccessorCall {
    cx: *mut LaneContext,
}

impl AccessorCall {
    #[inline(always)]
    pub(crate) fn of(cx: *mut LaneContext) -> Self {
        Self { cx }
    }

    /// A read whose site caches an accessor (`Vm::cached_field_getter`) whose getter is a pure
    /// native read (`Vm::pure_native_getter_value`) or a frameless read target, answered as the
    /// getter's call would return (`call_read_in_lane`): `[[Get]]` calls it on the base with no
    /// arguments, and neither runs guest code. `Some(None)` is a cached getter that must be
    /// called (`activate_getter`); `None`, a site with no accessor entry for `object`.
    ///
    /// # Safety
    /// `self.cx` is the running lane's context.
    #[inline(never)]
    pub(crate) unsafe fn getter_read(&self, object: Value, atom: quench_runtime::bytecode::Atom, site: u16) -> Option<Option<Value>> {
        let vm = unsafe { vm(self.cx) };
        let index = vm.field_cache_index(site);
        let getter = match vm.getter_entry_read(object, index) {
            quench_runtime::vm::accessor_cache::GetterEntryRead::Answer(value, _) => return Some(Some(value)),
            quench_runtime::vm::accessor_cache::GetterEntryRead::Call(getter) => getter,
            quench_runtime::vm::accessor_cache::GetterEntryRead::Miss => vm.cached_getter_after_miss(object, atom, index)?,
        };
        let mut answer = LaneGetterAnswer { cx: self.cx, memo: Some(index) };
        Some(step_getter_answer_of(&mut Concrete::new(std::ptr::null()), &mut answer, getter, object))
    }

    /// A lane read of `base` at the record `i` (at `ip`) that met `getter`: the interpreter's getter
    /// read (`getters::lane_read_getter`), once the admission word admits a native getter, which has
    /// no target to admit it, as a link's compare would.
    pub(crate) fn read_getter(&self, r: Registers, ip: Ip, i: &LaneInstruction, getter: Value, base: Value) -> Result<(Registers, Ip), Ip> {
        let vm = unsafe { vm(self.cx) };
        if !vm.lane_stack.script_calls_allowed() && matches!(vm.function_record(getter), Some(FunctionRecord { kind: FunctionKind::Native(_), .. })) {
            return Err(exit(ip));
        }
        lane_read_getter(self.cx, r, ip, i, getter, base)
    }

    /// What the getter the site cached answers for `base` (`step_getter_answer_of`), made the record's
    /// hint where the getter returns an own field of its receiver; any other getter moves the record
    /// to `get_field_getter_miss` for good.
    ///
    /// # Safety
    /// `self.cx` is the running lane's context and `ip` the `GetField` record `i`.
    pub(crate) unsafe fn hold_getter_hint(&self, ip: Ip, i: &LaneInstruction, base: Value) -> Option<Value> {
        let vm = unsafe { vm(self.cx) };
        let site = vm.field_cache_index(i.c);
        let getter = vm.cached_field_getter(base, i.imm(), site)?;
        let mut answer = LaneGetterAnswer { cx: self.cx, memo: None };
        let value = step_getter_answer_of(&mut Concrete::new(std::ptr::null()), &mut answer, getter, base)?;
        let slot = self
            .getter_field_slot(getter, base)
            .filter(|_| !vm.is_private_name(i.imm()) && vm.cached_accessor_on_prototype(base, i.imm(), site));
        if !slot.is_some_and(|slot| set_getter_hint(vm, ip, i.imm(), base, slot)) {
            requicken(ip, protocol_handler::<{ quench_runtime::bytecode::FIELD_READ as u16 }, ACC_NONE, { protocol_family::ELEMENT | protocol_misses::element::GET_FIELD_GETTER as u16 }>());
        }
        Some(value)
    }

    /// The own slot of `base` that `getter`, a frameless read body, returns (`return this.field`),
    /// by the field cache entry its record answers with.
    fn getter_field_slot(&self, getter: Value, base: Value) -> Option<u16> {
        let target = script_target_in_lane(self.cx, getter).filter(|target| target.entry == CallEntry::Reads)?;
        let vm = unsafe { vm(self.cx) };
        (direct_call_this_in_lane(vm, &target, base)? == base).then_some(())?;
        // SAFETY: a `CallEntry::Reads` target of the running program.
        let LeafRead::Field { base: field_base, kind: RETURNING_FIELD_OWN, cache } = unsafe { LeafRead::of(&target) }? else {
            return None;
        };
        let entry = vm.field_cache_entry(cache);
        let receiver = vm.heap_reads().object_record_by_id(base.direct_object_index()?);
        (quench_runtime::bytecode::FieldBase(field_base) == quench_runtime::bytecode::FieldBase::THIS
            && entry.holder == NO_FIELD_HOLDER
            && entry.receiver == receiver.exact_shape())
        .then_some(entry.slot)
    }

    /// A lane read of `atom` from the proxy `base` at the record `ip` whose handler has no `get`
    /// trap: the read of its target (through further trapless proxies) along the ordinary chain
    /// (`Vm::ordinary_chain_read`), a data value or an absent key completing the record in place
    /// and a getter running as an activation with the proxy as receiver. `Err` carries the
    /// record to continue at: the general path for any other read, a returning or nested record
    /// among them.
    fn read_forwarded(&self, r: Registers, ip: Ip, base: Value, atom: quench_runtime::bytecode::Atom) -> Result<(Registers, Ip), Ip> {
        let vm = unsafe { vm(self.cx) };
        // SAFETY: `ip` is a record of the running view.
        let i = unsafe { &*ip };
        if i.a & quench_runtime::bytecode::RETURN_REGISTER != 0 || i.b == quench_runtime::bytecode::FieldBase::NESTED || vm.is_private_name(atom) {
            return Err(exit(ip));
        }
        let mut target = base;
        while let Some((inner, handler)) = vm.proxy_parts(target) {
            if !vm.handler_lacks_trap(handler, ProxyTrap::Get) {
                return Err(exit(ip));
            }
            target = inner;
        }
        match vm.ordinary_chain_read_proved(target, atom) {
            Some(ChainRead::Value(value)) => Self::complete_read(r, ip, i, value),
            Some(ChainRead::Absent) => Self::complete_read(r, ip, i, Value::UNDEFINED),
            Some(ChainRead::Getter(Some(getter))) if !getter.is_undefined() => activate_getter_in_lane(self.cx, r, ip, getter, base, None),
            _ => Err(exit(ip)),
        }
    }

    /// The read record `i` at `ip` completed in place with `value`.
    fn complete_read(r: Registers, ip: Ip, i: &LaneInstruction, value: Value) -> Result<(Registers, Ip), Ip> {
        r.set(i.a, value);
        Ok((r, following(ip)))
    }

    /// A lane read at the record `ip` of `read`'s base and name that no cached getter answers:
    /// a proxy's `get` trap of the running program activated with the handler as its receiver
    /// (`get_trap_call`), whose completion is the read's value. `Err` carries the record to
    /// continue at: the general path for any other read, or for an activation the lane has no
    /// room for.
    pub(crate) fn read_get_trap(&self, r: Registers, ip: Ip, read: Option<(Value, quench_runtime::bytecode::Atom)>) -> Result<(Registers, Ip), Ip> {
        let vm = unsafe { vm(self.cx) };
        let Some((base, atom)) = read else {
            return Err(exit(ip));
        };
        let Some((trap, handler, arguments)) = get_trap_call(vm, base, atom) else {
            #[cfg(feature = "profile-lane-exits")]
            if let Some((target, handler)) = vm.proxy_parts(base) {
                let trap = vm.plain_property_lookup(handler, quench_runtime::vm::PropertyKey::string(vm.runtime_atoms.get));
                quench_runtime::vm::lane_exits::record_detail(format!(
                    "get trap decline trap={} unconstrained={} name={}",
                    match trap { None => "unanswered", Some(None) => "absent", Some(Some(_)) => "found" },
                    vm.shape_leaves_atom_unconstrained(target, atom),
                    vm.atom_name(atom)
                ));
            }
            return self.read_forwarded(r, ip, base, atom);
        };
        materialize_running_leaf_in_lane(self.cx);
        let Some(target) = self.trap_target(trap) else {
            #[cfg(feature = "profile-lane-exits")]
            quench_runtime::vm::lane_exits::record_detail("get trap decline target".into());
            return Err(exit(ip));
        };
        let activated = self.activate_trap(r, ip, target, handler, &arguments)?;
        owe_invariants(vm, Completion::ReadsTrapped, arguments[0], atom, TrapAccess::Read);
        Ok(activated)
    }

    /// The frameless target of the trap function `trap` and whether it is another program's,
    /// which its activation enters and its return leaves (`complete_activation_in_lane`): a trap of
    /// a library loaded as its own script, as Vue's and MobX's handlers are, activates as one of
    /// the running program's does. `None` for a trap the lane does not activate.
    pub(crate) fn trap_target(&self, trap: Value) -> Option<(ScriptCallTarget, bool)> {
        if let Some(target) = script_target_in_lane(self.cx, trap) {
            return (!target.forwards()).then_some((target, false));
        }
        let vm = unsafe { vm(self.cx) };
        // SAFETY: as `Vm::script_call_entry`; the entry is copied before anything resolves a target.
        let target = vm.cross_call_entry(trap).map(|target| unsafe { *target })?;
        (target.entry.activates() && !target.forwards() && !constructing_in_lane(vm, trap)).then_some((target, true))
    }

    /// The activation of the trap `target` (`trap_target`) called on `handler` with
    /// `arguments` for the access record at `ip`. `Err` carries the record to continue at.
    fn activate_trap<const N: usize>(&self, r: Registers, ip: Ip, (target, crosses): (ScriptCallTarget, bool), handler: Value, arguments: &[Value; N]) -> Result<(Registers, Ip), Ip> {
        let Some(this) = call_this_in_lane(unsafe { vm(self.cx) }, &target, handler) else {
            return Err(exit(ip));
        };
        let passed = ActivationArguments::Kept(arguments.as_ptr(), N);
        if crosses {
            activate_keeping_in_lane::<true>(self.cx, r, ip, &target, passed, this, false, None)
        } else {
            activate_keeping_in_lane::<false>(self.cx, r, ip, &target, passed, this, false, None)
        }
    }

    /// A lane `GetIndex` of `base[key]` at the record `ip` where `base` is a proxy whose `get`
    /// trap a named read would activate (`read_get_trap`): the same activation with the
    /// key's string. `None`, before any effect, for any other read, which goes on along its
    /// path; `Err` carries the record to continue at.
    pub(crate) fn keyed_get_trap(&self, r: Registers, ip: Ip, base: Value, key: Value) -> Option<Result<(Registers, Ip), Ip>> {
        let vm = unsafe { vm(self.cx) };
        vm.proxy_parts(base)?;
        let atom = keyed_trap_atom(vm, key)?;
        let (trap, handler, arguments) = get_trap_call(vm, base, atom)?;
        materialize_running_leaf_in_lane(self.cx);
        let target = self.trap_target(trap)?;
        Some(self.activate_trap(r, ip, target, handler, &arguments).map(|activated| {
            owe_invariants(vm, Completion::ReadsTrapped, arguments[0], atom, TrapAccess::Read);
            activated
        }))
    }

    /// A lane store of `value` to the name `atom` of the proxy `object` at the record `ip` whose
    /// handlers (it may be nested) have no `set`, `getOwnPropertyDescriptor` or `defineProperty`
    /// trap: the ordinary set of the target with the proxy as receiver, which writes the target's
    /// writable data property or adds one (`Vm::set_plain_key_property`). `None`, before any
    /// effect, leaves any other store, an accessor, a read-only property or a non-extensible
    /// target among them, to the general path.
    fn store_forwarded(&self, r: Registers, ip: Ip, object: Value, atom: quench_runtime::bytecode::Atom, value: Value) -> Option<Resumed> {
        let vm = unsafe { vm(self.cx) };
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
        Some(Resumed::At(r, following(ip), Carried::script(self.cx)))
    }

    /// A lane store of `value` to the name `atom` of `object` at the record `ip` that no setter
    /// answers: a proxy's `set` trap of the running program activated with the handler as its
    /// receiver (`set_trap_call`), whose completion leaves the record's registers as they are
    /// (`Completion::Assigns`, or `AssignsChecked` for a strict store, whose rejection the
    /// general path raises). `None`, before any effect, leaves any other store to its path; an
    /// activation the lane has no room for exits at the record.
    pub(crate) fn store_set_trap(&self, r: Registers, ip: Ip, object: Value, atom: quench_runtime::bytecode::Atom, value: Value, strict: bool) -> Option<Resumed> {
        let vm = unsafe { vm(self.cx) };
        let Some((trap, handler, arguments)) = set_trap_call(vm, object, atom, value) else {
            return self.store_forwarded(r, ip, object, atom, value);
        };
        let target = self.trap_target(trap)?;
        call_this_in_lane(vm, &target.0, handler)?;
        Some(match self.activate_trap(r, ip, target, handler, &arguments) {
            Ok((registers, next)) => {
                let access = if strict { TrapAccess::StrictStore } else { TrapAccess::Store };
                owe_invariants(vm, Completion::AssignsTrapped, arguments[0], atom, access);
                Resumed::At(registers, next, Carried::script(self.cx))
            }
            Err(next) => Resumed::Exit(next),
        })
    }

    /// A lane store of `value` to `object` at the record `i` (at `ip`) that meets a setter of the
    /// running program the lane activates: its activation with the stored value as its one
    /// argument and `object` as its receiver, whose completion leaves the record's registers as
    /// they are (`Completion::Assigns`). `None`, before any effect, leaves any other store to its
    /// path; an activation the lane has no room for exits at the record, where the general path
    /// makes the call.
    pub(crate) fn store_setter(&self, r: Registers, ip: Ip, i: &LaneInstruction, object: Value) -> Option<Resumed> {
        let vm = unsafe { vm(self.cx) };
        let site = vm.field_cache_index(i.c);
        if !met_setter(&vm.lane_stack, site) {
            return None;
        }
        let setter = stored_setter(vm, object, i.imm(), site)?;
        let target = script_target_in_lane(self.cx, setter).filter(|target| !target.forwards())?;
        let this = call_this_in_lane(vm, &target, object)?;
        // SAFETY: the record's value register is one of the running function's.
        let stored = unsafe { std::slice::from_raw_parts(r.0.add(usize::from(i.a)).cast_const(), 1) };
        Some(match activate_kept_in_lane::<false>(self.cx, r, ip, &target, stored, this) {
            Ok((registers, next)) => {
                if let Some(ActivationKind::Script(script)) = vm.lane_stack.activations_mut().last_mut().map(|activation| activation.kind_mut()) {
                    script.completion = Completion::Assigns;
                }
                Resumed::At(registers, next, Carried::script(self.cx))
            }
            Err(next) => Resumed::Exit(next),
        })
    }
}
