//! A plain call of a leaf target: its records run on a window of the lane stack without an
//! activation (`CallEntry::leaf`), and its return completes the call.
use super::*;

script_handler! {
    /// A plain call of a leaf target of form `LEAF` (`CallEntry::leaf`): the callee's records
    /// run on a window at the lane stack's top, the call's facts held by `LaneContext::leaf`
    /// instead of an activation until its return completes the call (`complete_leaf_in_lane`)
    /// or anything observing activations makes it one (`materialize_leaf_in_lane`). The window
    /// and the stack room are those its activation would take, so the guest-call budget and
    /// a full stack decide as they would. A call of this leaf form the site does not make (a
    /// receiver whose binding reads the heap, values past the parameters, no room) takes
    /// `script_call_full_miss`, which activates a leaf as any registers-only target. `ARGC`
    /// is the record's argument count (`leaf_call_handler`), which a call returning its
    /// completion never has.
    ///
    /// The proved form (leaf-keeping-c.md, as link-proofs.md's `script_call_miss`): the entry
    /// the link names, masked to the cache, is tested whole by one admission compare (the
    /// generation, the program, the run's conditions, `DIAGNOSED`, and the proof `UNPROVEN`,
    /// which `script_call_leaf_prove_miss` clears for a leaf entry whose cell body is
    /// unpublished), the callee's identity, the entry's form and the site's argument count
    /// against its parameters. The room is one compare (`LaneStack::plain_room`). Anything
    /// else takes `script_call_leaf_prove_miss`, which makes the call as before.
    pub(super) fn script_call_leaf_miss<LEAF: u8, ARGC: u16>(cx, r, ip, i, view) {
        debug_assert!(unsafe { (*cx).script_calls || !vm(cx).lane_stack.script_calls_allowed() });
        let vm = unsafe { vm(cx) };
        let callee = r.get(i.b);
        let entries = &vm.lane_stack.script_targets.entries;
        let index = usize::from(i.call_link.get()) & (entries.len() - 1);
        // SAFETY: the cache's length is a power of two, so the masked index lies within it;
        // nothing in the leaf resolves a target or collects.
        let target = unsafe { &*std::ptr::from_ref(entries.get_unchecked(index)) };
        if target.callee != callee
            || target.admission != vm.lane_stack.admission
            || target.entry != CallEntry::leaf(LEAF)
            || !quench_runtime::staged::call::concrete_fits(u64::from(target.params), u64::from(ARGC))
        {
            sibling!(script_call_leaf_prove_miss::<LEAF, ARGC>, r, ip, Carried::script(cx))
        }
        debug_assert!(!vm.programs.js_jit_diagnostics_enabled());
        // SAFETY: the target's view lives with its program.
        debug_assert!(!unsafe { (*target.view).entry_published() }, "publishing unproves the view's entries");
        debug_assert!(!crosses_in_lane::<true>(cx, target));
        debug_assert!(!vm.with_scope_entered);
        let receiver = r.get(i.c);
        let this = if leaf_binds_this(LEAF) {
            direct_call_this_in_lane(vm, target, receiver)
        } else {
            binds_directly_in_lane(target, receiver).then_some(receiver)
        };
        let Some(this) = this else {
            sibling!(script_call_full_miss, r, ip, Carried::script(cx))
        };
        // The activation count and the values' room in one compare, the depth loaded only by
        // the exact test past it; a leaf pushes no plain activation, so the bound stands.
        debug_assert_eq!(vm.lane_stack.plain_room(vm.lane_stack.plain.depth.get(), target.room), activation_room_in_lane(vm, target));
        if !vm.lane_stack.plain_room(vm.lane_stack.plain.depth.get(), target.room) {
            sibling!(script_call_full_miss, r, ip, Carried::script(cx))
        }
        // SAFETY: as `script_call_leaf_prove_miss`'s.
        let (registers, base) = unsafe { lane_run_leaf::<LEAF, ARGC>(cx, r, ip, i, target, this) };
        next!(cx, registers, base, Carried::script(cx))
    }
}

script_handler! {
    /// `script_call_leaf_miss` of a site whose link is not proved: the target it links, whose
    /// entry, where it is this form's leaf, is proved (`Admission::UNPROVEN` cleared) unless the
    /// tier's diagnostics count entries or its cell holds a body; then the call as the unproved
    /// leaf call always made it. A plain activation's entry takes it in place, another leaf
    /// form's call its handler, any other the full form.
    #[cold]
    #[inline(never)]
    fn script_call_leaf_prove_miss<LEAF: u8, ARGC: u16>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let callee = r.get(i.b);
        // A pending construction refuses every cached target (`LaneStack::set_refused`).
        // `linked_script_call_entry` names no entry while the run calls no script functions
        // (`Admission::NO_SCRIPT_CALLS` fails the link, `relink_site_closure` refuses a new one).
        let target = vm.linked_script_call_entry(callee, &i.call_link);
        debug_assert!(unsafe { (*cx).script_calls } || target.is_none());
        // SAFETY: as `Vm::script_call_entry`; nothing in the leaf resolves a target or collects.
        let Some(target) = target.map(|target| unsafe { &*target }) else {
            sibling!(script_call_full_miss, r, ip, Carried::script(cx))
        };
        // A site meeting other callees too calls each as its own form would: a plain
        // activation's in place, another leaf form's through it, any other through the
        // full form.
        if target.entry != CallEntry::leaf(LEAF) {
            if target.entry == CallEntry::Activates {
                lane_plain_activation!(cx, vm, r, ip, i, target, ARGC, false)
            }
            sibling!(other_leaf_form::<ARGC>(target.entry), r, ip, Carried::script(cx))
        }
        // The proof: a leaf entry activates as `Activates` (`CallEntry::activates`), so a proved
        // plain call that lands on it may activate it too; neither proves a published body's.
        // SAFETY: the target's view lives with its program.
        if !vm.programs.js_jit_diagnostics_enabled() && !unsafe { (*target.view).entry_published() } {
            let (index, resolved) = (link_index(i.call_link.get()), std::ptr::from_ref(target));
            let entry = &mut vm.lane_stack.script_targets.entries[index];
            debug_assert!(std::ptr::eq(entry, resolved), "the link names the entry it resolved");
            entry.admission.0 &= !Admission::UNPROVEN;
            debug_assert!(entry.admission == vm.lane_stack.admission, "an admitted entry's word is the lane's");
        }
        // Only a leaf reading its receiver binds it here; any other binds it if it
        // materializes (`materialize_leaf_in_lane`), which a binding past the receiver's own
        // bits could not.
        let receiver = r.get(i.c);
        let this = if leaf_binds_this(LEAF) {
            direct_call_this_in_lane(vm, target, receiver)
        } else {
            binds_directly_in_lane(target, receiver).then_some(receiver)
        };
        let Some(this) = this else {
            sibling!(script_call_full_miss, r, ip, Carried::script(cx))
        };
        // The cached target is of the running program (`Vm::script_call_holds`).
        debug_assert!(!crosses_in_lane::<true>(cx, target));
        // An admitted target runs under no `with` scope (`LaneStack::refuse_cached_calls`).
        debug_assert!(!vm.with_scope_entered);
        if !admits_window_in_lane(vm, target, ARGC) {
            sibling!(script_call_full_miss, r, ip, Carried::script(cx))
        }
        vm.programs.record_js_jit_lane_entry(target.program(), target.function);
        // SAFETY: the target is the running program's admitted leaf of form `LEAF`, whose
        // parameters take `ARGC` values and whose window the stack holds.
        let (registers, base) = unsafe { lane_run_leaf::<LEAF, ARGC>(cx, r, ip, i, target, this) };
        next!(cx, registers, base, Carried::script(cx))
    }
}

/// The leaf call of `target` by the call record `i` at `ip` past its admission: its window at
/// the lane stack's top filled and the leaf record entered: the callee's registers and first
/// record, which the handler dispatches.
///
/// # Safety
/// `cx` is the running lane's context; `target` is the running program's leaf of form `LEAF`,
/// cached, whose parameters take `ARGC` values and whose activation's room the stack holds;
/// `this` is the receiver its form binds.
#[inline(always)]
pub(super) unsafe fn lane_run_leaf<const LEAF: u8, const ARGC: u16>(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    target: &ScriptCallTarget,
    this: Value,
) -> (Registers, Ip) {
    let vm = unsafe { vm(cx) };
    // SAFETY: as `push_passed_in_lane`'s window, which the stack's length covers only once
    // the leaf is materialized: no collection runs before.
    let registers = unsafe {
        let registers = vm.lane_stack.values.top();
        fill_admitted_window(registers, usize::from(target.fill), usize::from(target.registers));
        copy_arguments(registers, r.0.add(usize::from(ImmediateLayout::call_window_base(i.imm()))), ARGC);
        Registers(registers)
    };
    // SAFETY: the program store keeps a derived view alive with its program; the tier
    // claims no leaf's view, so its activation starts at its first record.
    let base = unsafe { (*target.view).records() };
    // SAFETY: the running lane's context; the window is the callee's.
    unsafe {
        lane_enter_leaf(cx, registers, ip, r, target, LEAF, ARGC, this, None);
        (*cx).registers = registers;
        (*cx).base = base;
    }
    (registers, base)
}

/// The handler of a site meeting another leaf form's callee: `entry`'s leaf form at the record's
/// argument count (`leaf_arm`), or `script_call_full_miss` for no leaf, out of line: a site
/// meeting another form's callee takes it from the leaf it was made for.
#[cold]
#[inline(never)]
fn other_leaf_form<const ARGC: u16>(entry: CallEntry) -> Handler {
    leaf_arm(entry.leaf_form(), ARGC)
}

/// The protocol row of a leaf call of `leaf` form and argument count `argc` (the `Call` record's
/// `LEAF_CALL` miss is `script_call_leaf_miss` of the same form and count, `leaf_call_miss_of`), or,
/// for no leaf or an argument count past `FIXED_ARGUMENT_COPIES`, `script_call_full_miss`.
fn leaf_arm(leaf: LeafForm, argc: u16) -> Handler {
    use protocol_misses::leaf_call::leaf_parameters;
    macro_rules! leaf {
        ($form:ident) => {
            match argc {
                0 => protocol_handler::<{ quench_runtime::bytecode::CALL_ROW as u16 }, ACC_NONE, { protocol_family::LEAF_CALL | leaf_parameters($form, 0) as u16 }>(),
                1 => protocol_handler::<{ quench_runtime::bytecode::CALL_ROW as u16 }, ACC_NONE, { protocol_family::LEAF_CALL | leaf_parameters($form, 1) as u16 }>(),
                2 => protocol_handler::<{ quench_runtime::bytecode::CALL_ROW as u16 }, ACC_NONE, { protocol_family::LEAF_CALL | leaf_parameters($form, 2) as u16 }>(),
                3 => protocol_handler::<{ quench_runtime::bytecode::CALL_ROW as u16 }, ACC_NONE, { protocol_family::LEAF_CALL | leaf_parameters($form, 3) as u16 }>(),
                4 => protocol_handler::<{ quench_runtime::bytecode::CALL_ROW as u16 }, ACC_NONE, { protocol_family::LEAF_CALL | leaf_parameters($form, 4) as u16 }>(),
                _ => call_row::<{ protocol_family::SCRIPT_CALL | protocol_misses::script_call::FULL as u16 }>(),
            }
        };
    }
    match leaf {
        PLAIN_LEAF => leaf!(PLAIN_LEAF),
        CAPTURE_LEAF => leaf!(CAPTURE_LEAF),
        CONTEXT_LEAF => leaf!(CONTEXT_LEAF),
        THIS_LEAF => leaf!(THIS_LEAF),
        _ => call_row::<{ protocol_family::SCRIPT_CALL | protocol_misses::script_call::FULL as u16 }>(),
    }
}

/// The form a call record `i` whose linked target is a leaf (`entry`) calls it with: the
/// protocol row of its entry and argument count, whose `LEAF_CALL` miss is the
/// `script_call_leaf_miss` instantiation of the same form and count (`leaf_call_miss_of`), or,
/// for a record past `FIXED_ARGUMENT_COPIES` or returning its completion, which completes its
/// caller too, the row whose miss is `script_call_full_miss`, which activates the leaf.
pub(super) fn leaf_call_handler(entry: CallEntry, i: &LaneInstruction) -> Handler {
    const _: () = assert!(FIXED_ARGUMENT_COPIES == 4);
    if i.a & quench_runtime::bytecode::RETURN_REGISTER != 0 {
        return call_row::<{ protocol_family::SCRIPT_CALL | protocol_misses::script_call::FULL as u16 }>();
    }
    leaf_arm(entry.leaf_form(), ImmediateLayout::argument_count(i.imm()))
}

/// Whether `direct_call_this_in_lane` binds `receiver` for `target`, without the binding: any
/// binding but a sloppy function's, and its of an object in the direct record table, `null`
/// or `undefined`, the commonest receiver of a plain call, tested first.
/// The staged row (`call_leaf::guard_receiver_binds`) on machine words.
#[inline(always)]
fn binds_directly_in_lane(target: &ScriptCallTarget, receiver: Value) -> bool {
    quench_runtime::staged::call_leaf::guard_receiver_binds(
        &mut quench_runtime::staged::Concrete::new(std::ptr::null()), receiver.to_bits(), target.this_binding == ThisBinding::Sloppy, (),
    )
    .is_some()
}

/// Make the call record `call` of `caller`'s registers the running leaf of `target` in `form`
/// with `argc` values on `window` (`LaneContext::leaf`): the staged leaf row
/// (`call_leaf::enter_leaf`) on machine words writes the leaf record and the window's context
/// words its form makes the callee's (`this` where it binds its receiver, the target's
/// environment and captures where it swaps them); `caller_program` is the program a crossing
/// leaf's return enters again. The context's registers and records stay the caller's.
///
/// # Safety
/// `cx` is the running lane's context, and `window` the callee's window with its context words
/// below it.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn lane_enter_leaf(
    cx: *mut LaneContext,
    window: Registers,
    call: Ip,
    caller: Registers,
    target: &ScriptCallTarget,
    form: LeafForm,
    argc: u16,
    this: Value,
    caller_program: Option<ProgramId>,
) {
    use quench_runtime::staged::call_leaf::{enter_leaf, LeafContext, LeafEntry};
    // SAFETY: the function's contract.
    unsafe {
        let entry = LeafEntry {
            call: call.expose_provenance() as u64,
            caller_registers: caller.0.expose_provenance() as u64,
            caller_base: (*cx).base.expose_provenance() as u64,
            target: std::ptr::from_ref(target).expose_provenance() as u64,
            argc: u64::from(argc),
            form,
        };
        let context = LeafContext { this: this.to_bits(), env: target.env.to_bits(), capture: target.capture.expose_provenance() as u64 };
        let leaf = std::ptr::addr_of_mut!((*cx).leaf);
        enter_leaf(
            &mut quench_runtime::staged::Concrete::new(std::ptr::null()), leaf.expose_provenance() as u64, window.0.expose_provenance() as u64,
            entry, context,
        );
        (*leaf).caller_program = caller_program;
    }
}

/// The return of the running leaf with `value` (`LaneContext::leaf`): the caller's registers,
/// records and context again, and the call's result in its register, as the plain
/// activation's return makes them (`return_plain_in_lane`). `None` while no leaf runs.
#[inline(always)]
pub(super) fn complete_leaf_in_lane(cx: *mut LaneContext, value: Value, form: LeafForm) -> Option<(Registers, Ip)> {
    // SAFETY: the running lane's context.
    let leaf = unsafe { std::ptr::addr_of_mut!((*cx).leaf) };
    let caller = leave_leaf_in_lane(leaf)?;
    if let Some(program) = unsafe { (*leaf).caller_program.take() } {
        std::hint::cold_path();
        enter_program_in_lane(cx, program);
    }
    // SAFETY: `caller` is the leaf's, just completed.
    Some(unsafe { resume_leaf_caller_in_lane(cx, leaf, caller, value, form) })
}

/// `complete_leaf_in_lane` of a running leaf of the caller's own program, the common leaf return
/// as one op: it makes no call, so a returning handler keeps no frame. `None`, which changes
/// nothing, where no leaf runs or the leaf crosses programs (`caller_program`), which
/// `complete_leaf_in_lane` completes.
#[inline(always)]
pub(super) fn lane_return_leaf_header(cx: *mut LaneContext, value: Value, form: LeafForm) -> Option<(Registers, Ip)> {
    // SAFETY: the running lane's context.
    let leaf = unsafe { std::ptr::addr_of_mut!((*cx).leaf) };
    if unsafe { (*leaf).caller_program }.is_some() {
        return None;
    }
    let caller = leave_leaf_in_lane(leaf)?;
    // SAFETY: `caller` is the leaf's, just completed.
    Some(unsafe { resume_leaf_caller_in_lane(cx, leaf, caller, value, form) })
}

/// The staged row (`call_leaf::complete_leaf`) on machine words: the running leaf's caller,
/// its call word cleared; `None`, which changes nothing, while no leaf runs.
#[inline(always)]
fn leave_leaf_in_lane(leaf: *mut LeafCall) -> Option<quench_runtime::staged::call_leaf::LeafCaller<u64>> {
    quench_runtime::staged::call_leaf::complete_leaf(&mut quench_runtime::staged::Concrete::new(std::ptr::null()), leaf.expose_provenance() as u64, ())
}

/// The caller of a completed leaf resumed: its registers and records the context's again and
/// `value` in the call's result register; where it continues.
///
/// # Safety
/// `caller` is what `leave_leaf_in_lane` returned for the context's `leaf` just now.
#[inline(always)]
unsafe fn resume_leaf_caller_in_lane(
    cx: *mut LaneContext,
    leaf: *mut LeafCall,
    caller: quench_runtime::staged::call_leaf::LeafCaller<u64>,
    value: Value,
    form: LeafForm,
) -> (Registers, Ip) {
    let call: Ip = std::ptr::with_exposed_provenance(caller.call as usize);
    let registers = Registers(std::ptr::with_exposed_provenance_mut(caller.registers as usize));
    let base: Ip = std::ptr::with_exposed_provenance(caller.base as usize);
    // SAFETY: the call record belongs to the caller's live view; the target stays cached.
    unsafe {
        debug_assert!((*leaf).form == form);
        (*cx).registers = registers;
        (*cx).base = base;
        registers.set((*call).a & quench_runtime::bytecode::REGISTER_MASK, value);
    }
    (registers, following(call))
}

/// `materialize_leaf_in_lane` of the running leaf, if one runs: where a leaf's records reach
/// what observes activations or frames (the lane's exit, a getter's activation, an operation
/// made in place), a leaf is the activation its call would have pushed. Its records reach
/// nothing else that does (`leaf_body`).
#[inline(always)]
pub(super) fn materialize_running_leaf_in_lane(cx: *mut LaneContext) {
    if !unsafe { (*cx).leaf.call }.is_null() {
        materialize_leaf_in_lane(cx);
    }
}

/// A plain call record `i` of `target`, another program's leaf (`CallEntry::leaf`), run as
/// `script_call_leaf_miss` runs its own program's: its records on a window at the lane
/// stack's top without an activation, in its program, which its return leaves
/// (`complete_leaf_in_lane`) and its materialization records as a crossing activation's
/// (`Completion::Cross`). `None`, which changes nothing, for a call the leaf form does not
/// make (more values than `FIXED_ARGUMENT_COPIES` or past the parameters, a receiver whose
/// binding reads the heap, no room, a call returning its completion, refused cached calls).
#[inline(always)]
pub(super) fn cross_leaf_call_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    target: &ScriptCallTarget,
) -> Option<(Registers, Ip)> {
    let form = target.entry.leaf_form();
    let argc = ImmediateLayout::argument_count(i.imm());
    if form == NOT_LEAF || argc > FIXED_ARGUMENT_COPIES || i.a & quench_runtime::bytecode::RETURN_REGISTER != 0 {
        return None;
    }
    let vm = unsafe { vm(cx) };
    if vm.with_scope_entered || vm.lane_stack.refuses() || !unsafe { (*cx).leaf.call }.is_null() {
        return None;
    }
    let receiver = r.get(i.c);
    let this = if leaf_binds_this(form) {
        direct_call_this_in_lane(vm, target, receiver)?
    } else {
        binds_directly_in_lane(target, receiver).then_some(receiver)?
    };
    if !admits_window_in_lane(vm, target, argc) {
        return None;
    }
    vm.programs.record_js_jit_lane_entry(target.program(), target.function);
    // SAFETY: as `script_call_leaf_miss`'s window.
    let registers = unsafe {
        let registers = vm.lane_stack.values.top();
        fill_admitted_window(registers, usize::from(target.fill), usize::from(target.registers));
        copy_arguments(registers, r.0.add(usize::from(ImmediateLayout::call_window_base(i.imm()))), argc);
        Registers(registers)
    };
    // SAFETY: as `script_call_leaf_miss`'s first record.
    let base = unsafe { (*target.view).records() };
    // SAFETY: as `script_call_leaf_miss`'s leaf.
    unsafe {
        let caller_program = (*cx).program;
        lane_enter_leaf(cx, registers, ip, r, target, form, argc, this, Some(caller_program));
        enter_program_in_lane(cx, target.program());
        (*cx).registers = registers;
        (*cx).base = base;
    }
    Some((registers, base))
}
