//! The host protocol of the accessor and proxy call-outs a generated row makes: a read or store
//! that meets a getter, a setter or a proxy trap answers it or runs it as an activation of the
//! lane. The rows call these through `AccessorCall`.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::getters::set_getter_hint;
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
        let Some(value) = (unsafe { self.getter_answer(getter, object) }) else {
            return Some(None);
        };
        self.memoize_getter(getter, object, index);
        Some(Some(value))
    }

    /// Memoizes at the accessor entry of cache index `site` that `getter`, whose call on
    /// `object` a read just answered, returns its own field read of the receiver
    /// (`Vm::memoize_getter_answer`): a `Reads` target whose returning `GetField` of `this` hits
    /// an own or holder entry and binds the receiver as `this`.
    #[inline(never)]
    pub(crate) fn memoize_getter(&self, getter: Value, object: Value, site: usize) {
        let vm = unsafe { vm(self.cx) };
        // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target or collects.
        let Some(target) = vm.script_call_entry(getter).map(|entry| unsafe { *entry }) else {
            return;
        };
        if target.entry != CallEntry::Reads || direct_call_this_in_lane(vm, &target, object) != Some(object) {
            return;
        }
        // SAFETY: a `Reads` target of the running program holds its returning record first.
        let Some(LeafRead::Field { base, kind: RETURNING_FIELD_OWN | RETURNING_FIELD_HOLDER, cache }) = (unsafe { LeafRead::of(&target) }) else {
            return;
        };
        if base == quench_runtime::bytecode::FieldBase::THIS.0 {
            let answer = vm.field_cache_entry(cache);
            vm.memoize_getter_answer(object, site, getter, quench_runtime::vm::AccessorForward::Field(answer));
        }
    }

    /// What `getter` returns for a read of `object` where its call runs no guest code: a pure
    /// native read, or a frameless read target (`getter_read`).
    ///
    /// # Safety
    /// `self.cx` is the running lane's context.
    #[inline(never)]
    unsafe fn getter_answer(&self, getter: Value, object: Value) -> Option<Value> {
        if let Some(value) = (unsafe { vm(self.cx) }).pure_native_getter_value(getter, object) {
            return Some(value);
        }
        let target = script_target_in_lane(self.cx, getter)?;
        unsafe { self.script_getter_answer(&target, object) }
    }

    /// What the frameless script getter `target` returns for a read of `object` where a read
    /// answers its call (`CallEntry::answered_by_read`); `None` for any other getter.
    ///
    /// # Safety
    /// `self.cx` is the running lane's context.
    #[inline(always)]
    unsafe fn script_getter_answer(&self, target: &ScriptCallTarget, object: Value) -> Option<Value> {
        if target.entry == CallEntry::Reads {
            // A getter only the general path has called has not run its read in the lane yet: it
            // takes the kind its first run would.
            // SAFETY: as `call_read_in_lane`; the context's program outlives the lane run.
            let (entry, code) = unsafe { ((*target.view).records(), &*(*self.cx).code) };
            if unsafe { (*entry).handler } == row_returning_field_first::<NOT_LEAF> as *const () {
                requicken(entry, returning_field_first_kind::<NOT_LEAF>(unsafe { vm(self.cx) }, code, unsafe { &*entry }));
            }
        } else if !target.entry.answered_by_read() {
            return None;
        }
        let this = direct_call_this_in_lane(unsafe { vm(self.cx) }, target, object)?;
        unsafe { call_read_in_lane(self.cx, target, ReadArguments::NONE, this) }
    }

    /// A lane read of `base` at the record `i` (at `ip`) that met `getter`: its value where a read
    /// answers the getter's call (`getter_answer`), else the getter's activation
    /// (`activate_getter_in_lane`), whose completion is the read's value. Only a read that writes its
    /// register is answered in place: a returning read (`RETURN_REGISTER`) completes its
    /// activation, which only the getter's activation does. A native getter is called in place
    /// (`call_native_in_lane`); `Err` after such a call that raised leaves the error in
    /// `Vm::lane_raised`. The answer's read may have made the activations frames, so the
    /// registers are the context's again.
    pub(crate) fn read_getter(&self, r: Registers, ip: Ip, i: &LaneInstruction, getter: Value, base: Value) -> Result<(Registers, Ip), Ip> {
        let reads = i.a & quench_runtime::bytecode::RETURN_REGISTER == 0;
        // A native getter is answered or called in place; a script getter's target, resolved
        // once, is answered by its read or activated.
        if let Some(FunctionRecord { kind: FunctionKind::Native(native), .. }) = unsafe { vm(self.cx) }.heap.function(getter) {
            let native = *native;
            // A native getter has no target to admit it: the admission word decides, as a link's
            // compare would.
            if !unsafe { vm(self.cx) }.lane_stack.script_calls_allowed() {
                return Err(exit(ip));
            }
            if reads {
                if let Some(value) = unsafe { vm(self.cx) }.pure_native_getter_value(getter, base) {
                    let r = unsafe { (*self.cx).registers };
                    r.set(i.a, value);
                    return Ok((r, following(ip)));
                }
                return self.native_getter(ip, i, native, getter, base);
            }
            return activate_getter_in_lane(self.cx, r, ip, getter, base, None);
        }
        let target = script_target_in_lane(self.cx, getter);
        if reads
            && let Some(target) = &target
            && let Some(value) = unsafe { self.script_getter_answer(target, base) }
        {
            if i.b != quench_runtime::bytecode::FieldBase::NESTED {
                let site = unsafe { vm(self.cx) }.field_cache_index(i.c);
                self.memoize_getter(getter, base, site);
            }
            let r = unsafe { (*self.cx).registers };
            r.set(i.a, value);
            return Ok((r, following(ip)));
        }
        activate_getter_in_lane(self.cx, r, ip, getter, base, target)
    }

    /// `read_getter` of a native getter: called as a lane `Call` of it with no arguments is.
    /// Out of line, so script getters' activations keep their code.
    #[cold]
    #[inline(never)]
    fn native_getter(&self, ip: Ip, i: &LaneInstruction, native: Native, getter: Value, base: Value) -> Result<(Registers, Ip), Ip> {
        match native_complete_in_lane(self.cx, ip, i, call_native_in_lane(self.cx, ip, native, getter, base, &[])) {
            Resumed::At(r, next, _) => Ok((r, next)),
            Resumed::Exit(next) => Err(next),
        }
    }

    /// What the getter the site cached answers for `base` (`getter_answer`), made the record's
    /// hint where the getter returns an own field of its receiver; any other getter moves the record
    /// to `get_field_getter_miss` for good.
    ///
    /// # Safety
    /// `self.cx` is the running lane's context and `ip` the `GetField` record `i`.
    pub(crate) unsafe fn hold_getter_hint(&self, ip: Ip, i: &LaneInstruction, base: Value) -> Option<Value> {
        let vm = unsafe { vm(self.cx) };
        let site = vm.field_cache_index(i.c);
        let getter = vm.cached_field_getter(base, i.imm(), site)?;
        let value = unsafe { self.getter_answer(getter, base) }?;
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
        let receiver = vm.heap.object_record_by_id(base.direct_object_index()?);
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
