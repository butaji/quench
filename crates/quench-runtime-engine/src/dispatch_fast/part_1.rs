//! Functions of `dispatch_fast` in a codegen unit of their own (B1: a staged edit recompiles only the units that inline it).
#![allow(unused_imports)]
use super::*;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// The native kind index a typed site fixes (`TYPED_KIND_SITES`), `None` for any other site.
pub(crate) const fn typed_site_kind(site: u8) -> Option<u32> {
    let mut index = 0;
    while index < TYPED_KIND_SITES.len() {
        if TYPED_KIND_SITES[index] == site {
            return Some(index as u32);
        }
        index += 1;
    }
    None
}

/// The typed keyed read or write row of the native kind at `index`, whose site fixes the kind.
pub(crate) const fn element_typed_row(read: bool, index: u32) -> Handler {
    macro_rules! rows {
        ($($index:literal)*) => {
            match (read, index) {
                $((true, $index) => generated_row!(ELEMENT_READ, ACC_NONE, TYPED_KIND_SITES[$index]),
                (false, $index) => generated_row!(ELEMENT_WRITE, ACC_NONE, TYPED_KIND_SITES[$index]),)*
                _ => panic!("no native typed kind at this index"),
            }
        };
    }
    rows!(0 1 2 3 4 5 6 7 8)
}

/// Whether `handler` is a typed keyed read row (`read`) or write row, of any kind (`element_typed_row`).
pub(crate) fn is_typed_element_row(handler: *const (), read: bool) -> bool {
    (0..TYPED_KIND_SITES.len() as u32).any(|index| element_typed_row(read, index) as *const () == handler)
}

/// The opcode of the catalog's named field store of `receiver`, strict or not (`Op::field_store`),
/// whose generated row (`generated_row`) a store record runs: its hint as a layout store.
pub(super) const fn field_store_opcode(receiver: StoreReceiver, strict: bool) -> u16 {
    let mut index = 0;
    loop {
        match Op::from_index(index) {
            Some(op) => match op.field_store() {
                Some(FieldStore { receiver: found, strict: s })
                    if s == strict && matches!((found, receiver), (StoreReceiver::This, StoreReceiver::This)
                        | (StoreReceiver::Register, StoreReceiver::Register)) => break index as u16,
                _ => index += 1,
            },
            None => panic!("the catalog names every field store"),
        }
    }
}

/// The generated own-field store (`generated_row` of the store opcode) of a store record whose
/// receiver and strictness `store` names.
pub(super) const fn field_store_row(store: FieldStore) -> Handler { field_store_row_at::<FLAG_UNREAD>(store) }

/// `field_store_row` at site fact `FLAG`: `GENERAL_SITE` for a store the class's facts do not prove always stored,
/// `TRACKED_SITE` for a store whose field keeps a narrower domain (`named_store_row`).
pub(super) const fn field_store_row_at<const FLAG: u8>(store: FieldStore) -> Handler {
    match (store.receiver, store.strict) {
        (StoreReceiver::Register, false) => generated_row!(FIELD_STORE, ACC_NONE, FLAG),
        (StoreReceiver::Register, true) => generated_row!(FIELD_STORE_STRICT, ACC_NONE, FLAG),
        (StoreReceiver::This, false) => generated_row!(THIS_FIELD_STORE, ACC_NONE, FLAG),
        (StoreReceiver::This, true) => generated_row!(THIS_FIELD_STORE_STRICT, ACC_NONE, FLAG),
    }
}

/// The site fact (`generated_row`'s `FLAG`) of an own-field read of `this` (else of a register) whose
/// hint fixes form `OWN_FORMS[form]`. Zero is no site: the row reads it as unread.
pub(super) const fn own_site(this: bool, form: usize) -> u8 { 1 + (form as u8) * 2 + this as u8 }

pub(crate) const fn length_site(site: u8) -> bool { site == ARRAY_LENGTH_SITE || site == STRING_LENGTH_SITE }

pub(crate) const fn host_read_site(site: u8) -> bool { site == TYPED_LENGTH_SITE }

pub(crate) const fn general_site(site: u8) -> bool { site == GENERAL_THIS || site == GENERAL_REGISTER }

/// The generated general read of a `GetField` by its base: `GENERAL_THIS` for `this`,
/// `GENERAL_REGISTER` for a register, and `row_get_field_general` for a nested or local base.
pub(super) fn general_handler(base: u16) -> Handler {
    if base == quench_runtime::bytecode::FieldBase::THIS.0 {
        generated_row!(FIELD_LEAF_READ, ACC_NONE, GENERAL_THIS)
    } else if quench_runtime::bytecode::FieldBase(base).register_index().is_some() {
        generated_row!(FIELD_LEAF_READ, ACC_NONE, GENERAL_REGISTER)
    } else {
        row_get_field_general
    }
}

/// The VM's general read of `atom` of `object` through cache `site`, as the row's host path
/// (`generated_row`'s `GENERAL_*` answer): a register's proved inherited entry answers first
/// (`proved_inherited_field_hit`), then the caches' probe (`field_step_in_lane`), with `record` the
/// instruction's address for a private name's site. `None` leaves to `row_get_field_general`.
#[inline(always)]
pub(crate) fn general_field_read(context: *mut (), record: usize, register: bool, object: Value, atom: u32,
    site: u16) -> Option<Value> {
    let cx = context.cast::<LaneContext>();
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let quick = if register && vm.specialized_operations_enabled(code) {
        vm.proved_inherited_field_hit(code, object, site)
    } else {
        None
    };
    quick.or_else(|| field_step_in_lane(cx, record, code, object, atom, site))
}

/// Whether site `site` is a field read of a Math call's callee (`MATH_CALL_SITE`, `MATH_CALL2_SITE`,
/// `MATH_CALL2_CONST_SITE`), chosen at load (`fuse_math_calls`).
pub(crate) const fn math_site(site: u8) -> bool {
    site == MATH_CALL_SITE || site == MATH_CALL2_SITE || site == MATH_CALL2_CONST_SITE
}

/// The number of arguments the Math call of math site `site` passes.
pub(crate) const fn math_arity(site: u8) -> usize {
    if site == MATH_CALL_SITE { 1 } else { 2 }
}

/// Whether site `site` is a sinking read (`SINK_SITE`).
pub(crate) const fn sink_site(site: u8) -> bool { site == SINK_SITE }

pub(crate) const fn private_field_site(site: u8) -> bool {
    site == PRIVATE_FIELD_THIS || site == PRIVATE_FIELD_REGISTER
}

pub(crate) const fn returning_site(this: bool, leaf: u8) -> u8 { own_site(this, RETURNING_FORM) + 2 * leaf }

pub(crate) const fn returning_field_site(site: u8) -> bool { site >= RETURNING_BASE }

/// Saturating: a const branch for a site of another family still instantiates the call.
pub(crate) const fn returning_leaf(site: u8) -> u8 { site.saturating_sub(RETURNING_BASE) / 2 }

/// Whether site `site` is a trusted own-field read of a fixed class's cell (`TRUSTED_OWN_*`, `TRUSTED_RUN_*`).
pub(crate) const fn trusted_site(site: u8) -> bool {
    site == TRUSTED_OWN_THIS || site == TRUSTED_OWN_REGISTER || site == TRUSTED_RUN_THIS || site == TRUSTED_RUN_REGISTER || trusted_local_site(site)
}

/// Whether site `site` is a trusted own-field read of a frame local's object (`TRUSTED_LOCAL`), whose base is no register.
pub(crate) const fn trusted_local_site(site: u8) -> bool {
    site == TRUSTED_LOCAL
}

/// Whether site `site` is a nested read of two proven fixed cells (`NESTED_TRUSTED`): the first hop's word is the
/// operand's, the second the record's mode.
pub(crate) const fn nested_trusted_site(site: u8) -> bool { site == NESTED_TRUSTED }

/// Whether own-field or held site `site` reads `this`.
pub(crate) const fn own_site_this(site: u8) -> bool { site.wrapping_sub(1) & 1 == 1 }

/// The `OWN_FORMS` index of the storage form layout id `hint` names (`0`, the hinted form, for none).
/// A narrow run's hint names neither inline values nor extras, so those tests fold away. Record slots are
/// all tagged words (invariant I), so a flat run, extras or inline form decodes nothing.
pub(super) fn own_form_index(hint: Option<u32>) -> usize {
    use quench_runtime::value_vec::{EXTRAS_FORM, INLINE_FORM, NARROW_FORM};
    // The hint is a layout id; its form bits are its entry's (`layouts::auxiliary`).
    match hint.map(quench_runtime::tier_surface::layouts::auxiliary) {
        None => 0,
        Some(hint) if hint & NARROW_FORM != 0 => 2,
        Some(hint) => match hint & (INLINE_FORM | EXTRAS_FORM) {
            0 => 3,
            EXTRAS_FORM => 4,
            INLINE_FORM => 1,
            _ => 5,
        },
    }
}

/// The generated own-field read (`generated_row` at an `own_site`) for the base `THIS` names and the
/// storage form the layout id `hint`'s form bits name (`own_form_index`): an arena run, extras or packed
/// inline values, whose layout it then reads untested; `None` for the form of no particular hint.
pub(super) fn own_formed_handler<const THIS: bool>(hint: Option<u32>) -> Handler {
    macro_rules! row { ($site:expr) => { generated_row!(FIELD_LEAF_READ, ACC_NONE, $site) as Handler } }
    own_sites!(row)[own_form_index(hint) * 2 + usize::from(!THIS)]
}

/// Makes the own field entry `entry` the hint of the field read/store record at `ip`
/// (`LaneInstruction::field_hint`). A held loop header's `call_link` names its site
/// (`header_site_in_lane`), and such a record keeps no hint: an empty hint's link is
/// `NO_CALL_LINK`, a held one's shape a recorded one.
#[inline(always)]
pub(super) fn set_field_hint(ip: Ip, entry: quench_runtime::vm::FieldCache) {
    // A hint holds a `u16` field index, which a spilled slot word does not fit (`Vm::form_hint`): a spilled entry
    // writes the empty hint, so its site keeps reading through the entry (`class_slot_address`).
    if quench_runtime::staged::field_cache_layout::is_spilled_slot(entry.slot) {
        set_record_hint(ip, quench_runtime::vm::NO_FIELD_RECEIVER, NO_CALL_LINK);
        return;
    }
    set_record_hint(ip, entry.receiver, entry.record_slot());
}

/// Whether the field record at `ip` runs a trusted own-field row (`trusted_site`): its operand word holds the field's
/// slot the load chose (`trusted_named_read`), so no hint may overwrite it. Its general misses write no hint either.
#[inline(always)]
pub(super) fn holds_trusted_slot(ip: Ip) -> bool {
    // SAFETY: `ip` is a field read record of a live view.
    holds_trusted_record(unsafe { &*ip })
}

/// `holds_trusted_slot` of the record `record`, whichever way it is addressed.
#[inline(always)]
pub(crate) fn holds_trusted_record(record: &LaneInstruction) -> bool {
    let handler = record.handler;
    handler == generated_row!(FIELD_LEAF_READ, ACC_NONE, TRUSTED_OWN_THIS) as *const ()
        || handler == generated_row!(FIELD_LEAF_READ, ACC_NONE, TRUSTED_OWN_REGISTER) as *const ()
        || handler == generated_row!(FIELD_LEAF_READ, ACC_NONE, TRUSTED_RUN_THIS) as *const ()
        || handler == generated_row!(FIELD_LEAF_READ, ACC_NONE, TRUSTED_RUN_REGISTER) as *const ()
        || handler == generated_row!(FIELD_LEAF_READ, ACC_NONE, TRUSTED_LOCAL) as *const ()
        || handler == generated_row!(FIELD_LEAF_READ, ACC_NONE, RETURNING_TRUSTED_THIS) as *const ()
        || handler == generated_row!(FIELD_LEAF_READ, ACC_NONE, RETURNING_TRUSTED_REGISTER) as *const ()
        || handler == generated_row!(FIELD_LEAF_READ, ACC_NONE, NESTED_TRUSTED) as *const ()
}

/// Writes a field record's hint, `shape` and `link`, unless it is a held loop header (`holds_loop_site`) or a trusted
/// record, whose operand word is its slot (`holds_trusted_slot`).
#[inline(always)]
pub(super) fn set_record_hint(ip: Ip, shape: u32, link: u16) {
    if !holds_loop_site(ip) && !holds_trusted_slot(ip) {
        // SAFETY: as `edge_rewrite`; `ip` is a field read/store record of a live view.
        unsafe {
            (*ip.cast_mut()).operand.field.shape = shape;
            (*ip).call_link.set(link);
        }
    }
}

/// Whether the field record at `ip` is a held loop header's, whose `call_link` names its
/// site (`header_site_in_lane`), so it keeps no hint: an empty hint's link is `NO_CALL_LINK`,
/// and a held one's shape a recorded one.
#[inline(always)]
pub(super) fn holds_loop_site(ip: Ip) -> bool {
    // SAFETY: `ip` is a field read/store record of a live view.
    let i = unsafe { &*ip };
    (unsafe { i.operand.field.shape }) == quench_runtime::vm::NO_FIELD_RECEIVER && i.call_link.get() != NO_CALL_LINK
}

/// The intrinsic geometry getter's value of `atom` on `base` (`Vm::typed_array_geometry_read`)
/// for a geometry name (`AtomClass::VIRTUAL_FIELD`); `None` for any other read.
#[inline(always)]
pub(super) fn geometry_read_in_lane(vm: &mut Vm, base: Value, atom: quench_runtime::bytecode::Atom) -> Option<Value> {
    if !vm.atom_class(atom).contains(quench_runtime::vm::object::AtomClass::VIRTUAL_FIELD) {
        return None;
    }
    vm.typed_array_geometry_read(base, atom)
}

/// The `length` of a receiver that is no array or string (`FieldPlan::Length`): an arguments
/// object's or a user function's own data `length`, else a typed view's intrinsic geometry
/// (`Vm::typed_array_geometry_read_inline`); `None` for any other receiver.
#[inline(always)]
pub(super) fn length_read(vm: &mut Vm, receiver: Value) -> Option<Value> {
    let atom = vm.length_atom;
    vm.arguments_object_length(receiver)
        .or_else(|| vm.plain_function_length(receiver))
        .or_else(|| vm.typed_array_geometry_read_inline(receiver, atom))
}

/// The base of a `GetField` of a register or `this` that a frameless activation makes in
/// place (`row_get_field_beside_activations`): a public name whose site caches no getter,
/// which `getter_activation_in_lane` would run as an activation. `None` for a frame's run, a
/// nested or local base, a private name or such a getter.
#[inline(never)]
pub(super) fn field_read_beside_activations_in_lane(cx: *mut LaneContext, r: Registers, i: &LaneInstruction) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    if !vm.lane_stack.has_frames() || i.b == quench_runtime::bytecode::FieldBase::NESTED || vm.is_private_name(i.imm()) {
        return None;
    }
    let base = unsafe { field_base_in_lane(cx, r, quench_runtime::bytecode::FieldBase(i.b)) }?;
    vm.cached_field_getter(base, i.imm(), vm.field_cache_index(i.c)).is_none().then_some(base)
}

/// A lane `GetField` whose read meets the accessor its site cached
/// (`Vm::cached_field_getter`): the getter runs as a frameless activation of the read's
/// record, as `script_call_prove_miss` runs a call, with the base as receiver and no arguments,
/// and its completion is the read's value. `Err` carries the record to continue at: the
/// general path for any other read.
#[inline(never)]
pub(super) fn getter_activation_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
) -> Result<(Registers, Ip), Ip> {
    // No admission test here: the getter's target is found through the admission word
    // (`lane_script_target_in`, `cross_call_entry`), which refuses a run that calls no script
    // functions or has a construction pending, and a native getter asks it in `read_getter`.
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    // The object the getter's read is of and its site: a nested site's second field is read
    // of the value its first, cached read gives.
    let read = if i.b == quench_runtime::bytecode::FieldBase::NESTED {
        code.field_sites.get(i.imm() as usize).copied().and_then(|site| {
            let base = unsafe { field_base_in_lane(cx, r, site.base) }?;
            match site.second {
                Some((atom, cache)) => {
                    Some((lane_frame::LaneFrame::<ACC_NONE>::cached_read(cx, code, base, site.first.0, site.first.1, 0)?, atom, cache))
                }
                None => Some((base, site.first.0, site.first.1)),
            }
        })
    } else {
        unsafe { field_base_in_lane(cx, r, quench_runtime::bytecode::FieldBase(i.b)) }.map(|base| (base, i.imm(), i.c))
    };
    let vm = unsafe { vm(cx) };
    #[cfg(feature = "profile-lane-exits")]
    if let Some((base, atom, cache)) = read
        && vm.cached_field_getter(base, atom, vm.field_cache_index(cache)).is_none()
    {
        quench_runtime::vm::lane_exits::record_detail(format!(
            "getter decline uncached {}",
            &code.atoms[atom as usize]
        ));
    } else if read.is_none() {
        quench_runtime::vm::lane_exits::record_detail("getter decline base".into());
    }
    let Some((base, getter)) = read.and_then(|(base, atom, cache)| {
        Some((base, vm.cached_field_getter(base, atom, vm.field_cache_index(cache))?))
    }) else {
        return AccessorCall::of(cx).read_get_trap(r, ip, read.map(|(base, atom, _)| (base, atom)));
    };
    AccessorCall::of(cx).read_getter(r, ip, i, getter, base)
}

/// The activation of `getter` with receiver `base` and no arguments for the read record at
/// `ip`, whose completion is the read's value (`resolved`: its frameless target, which the read
/// resolved already): a frameless target of the running program, or
/// another program's plain activation (`Vm::cross_call_entry`), which runs in its program.
/// `Err` carries the record to continue at: the general path for any other getter.
pub(super) fn activate_getter_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    getter: Value,
    base: Value,
    resolved: Option<ScriptCallTarget>,
) -> Result<(Registers, Ip), Ip> {
    // The getter's activation is pushed above the reading function's, and resolving its
    // target may displace the cached target a running leaf names, so a leaf's read makes the
    // leaf an activation first. A target the read resolved (`resolved`) is a copy, which
    // that leaves as it was.
    materialize_running_leaf_in_lane(cx);
    let vm = unsafe { vm(cx) };
    if let Some(target) = resolved.or_else(|| script_target_in_lane(cx, getter)).filter(|target| !target.forwards()) {
        // A getter is an ordinary call: the one call row where it is plain, else the kept activation.
        if let Some(entered) = getters::invoke_getter(cx, r, ip, &target, base) {
            return Ok(entered);
        }
        let Some(this) = call_this_in_lane(vm, &target, base) else {
            return Err(exit(ip));
        };
        return activate_kept_in_lane::<false>(cx, r, ip, &target, &[], this);
    }
    // A getter that makes closures, of this program or another, runs as the environment
    // activation its plain call makes (`activate_environment_in_lane`), with no arguments, as
    // Zod's `get error() { ... issues.map((iss) => ...) }` does.
    // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target or collects.
    let environment = vm
        .script_call_entry(getter)
        .or_else(|| vm.cross_call_entry(getter))
        .map(|target| unsafe { *target })
        .filter(|target| target.entry == CallEntry::framed(Framed::Environment) && !constructing_in_lane(vm, getter));
    if let Some(target) = environment {
        let Some(this) = call_this_in_lane(vm, &target, base) else {
            return Err(exit(ip));
        };
        return activate_environment_in_lane(cx, r, ip, &target, &[], (this, false));
    }
    // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target or collects.
    let Some(target) = vm
        .cross_call_entry(getter)
        .map(|target| unsafe { *target })
        .filter(|target| target.entry.activates() && !constructing_in_lane(vm, getter))
    else {
        #[cfg(feature = "profile-lane-exits")]
        quench_runtime::vm::lane_exits::record_detail(format!(
            "getter decline target cross={:?} constructing={}",
            vm.cross_call_entry(getter).map(|target| unsafe { (*target).entry } as u8),
            constructing_in_lane(vm, getter)
        ));
        return Err(exit(ip));
    };
    let Some(this) = call_this_in_lane(vm, &target, base) else {
        return Err(exit(ip));
    };
    activate_kept_in_lane::<true>(cx, r, ip, &target, &[], this)
}

/// A `GetField` record's value where caches answer it (`LaneFrame::cached_read`): one read of a
/// register or `this`, or a nested site's chain of two. `None` leaves the read to the
/// general path.
///
/// Out of line: the generic reads (`lane_get_field`, `row_get_field_sink`, a returning
/// `GetField`) run it on their slower paths, so one copy serves them all instead of each
/// carrying its own probe (DeltaBlue, PGO build: ~40 instruction-cache lines of warm code).
///
/// # Safety
/// `cx` is the running lane's context and `r` its registers.
#[inline(never)]
pub(super) unsafe fn field_value_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    i: &LaneInstruction,
) -> Option<Value> {
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    // The record's address names its private-name sites (`LaneFrame::private_slot`).
    let record = std::ptr::from_ref(i).addr();
    if i.b == quench_runtime::bytecode::FieldBase::NESTED {
        let site = *code.field_sites.get(i.imm() as usize)?;
        let base = unsafe { field_base_in_lane(cx, r, site.base) }?;
        let value = field_step_in_lane(cx, record + NESTED_STEPS[0], code, base, site.first.0, site.first.1)?;
        return match site.second {
            Some((atom, cache)) => field_step_in_lane(cx, record + NESTED_STEPS[1], code, value, atom, cache),
            None => Some(value),
        };
    }
    let base = unsafe { field_base_in_lane(cx, r, quench_runtime::bytecode::FieldBase(i.b)) }?;
    field_step_in_lane(cx, record, code, base, i.imm(), i.c)
}

/// The object a field base names: a register, or `this` where the lane knows it.
///
/// # Safety
/// As for `field_value_in_lane`.
#[inline(always)]
pub(super) unsafe fn field_base_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    base: quench_runtime::bytecode::FieldBase,
) -> Option<Value> {
    if let Some(slot) = base.local_index() {
        return plain_local_operand(cx, slot);
    }
    match base.register_index() {
        Some(register) => Some(r.get(register)),
        None => unsafe { this_in_lane(cx, r) },
    }
}

/// The general write of a store past its generated row (`GENERAL_SITE`): a private name's store
/// (`lane_private_field_store`), else the public name's store made in place
/// (`row_set_field_in_place`). The row's own cached write already refused it.
#[inline(never)]
pub(super) fn row_set_field_miss_path(cx: *mut LaneContext, r: Registers, ip: Ip, object: Value, i: &LaneInstruction) -> Resumed {
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let vm = unsafe { vm(cx) };
    if vm.is_private_name(i.imm()) {
        if !lane_frame::LaneFrame::<ACC_NONE>::private_store(cx, ip.addr(), code, object, i.imm(), r.get(i.a)) {
            return Resumed::Exit(exit(ip));
        }
        // Adding a property can allocate.
        note_allocation(vm);
        return Resumed::At(r, following(ip), Carried::script(cx));
    }
    row_set_field_in_place(cx, r, ip, object, i)
}

/// `lane_set_field` of a public name past its caches: the general store made in place
/// (`in_place_in_lane`), in the frame's own run or beside frameless activations, strict as its
/// record is. A store that adds a property to receivers of many shapes (zod's schema
/// initializers in the playwright npm scenario: 51K of its 79K lane exits) otherwise left the
/// lane at every store.
#[cold]
#[inline(never)]
pub(super) fn row_set_field_in_place(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    object: Value,
    i: &LaneInstruction,
) -> Resumed {
    // The running function decides the store's strictness: a running leaf's is its
    // materialized activation's.
    materialize_running_leaf_in_lane(cx);
    if let Some(resumed) = AccessorCall::of(cx).store_setter(r, ip, i, object) {
        return resumed;
    }
    let Some(strict) = store_is_strict_in_lane(cx, ip) else {
        return Resumed::Exit(exit(ip));
    };
    let (atom, value, site) = (i.imm(), r.get(i.a), i.c);
    if let Some(resumed) = AccessorCall::of(cx).store_set_trap(r, ip, object, atom, value, strict) {
        return resumed;
    }
    if let Err(next) = unwound_in_place(cx, ip, |vm, code| vm.set_field_cached(code, object, atom, value, site, strict)) {
        return Resumed::Exit(next);
    }
    // Guest code the store ran made the activations frames, whose registers these are.
    let r = unsafe { (*cx).registers };
    Resumed::At(r, following(ip), Carried::script(cx))
}

/// Whether the named field store at the record `ip` rejects a refused write with a
/// `TypeError`, as its opcode says; `None` for any other record.
pub(super) fn store_is_strict_in_lane(cx: *mut LaneContext, ip: Ip) -> Option<bool> {
    field_store_at(cx, ip).map(|store| store.strict)
}

/// The named field store at the record `ip`, as its opcode says; `None` for any other record.
pub(super) fn field_store_at(cx: *mut LaneContext, ip: Ip) -> Option<FieldStore> {
    let vm = unsafe { vm(cx) };
    let function = match vm.lane_stack.top_frame() {
        Some(activation) => activation.function(),
        None => vm.frames[unsafe { (*cx).frame() }].function,
    };
    // SAFETY: the context's program outlives the lane run, and `ip` is a record of the
    // innermost activation's view.
    let (code, pc) = unsafe { (&*(*cx).code, record_pc(cx, ip) as usize) };
    let body = code.function(function as usize);
    match crate::compiled::decoded(body.widened(body.code[pc])) {
        // A store to `this` has no base register (`SetThisField`).
        Some(quench_runtime::staged::Operation::SetField { base, strict, .. }) => Some(FieldStore {
            receiver: if base.is_some() { StoreReceiver::Register } else { StoreReceiver::This },
            strict,
        }),
        _ => None,
    }
}

/// The brand of private name `atom` at the record `ip`, whose home follows the running
/// function and the environment its frame would capture from: the lane's frame, or a
/// frameless activation's function and closure environment (`Vm::site_private_brand`).
#[inline(always)]
pub(super) fn private_brand_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    code: &ResidualProgram,
    atom: quench_runtime::bytecode::Atom,
) -> Option<quench_runtime::tier_surface::PrivateBrand> {
    let (environment, function, frame) = private_site_in_lane(cx)?;
    unsafe { vm(cx) }.site_private_brand(code, ip.addr(), environment, function, frame, atom)
}

/// The running function's private-name scope: the environment its frame, or its activation,
/// captures from, the function, and whether it runs in a frame (`Vm::site_private_brand`).
#[inline(always)]
pub(super) fn private_site_in_lane(cx: *mut LaneContext) -> Option<(Value, u32, bool)> {
    let vm = unsafe { vm(cx) };
    Some(match vm.lane_stack.top_frame() {
        None => {
            // The running frame is the top one, whose home `Vm::private_name_home` finds.
            let frame = vm.frames.len().checked_sub(1)?;
            (vm.captured_parent_environment(vm.frame_caller(frame)), vm.frames[frame].function, true)
        }
        // SAFETY: the context outlives the lane run.
        Some(activation) => (unsafe { (*cx).registers }.env(), activation.script().map(|_| activation.function())?, false),
    })
}

/// One `GetField` step of `field_value_in_lane`: a private name's at its site, any other name's
/// through the field caches (`LaneFrame::cached_read`).
#[inline(always)]
pub(super) fn lane_private_field_slot(
    cx: *mut LaneContext,
    ip: Ip,
    code: &ResidualProgram,
    object: Value,
    atom: quench_runtime::bytecode::Atom,
) -> Option<usize> {
    lane_private_field_slot_at(cx, ip.addr(), code, object, atom)
}

/// `lane_private_field_slot` at site `site`: a record's address, or an address inside it for
/// a nested read's step (`lane_field_value`), which no other record's site can be. A site
/// whose name belongs to a class evaluated once (`PrivateClasses::unique_name`) answers from
/// the receiver's shape alone, before the running function's scope is found.
#[inline(always)]
pub(super) fn lane_private_field_slot_at(
    cx: *mut LaneContext,
    site: usize,
    code: &ResidualProgram,
    object: Value,
    atom: quench_runtime::bytecode::Atom,
) -> Option<usize> {
    {
        let vm = unsafe { vm(cx) };
        if vm.private_classes.unique_name(atom)
            && let Some(slot) = vm.private_site_homes.unique_field_slot(site, vm.heap_reads().object(object)?.shape())
        {
            return Some(slot);
        }
    }
    let scope = private_site_in_lane(cx)?;
    (unsafe { vm(cx) }).site_private_field_slot(code, site, scope, object, atom)
}

/// Makes the slot site `ip`'s entry recorded for an object of `shape` (`lane_private_field_slot`)
/// the hint of its `GetField` or `SetField` record (`lane_get_private_field`,
/// `lane_private_field_store`), where its private name `atom` belongs to a class evaluated
/// once: the entry recorded it under that class's home (`SiteHome::unique`), the site's for
/// good while the name stays unique, in a shape no edit in place changes, and a shape
/// compaction clears both. A record of a site that recorded none keeps its hint. A hit
/// compares the receiver's shape and reads the name's word (`PrivateClasses::unique_name`):
/// a second evaluation of the class, or eval code that sees the name, clears it, which
/// disables the hints of that name's sites alone.
#[inline(never)]
pub(super) fn hint_private_field(vm: &Vm, ip: Ip, atom: quench_runtime::bytecode::Atom, shape: u32, slot: usize) {
    if vm.private_classes.unique_name(atom)
        && vm.private_site_homes.unique_field_slot(ip.addr(), shape) == Some(slot)
        && let Ok(slot) = u16::try_from(slot)
        && slot != NO_CALL_LINK
    {
        set_field_hint(ip, quench_runtime::vm::FieldCache::own(shape, u32::from(slot)));
    }
}

/// One `GetField` step of `lane_field_value`: a private name's at its site, any other name's
/// through the field caches (`lane_cached_field`).
#[inline(always)]
pub(super) fn field_step_in_lane(
    cx: *mut LaneContext,
    site: usize,
    code: &ResidualProgram,
    object: Value,
    atom: quench_runtime::bytecode::Atom,
    cache: u16,
) -> Option<Value> {
    lane_frame::LaneFrame::<ACC_NONE>::cached_read(cx, code, object, atom, cache, site)
}

/// Raise at `ip` after its instruction failed with `error` before any effect, where a handler
/// of the running function or of a plain caller takes it in the lane
/// (`handler_edges::unwind_raised`). A frame run raises `error`; beside frameless activations,
/// whose frames `error`'s stack capture did not read, `again` makes the instruction's error a
/// second time beside them (`handler_edges::raise_again`, frame-switches.md #6).
#[inline(never)]
pub(super) fn raise_in_lane<T>(
    cx: *mut LaneContext,
    ip: Ip,
    error: JsError,
    again: impl FnOnce(&mut Vm, &ResidualProgram) -> Result<T, JsError>,
) -> Ip {
    let vm = unsafe { vm(cx) };
    if vm.lane_stack.has_frames() {
        drop(error);
        return handler_edges::raise_again(cx, ip, again);
    }
    vm.lane_completion.raise(error);
    handler_edges::unwind_raised(cx, ip)
}

/// `DefinePropertyRecord` of the record at `ip`, defined as the general step defines it: a
/// class method or accessor, or an object literal accessor. An out-of-domain mode leaves the
/// instruction to the general path, a throw is raised at the record (`unwound_in_place`):
/// `Err` is the record the run goes on at. Out of line, as `row_index_write`:
/// the handler's frame then holds none of the descriptor.
#[inline(never)]
pub(super) fn define_property_record_in_place_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    i: &LaneInstruction,
) -> Result<(), Ip> {
    let mode = quench_runtime::bytecode::PropertyDefinitionMode::from_word(i.imm()).ok_or(exit(ip))?;
    let (value, target, key) = (i.a, i.b, i.c);
    unwound_in_place(cx, ip, |vm, code| {
        let (value, target, key) = {
            let r = unsafe { (*cx).registers };
            (r.get(value), r.get(target), r.get(key))
        };
        let descriptor =
            quench_runtime::vm::object_descriptors::PropertyDescriptorRecord::for_definition(mode, value);
        vm.define_property_or_throw(code, target, key, descriptor)
    })
}

/// The edge a decided comparison takes: the jump is taken where it fails.
#[inline(always)]
pub(super) const fn holds_edge(holds: bool) -> quench_runtime::staged::Edge {
    if holds { quench_runtime::staged::Edge::Following } else { quench_runtime::staged::Edge::Target }
}

/// The tests the general loop decides past Number comparison without running guest code:
/// identity equality, an ordinary `instanceof`, BigInt order, and the truth of a Number
/// operator's result.
/// Out of line, so the comparison handlers keep their Number path's shape.
#[cold]
#[inline(never)]
pub(super) fn relation_other(operator: u32, left: Value, right: Value) -> Option<bool> {
    let vm = unsafe { vm(context_in_lane()) };
    // A nullish value is unequal to every Number under both loose and strict equality.
    // The Number/nullish pair cannot invoke conversion hooks or `document.all`; keep this
    // check in the shared cold fallback so it does not enlarge each hot comparison handler.
    if operator <= STRICT_INEQUALITY_OPERATOR
        && ((left.is_null() || left.is_undefined()) && right.as_number().is_some()
            || (right.is_null() || right.is_undefined()) && left.as_number().is_some())
    {
        return Some(matches!(operator, 1 | STRICT_INEQUALITY_OPERATOR));
    }
    // An ordering operator (`<`, `<=`, `>`, `>=`) is decided by its operand pair alone: the
    // membership, prototype and identity tests below never answer it.
    // `==` and `===` share `Equal` (`NumberRelation::from_operator`), which is no ordering: they take
    // the identity and string tests below.
    if quench_runtime::vm::operations::NumberRelation::from_operator(operator)
        .is_some_and(|relation| !matches!(relation, quench_runtime::vm::operations::NumberRelation::Equal | quench_runtime::vm::operations::NumberRelation::NotEqual))
    {
        return vm.string_relation(operator, left, right)
            .or_else(|| vm.bigint_relation(operator, left, right))
            .or_else(|| vm.primitive_comparison(operator, left, right))
            .or_else(|| vm.number_binary(operator, left, right)?.truthiness());
    }
    vm.instanceof_truthy(operator, left, right)
        .or_else(|| vm.plain_in(operator, left, right))
        .or_else(|| vm.identity_equality(operator, left, right))
        .or_else(|| vm.string_relation(operator, left, right))
        // A test of an arithmetic or bitwise result (`if (flags & MASK)`): the Number
        // operator's value, whose ToBoolean runs no guest code.
        .or_else(|| vm.bigint_relation(operator, left, right))
        // The remaining primitive pairs, whose conversions are pure: `undefined > 5`,
        // `flag == 1`, `"3" == 3`.
        .or_else(|| vm.primitive_comparison(operator, left, right))
        .or_else(|| vm.number_binary(operator, left, right)?.truthiness())
}

/// A taken jump. Backward jumps, the only ones that close a loop, are
/// safepoints: they leave the lane when a collection is due; forward jumps
/// dispatch directly. Every jump's direction is fixed when the view is
/// derived. Leaving hands the whole instruction to the general path, so
/// `commit`, the register write the instruction makes before its jump,
/// applies only when the lane takes the jump itself.
#[inline(always)]
pub(super) fn take_jump(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    backward: bool,
    view: Carried,
    commit: Option<(u16, Value)>,
) -> Resumed {
    let jump = unsafe { (*ip).jump() };
    // SAFETY: the view records the distance to a validated jump target.
    let target = unsafe { ip.byte_offset(jump) };
    // The backward edge spends fuel through the run's context the handler already holds
    // (`script_fuel_out`): a decrement of `LaneContext::fuel` and a branch, with no
    // thread-local load of the counter's address (`fuel_out`).
    if backward && script_fuel_out(cx) {
        return Resumed::Exit(match commit {
            None => safepoint_at(ip, target, view.acc),
            Some((register, value)) => safepoint_writing(r, ip, target, view.acc, register, value),
        });
    }
    if let Some((register, value)) = commit {
        r.set(register, value);
    }
    Resumed::At(r, target, view)
}

/// Counts an entry of `function`'s lane activation toward its lane body (`own`), or, `now`,
/// specializes it at once, from the targets and callees the program's views hold now. Whether the body
/// is still warming or specialized (`LaneView::specialize_lane_body`).
pub(super) fn specialize_in_lane(cx: *mut LaneContext, own: &LaneView, function: u32, parameters: &[Value]) -> bool {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    // Where the body comes from is the specializer's decision (`admission::route_for_lane`): an ahead image (the
    // ladder's first body, no warm-up), the stencil
    // tier at once, a wait for callees still compiling, or the lane body tier.
    let ahead_image = code.kind != quench_runtime::bytecode::ProgramKind::Wasm && vm.programs.ahead_images(unsafe { (*cx).program }, code).is_some();
    let pick = vm.lane_pick(code, function);
    if crate::compiled::admission::route_for_lane(crate::compiled::admission::LaneRouteFacts { ahead_image, pick, callees_compiling: false })
        == crate::compiled::admission::LaneRoute::Ahead
    {
        // An image body takes a claimed prologue, whichever entry reached it first: its entry reads the function there.
        if let Some(body) = code.function_get(function as usize) && unsafe { (*own.prologue.get()).handler }.is_null() { own.claim_for_tier(tier_prologue_row(), function, body.params, !body.is_arrow); }
        let published = own.entry_published();
        let installed = own.install_ahead_lane_body(body_entry_row(), || vm.install_ahead_body(crate::compiled::admission::ahead_unit(), code, function, None));
        if !published && own.entry_published() {
            unsafe { super::vm(cx) }.unprove_view_entries(own);
        }
        return installed;
    }
    // The JIT's routes link only into a binary whose runtimes it serves (`jit_routes`): without
    // them, a function with no ahead image declines its lane body, as under `--no-jit`.
    let jit = crate::jit_routes::jit().map(|routes| routes.lane);
    let specialize = |work: crate::compiled::LaneBodyWork| jit.map_or(crate::compiled::LaneBodyOutcome::Declined, |jit| jit(cx, own, function, parameters, work));
    let published = own.entry_published();
    let warming = own.specialize_lane_body(body_entry_row(), quench_runtime::vm::tier_policy::lane_body_tier(pick), specialize);
    // Every cell body is published here (`tier_machine::publish` from `specialized`): the
    // proved calls of the view, which skip the cell body, lose their proof.
    if !published && own.entry_published() {
        unsafe { super::vm(cx) }.unprove_view_entries(own);
    }
    warming
}

/// Works toward `function`'s lane body on the JIT's routes: the stencil tier, a wait for callees
/// still compiling, or the lane body tier (`admission::route_for_lane`).
pub(crate) fn jit_lane_body(cx: *mut LaneContext, own: &LaneView, function: u32, parameters: &[Value], work: crate::compiled::LaneBodyWork<'_>) -> crate::compiled::LaneBodyOutcome {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let diagnostics = crate::compiled::admission::diagnosing(vm);
    let direct = |vm: &mut Vm| code.function_get(function as usize).map(|body| direct_callees_in_lane(vm, own, function, code, body)).unwrap_or_default();
    // A start waits for callees still compiling (`body_tier_in_lane::callees_compiling`), calling their bodies by their entries.
    let starting = matches!(work, crate::compiled::LaneBodyWork::Start { .. });
    let callees_compiling = starting && body_tier_in_lane::callees_compiling(vm, own, function, code.function_get(function as usize));
    let pick = vm.lane_pick(code, function);
    match crate::compiled::admission::route_for_lane(crate::compiled::admission::LaneRouteFacts { ahead_image: false, pick, callees_compiling }) {
        // The interpreter's pick compiles nothing: the interpreter runs the function.
        crate::compiled::admission::LaneRoute::Interpreted => return crate::compiled::LaneBodyOutcome::Unpaid,
        // A stencil pick stitches its body at once: it calls the callees its sites recorded by their entries and waits for none.
        crate::compiled::admission::LaneRoute::Base => {
            let crate::compiled::LaneBodyWork::Start { .. } = &work else { unreachable!("a stencil body never compiles in the background") };
            let unit = match vm.base_unit_or_outcome(code, function, parameters) { Ok(unit) => unit, Err(outcome) => return outcome };
            return vm.base_body_work(unit, code, function, parameters, own, &direct);
        }
        crate::compiled::admission::LaneRoute::Deferred => return crate::compiled::LaneBodyOutcome::Deferred,
        crate::compiled::admission::LaneRoute::Ahead => unreachable!("an ahead image's program returned above"),
        crate::compiled::admission::LaneRoute::Lane => {}
    }
    let outcome = vm.body_work_in_lane(code, function, &|callee| callee_captures_in_lane(cx, callee), &direct, work, None);
    if diagnostics && let crate::compiled::LaneBodyOutcome::Published(method) = &outcome {
        if let Some(method) = method.lease() { own.tier().body_stats.compiled(function, &method.inlined); }
    }
    outcome
}

/// Gives a loop site that will not enter native code again its plain backward jump back,
/// so each iteration takes the jump without the site's checks.
pub(super) fn loop_retire_in_lane(cx: *mut LaneContext, ip: Ip, site: &LoopSite) {
    // A hot loop no region runs makes its function worth a body past the entry warm-up: its
    // calls have run, so their sites have linked, and its next entry runs the loop natively.
    // SAFETY: the program's views outlive the lane run.
    if crate::compiled::admission::lane_bodies_enrolled(unsafe { vm(cx) })
        && let Some(own) = unsafe { &*(*cx).views }.get(site.function as usize).and_then(quench_runtime::vm::program_store::derived_lane_view)
    {
        specialize_in_lane(cx, own, site.function, &[]);
    }
    // SAFETY: as `edge_rewrite`: the record lives in an `UnsafeCell` of the running view, and
    // the jump reads its distance from the operand, which the site kept (`LoopSite::jump`).
    let header = unsafe { ip.byte_offset(site.jump) };
    unsafe { (*ip.cast_mut()).operand = LaneOperand { jump: site.jump }; }
    edge_rewrite(ip, jump_row::<JUMP>(true));
    // The site kept the update before the jump from fusing with it (`install_loop_sites`
    // runs before `fuse_rows`): the plain jump takes the loader's pair fusion now, with the
    // `FUSED_TARGET` record where the header's target run holds (`update_fusion`'s `target`).
    if site.backedge > site.header {
        // The header keeps the compare-jump form the load gave it (`specialize_compare_jumps`); it is not re-formed
        // here. A formed test is an instance of the comparison jump row (`compare_instances`), which the target run
        // tests as it tests the plain row.
        // SAFETY: the record before the backedge is the loop body's, in the same view as the jump.
        let update = unsafe { ip.sub(1) };
        if let Some((fused, mode)) = update_fusion(unsafe { &*update }, unsafe { &*ip }, Some(header)) {
            // SAFETY: the record is the running view's, on this thread; its handler is requickened next.
            unsafe { (*update.cast_mut()).mode = mode };
            edge_rewrite(update, int_steps::retired_step(cx, site, update, unsafe { &*header }, fused));
        }
    }
}

/// Whether the running activation's register storage holds the windows of the callees `calls` inlines,
/// which start past the function's environment window (`base_region::function::inline_base`): a lane
/// activation's value stack has the room `room_admits_in_lane` proved (`CallCode::recovery_space` covers
/// the reach); a VM frame's register file, sized to its own words, needs that much capacity past its
/// registers (`INLINED_WINDOW_ROOM`, which an environment window reserves).
#[inline(always)]
pub(super) fn frame_holds_window_in_lane(cx: *mut LaneContext, calls: &crate::compiled::CallCode) -> bool {
    let vm = unsafe { vm(cx) };
    if vm.lane_stack.has_frames() {
        return true;
    }
    frame_reaches_in_lane(cx, calls.caller_registers(), calls.window())
}

/// Whether the running VM frame's register file holds `reach` words past its `registers`, where code
/// writes past them (`base_region::function::inline_base`); a lane activation's storage, at the top of
/// the value stack, is proved by its room instead (`room_admits_in_lane`).
#[inline(always)]
pub(super) fn frame_reaches_in_lane(cx: *mut LaneContext, registers: u16, reach: usize) -> bool {
    let vm = unsafe { vm(cx) };
    reach == 0 || vm.lane_stack.has_frames()
        || vm.frames.get(unsafe { (*cx).frame() }).is_some_and(|frame| frame.registers.capacity() >= usize::from(registers) + reach)
}

/// Whether the running activation keeps its locals in the window past its registers, as
/// `body_locals` finds them for an OSR region (`LocalsOf::Osr`): an environment activation does, and so does a VM frame whose
/// own storage does not cover the function.
/// # Safety
/// `context` is the running lane's context.
pub(crate) unsafe fn context_locals_in_window_in_lane(context: usize) -> bool {
    let cx = context as *mut LaneContext;
    let vm = unsafe { vm(cx) };
    if let Some(activation) = vm.lane_stack.top_frame() {
        return activation.script().is_some_and(|script| script.environment);
    }
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    // A function none of whose records addresses a local (`CallPlan::addresses_locals`) keeps
    // none anywhere: its frame holds none, and neither its records nor its bodies read any.
    vm.frames.get(unsafe { (*cx).frame() }).is_some_and(|frame| code.function_get(frame.function as usize)
        .is_some_and(|body| frame.locals.len() < usize::from(body.locals) && body.call_plan(code).addresses_locals))
}
