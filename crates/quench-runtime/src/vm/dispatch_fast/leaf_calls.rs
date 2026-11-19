//! A plain call of a leaf target: its records run on a window of the lane stack without an
//! activation (`CallEntry::leaf`), and its return completes the call.
use super::*;

script_handler! {
    /// A plain call of a leaf target of form `LEAF` (`CallEntry::leaf`): the callee's records
    /// run on a window at the lane stack's top, the call's facts held by `LaneContext::leaf`
    /// instead of an activation until its return completes the call (`lane_complete_leaf`)
    /// or anything observing activations makes it one (`materialize_leaf_in_lane`). The window
    /// and the stack room are those its activation would take, so the guest-call budget and
    /// a full stack decide as they would. A call of this leaf form the site does not make (a
    /// receiver whose binding reads the heap, values past the parameters, no room) takes
    /// `lane_script_call_full`, which activates a leaf as any registers-only target. `ARGC`
    /// is the record's argument count (`leaf_call_handler`), which a call returning its
    /// completion never has.
    fn lane_script_call_leaf<LEAF: u8, ARGC: u16>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let callee = r.get(i.b);
        // A pending construction refuses every cached target (`LaneStack::set_refused`).
        let target = if unsafe { (*cx).script_calls } {
            vm.linked_script_call_entry(callee, &i.call_link)
        } else {
            None
        };
        // SAFETY: as `Vm::script_call_entry`; nothing in the leaf resolves a target or collects.
        let Some(target) = target.map(|target| unsafe { &*target }) else {
            sibling!(lane_script_call_full::<H>, r, ip, Carried::script(cx))
        };
        // A site meeting other callees too calls each as its own form would: a plain
        // activation's in place, another leaf form's through it, any other through the
        // full form.
        if target.entry != CallEntry::leaf(LEAF) {
            std::hint::cold_path();
            if target.entry == CallEntry::Activates {
                lane_plain_activation!(cx, vm, r, ip, i, target, ARGC, false)
            }
            sibling!(other_leaf_form::<H, ARGC>(target.entry), r, ip, Carried::script(cx))
        }
        // Only a leaf reading its receiver binds it here; any other binds it if it
        // materializes (`materialize_leaf_in_lane`), which a binding past the receiver's own
        // bits could not.
        let receiver = r.get(i.c);
        let this = if leaf_binds_this(LEAF) {
            direct_call_this_in_lane(vm, target, receiver)
        } else {
            lane_binds_directly(target, receiver).then_some(receiver)
        };
        let Some(this) = this else {
            sibling!(lane_script_call_full::<H>, r, ip, Carried::script(cx))
        };
        // The cached target is of the running program (`Vm::script_call_holds`).
        debug_assert!(!crosses_in_lane::<H, true>(cx, target));
        // An admitted target runs under no `with` scope (`LaneStack::refuse_cached_calls`).
        debug_assert!(!vm.with_scope_entered);
        if !lane_admits_window(vm, target, ARGC) {
            sibling!(lane_script_call_full::<H>, r, ip, Carried::script(cx))
        }
        vm.programs.record_js_jit_lane_entry(target.program(), target.function);
        let stack: &mut LaneStack = &mut vm.lane_stack;
        // SAFETY: as `push_passed_in_lane`'s window, which the stack's length covers only once
        // the leaf is materialized: no collection runs before.
        let registers = unsafe {
            let registers = stack.values.top();
            fill_admitted_window(registers, usize::from(target.fill), usize::from(target.registers));
            copy_arguments(registers, r.0.add(usize::from(ImmediateLayout::call_window_base(i.imm()))), ARGC);
            Registers(registers)
        };
        if leaf_binds_this(LEAF) {
            registers.set_this(this);
        }
        // SAFETY: the program store keeps a derived view alive with its program; the tier
        // claims no leaf's view, so its activation starts at its first record.
        let base = unsafe { (*target.view).records() };
        unsafe {
            let leaf = &mut (*cx).leaf;
            leaf.call = ip;
            (leaf.form, leaf.argc) = (LEAF, ARGC);
            leaf.caller_registers = r;
            leaf.caller_base = (*cx).base;
            leaf.target = target;
            leaf.caller_program = None;
            if leaf_swaps_env(LEAF) {
                registers.set_env(target.env);
                registers.set_capture(target.capture);
            }
            (*cx).registers = registers;
            (*cx).base = base;
        }
        next!(cx, registers, base, Carried::script(cx))
    }
}

/// The `lane_script_call_leaf` form of `entry` for a record of `ARGC` arguments, or
/// `lane_script_call_full` for no leaf, out of line: a site meeting another form's callee
/// takes it from the leaf it was made for.
#[cold]
#[inline(never)]
fn other_leaf_form<H: Host, const ARGC: u16>(entry: CallEntry) -> Handler {
    match entry.leaf_form() {
        PLAIN_LEAF => lane_script_call_leaf::<H, PLAIN_LEAF, ARGC>,
        CAPTURE_LEAF => lane_script_call_leaf::<H, CAPTURE_LEAF, ARGC>,
        CONTEXT_LEAF => lane_script_call_leaf::<H, CONTEXT_LEAF, ARGC>,
        THIS_LEAF => lane_script_call_leaf::<H, THIS_LEAF, ARGC>,
        _ => lane_script_call_full::<H>,
    }
}

/// The form a call record `i` whose linked target is a leaf (`entry`) calls it with: the
/// `lane_script_call_leaf` instantiation of its entry and argument count, or, for a record
/// past `FIXED_ARGUMENT_COPIES` or returning its completion, which completes its caller too,
/// `lane_script_call_full`, which activates the leaf.
pub(super) fn leaf_call_handler<H: Host>(entry: CallEntry, i: &LaneInstruction) -> Handler {
    const _: () = assert!(FIXED_ARGUMENT_COPIES == 4);
    macro_rules! leaf {
        ($form:ident) => {
            match ImmediateLayout::argument_count(i.imm()) {
                0 => lane_script_call_leaf::<H, $form, 0>,
                1 => lane_script_call_leaf::<H, $form, 1>,
                2 => lane_script_call_leaf::<H, $form, 2>,
                3 => lane_script_call_leaf::<H, $form, 3>,
                4 => lane_script_call_leaf::<H, $form, 4>,
                _ => lane_script_call_full::<H>,
            }
        };
    }
    if i.a & crate::bytecode::RETURN_REGISTER != 0 {
        return lane_script_call_full::<H>;
    }
    match entry.leaf_form() {
        CONTEXT_LEAF => leaf!(CONTEXT_LEAF),
        CAPTURE_LEAF => leaf!(CAPTURE_LEAF),
        THIS_LEAF => leaf!(THIS_LEAF),
        _ => leaf!(PLAIN_LEAF),
    }
}

/// Whether `direct_call_this_in_lane` binds `receiver` for `target`, without the binding: any
/// binding but a sloppy function's, and its of an object in the direct record table, `null`
/// or `undefined`, the commonest receiver of a plain call, tested first.
#[inline(always)]
fn lane_binds_directly(target: &ScriptCallTarget, receiver: Value) -> bool {
    target.this_binding != ThisBinding::Sloppy
        || receiver.is_undefined()
        || receiver.is_direct_object_family()
        || receiver.is_null()
}

/// The return of the running leaf with `value` (`LaneContext::leaf`): the caller's registers,
/// records and context again, and the call's result in its register, as the plain
/// activation's return makes them (`lane_return_plain`). `None` while no leaf runs.
#[inline(always)]
pub(super) fn lane_complete_leaf<H: Host>(cx: *mut LaneContext<H>, value: Value, form: LeafForm) -> Option<(Registers, Ip)> {
    // SAFETY: the running lane's context.
    let leaf = unsafe { &mut (*cx).leaf };
    if leaf.call.is_null() {
        return None;
    }
    let call = std::mem::replace(&mut leaf.call, std::ptr::null());
    let caller = leaf.caller_registers;
    if let Some(program) = leaf.caller_program.take() {
        std::hint::cold_path();
        lane_enter_program(cx, program);
    }
    // SAFETY: the call record belongs to the caller's live view; the target stays cached.
    unsafe {
        debug_assert!(leaf.form == form);
        (*cx).registers = caller;
        (*cx).base = leaf.caller_base;
        caller.set((*call).a & crate::bytecode::REGISTER_MASK, value);
    }
    Some((caller, following(call)))
}

/// `materialize_leaf_in_lane` of the running leaf, if one runs: where a leaf's records reach
/// what observes activations or frames (the lane's exit, a getter's activation, an operation
/// made in place), a leaf is the activation its call would have pushed. Its records reach
/// nothing else that does (`leaf_body`).
#[inline(always)]
pub(super) fn lane_materialize_running_leaf<H: Host>(cx: *mut LaneContext<H>) {
    if !unsafe { (*cx).leaf.call }.is_null() {
        materialize_leaf_in_lane(cx);
    }
}

/// A plain call record `i` of `target`, another program's leaf (`CallEntry::leaf`), run as
/// `lane_script_call_leaf` runs its own program's: its records on a window at the lane
/// stack's top without an activation, in its program, which its return leaves
/// (`lane_complete_leaf`) and its materialization records as a crossing activation's
/// (`Completion::Cross`). `None`, which changes nothing, for a call the leaf form does not
/// make (more values than `FIXED_ARGUMENT_COPIES` or past the parameters, a receiver whose
/// binding reads the heap, no room, a call returning its completion, refused cached calls).
#[inline(always)]
pub(super) fn lane_cross_leaf_call<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    target: &ScriptCallTarget,
) -> Option<(Registers, Ip)> {
    let form = target.entry.leaf_form();
    let argc = ImmediateLayout::argument_count(i.imm());
    if form == NOT_LEAF || argc > FIXED_ARGUMENT_COPIES || i.a & crate::bytecode::RETURN_REGISTER != 0 {
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
        lane_binds_directly(target, receiver).then_some(receiver)?
    };
    if !lane_admits_window(vm, target, argc) {
        return None;
    }
    vm.programs.record_js_jit_lane_entry(target.program(), target.function);
    // SAFETY: as `lane_script_call_leaf`'s window.
    let registers = unsafe {
        let registers = vm.lane_stack.values.top();
        fill_admitted_window(registers, usize::from(target.fill), usize::from(target.registers));
        copy_arguments(registers, r.0.add(usize::from(ImmediateLayout::call_window_base(i.imm()))), argc);
        Registers(registers)
    };
    if leaf_binds_this(form) {
        registers.set_this(this);
    }
    // SAFETY: as `lane_script_call_leaf`'s first record.
    let base = unsafe { (*target.view).records() };
    unsafe {
        let caller_program = (*cx).program;
        let leaf = &mut (*cx).leaf;
        leaf.call = ip;
        (leaf.form, leaf.argc) = (form, argc);
        leaf.caller_registers = r;
        leaf.caller_base = (*cx).base;
        leaf.target = target;
        leaf.caller_program = Some(caller_program);
        if leaf_swaps_env(form) {
            registers.set_env(target.env);
            registers.set_capture(target.capture);
        }
        lane_enter_program(cx, target.program());
        (*cx).registers = registers;
        (*cx).base = base;
    }
    Some((registers, base))
}
