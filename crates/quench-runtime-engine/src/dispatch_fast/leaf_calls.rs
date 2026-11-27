//! A plain call of a leaf target: its records run on a window of the lane stack without an
//! activation (`CallEntry::leaf`), and its return completes the call.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::staged::call_leaf::LeafWindow;
use quench_runtime::staged::Concrete;


script_handler! {
    /// `script_call_leaf_prove_miss` of a site whose link is not proved: the target it links, whose
    /// entry, where it is this form's leaf, is proved (`Admission::UNPROVEN` cleared), its cell
    /// body published or not; then the call as the unproved
    /// leaf call always made it. A plain activation's entry takes it in place, another leaf
    /// form's call its handler, any other the full form.
    #[cold]
    #[inline(never)]
    pub(crate) fn script_call_leaf_prove_miss<LEAF: u8, ARGC: u16>(cx, r, ip, i, view) {
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
                match activate_unproven_in_lane::<ARGC>(cx, r, ip, i, target) {
                    Some(resumed) => resume!(cx, resumed),
                    None => sibling!(script_call_full_miss, r, ip, Carried::script(cx)),
                }
            }
            sibling!(other_leaf_form::<ARGC>(target.entry), r, ip, Carried::script(cx))
        }
        // The proof: a leaf call runs its leaf here whether its cell holds a body or not, so the
        // proved form makes the same call; a leaf entry activates as `Activates` from a plain
        // call (`CallEntry::activates`), which tests the cell (`plain_call_activation`) or,
        // proved in a row (`plain_call_activation`), runs the leaf's records as this call does.
        {
            let (index, resolved) = (link_index(i.call_link.get()), std::ptr::from_ref(target));
            let entry = &mut vm.lane_stack.script_targets.entries[index];
            debug_assert!(std::ptr::eq(entry, resolved), "the link names the entry it resolved");
            entry.admission.0 &= !Admission::UNPROVEN;
            debug_assert!(entry.admission == vm.lane_stack.admission, "an admitted entry's word is the lane's");
        }
        // A proved leaf site is one its function's inlined view may splice in (N7a).
        super::inlined_views::note_leaf_site(cx, ip);
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
pub(crate) unsafe fn lane_run_leaf<const LEAF: u8, const ARGC: u16>(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    target: &ScriptCallTarget,
    this: Value,
) -> (Registers, Ip) {
    // SAFETY: the function's contract; the window is `push_passed_in_lane`'s, which the
    // stack's length covers only once the leaf is materialized: no collection runs before.
    unsafe {
        let window = Registers(vm(cx).lane_stack.values.top());
        let passed = r.0.add(usize::from(ImmediateLayout::call_window_base(i.imm())));
        lane_enter_leaf(cx, AdmittedWindow::of(target, passed, ARGC), window, ip, r, target, LEAF, ARGC, this, None, true)
    }
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
/// `LEAF_CALL` miss is `script_call_leaf_prove_miss` of the same form and count, `leaf_call_miss_of`), or,
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
/// `script_call_leaf_prove_miss` instantiation of the same form and count (`leaf_call_miss_of`), or,
/// for a record past `FIXED_ARGUMENT_COPIES` or returning its completion, which completes its
/// caller too, the row whose miss is `script_call_full_miss`, which activates the leaf.
pub(crate) fn leaf_call_handler(entry: CallEntry, i: &LaneInstruction) -> Handler {
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
        &mut quench_runtime::staged::Concrete::pure(), receiver.to_bits(), target.this_binding == ThisBinding::Sloppy, (),
    )
    .is_some()
}

/// The admitted window of a leaf call (`step_leaf_call_of`'s fill): `argc` values from `passed`,
/// then `undefined` by the admitted room's block stores (`fill_admitted_window`).
pub(crate) struct AdmittedWindow {
    fill: usize,
    registers: usize,
    passed: *const Value,
    argc: u16,
}

impl AdmittedWindow {
    #[inline(always)]
    pub(crate) fn of(target: &ScriptCallTarget, passed: *const Value, argc: u16) -> Self {
        Self { fill: usize::from(target.fill), registers: usize::from(target.registers), passed, argc }
    }
}

impl LeafWindow<Concrete> for AdmittedWindow {
    #[inline(always)]
    fn fill(&mut self, _: &mut Concrete, window: u64) {
        let registers = std::ptr::with_exposed_provenance_mut(window as usize);
        // SAFETY: `lane_enter_leaf`'s contract: the window holds the target's admitted room.
        unsafe {
            fill_admitted_window(registers, self.fill, self.registers);
            copy_arguments(registers, self.passed, self.argc);
        }
    }
}

/// The window of a leaf call its tier filled already (a cell body's, `fill_window_in_lane`).
pub(crate) struct FilledWindow;

impl LeafWindow<Concrete> for FilledWindow {
    #[inline(always)]
    fn fill(&mut self, _: &mut Concrete, _: u64) {}
}

/// Make the call record `call` of `caller`'s registers the running leaf of `target` in `form`
/// with `argc` values on `window` (`LaneContext::leaf`): the staged leaf call
/// (`call_leaf::step_leaf_call_of`) on machine words fills the window (`window_fill`) and writes
/// the leaf record and the window's context words its form makes the callee's (`this` where it
/// binds its receiver, the target's environment and captures where it swaps them);
/// `caller_program` is the program a crossing leaf's return enters again. The context's
/// registers and records become the callee's: its window and its first record, which the
/// caller dispatches.
///
/// # Safety
/// `cx` is the running lane's context, and `window` the callee's window with its context words
/// below it and the target's admitted room past it; the program store keeps the target's
/// derived view alive with its program, and the tier claims no leaf's view.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn lane_enter_leaf<F: LeafWindow<Concrete>>(
    cx: *mut LaneContext,
    mut window_fill: F,
    window: Registers,
    call: Ip,
    caller: Registers,
    target: &ScriptCallTarget,
    form: LeafForm,
    argc: u16,
    this: Value,
    caller_program: Option<ProgramId>,
    checked: bool,
) -> (Registers, Ip) {
    use quench_runtime::staged::call_leaf::{step_leaf_call_of, LeafContext, LeafEntry};
    // A form that binds the receiver writes the leaf's `this` (`call_leaf::binds_this`); any other reads none.
    // A call row's hand-off checked `this` where it bound it (`checked`, `call_hand_over`).
    if !checked && quench_runtime::staged::call_leaf::binds_this(form) {
        // SAFETY: the running lane's context; the program store keeps the view alive.
        note_lane_this(unsafe { vm(cx) }, this, || unsafe { (*target.view).this_rows });
    }
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
        step_leaf_call_of(
            &mut Concrete::pure(), &mut window_fill, leaf.expose_provenance() as u64,
            window.0.expose_provenance() as u64, entry, context,
        );
        (*leaf).caller_program = caller_program;
        let base = (*target.view).records();
        ((*cx).registers, (*cx).base) = (window, base);
        (window, base)
    }
}

/// The return of the running leaf with `value` (`LaneContext::leaf`): the caller's registers,
/// records and context again, and the call's result in its register, as the plain
/// activation's return makes them (`call_return::step_activation_return_of`). `None` while no leaf runs.
#[inline(always)]
pub(crate) fn complete_leaf_in_lane(cx: *mut LaneContext, value: Value, form: LeafForm) -> Option<(Registers, Ip)> {
    let resumed = lane_return_leaf(cx, value, form)?;
    // SAFETY: the running lane's context, whose leaf just completed.
    if let Some(program) = unsafe { (*cx).leaf.caller_program.take() } {
        std::hint::cold_path();
        enter_program_in_lane(cx, program);
    }
    Some(resumed)
}

/// `complete_leaf_in_lane` of a running leaf of the caller's own program, the common leaf return
/// as one op: it makes no call, so a returning handler keeps no frame. `None`, which changes
/// nothing, where no leaf runs or the leaf crosses programs (`caller_program`), which
/// `complete_leaf_in_lane` completes.
#[inline(always)]
pub(crate) fn lane_return_leaf_header(cx: *mut LaneContext, value: Value, form: LeafForm) -> Option<(Registers, Ip)> {
    // SAFETY: the running lane's context.
    if unsafe { (*cx).leaf.caller_program }.is_some() {
        return None;
    }
    lane_return_leaf(cx, value, form)
}

/// The staged leaf return (`call_leaf::step_leaf_return_of`) on machine words: the running
/// leaf's caller, its call word cleared and `value` in the call's result register; the
/// context's registers and records the caller's again, and where it continues past the call.
/// `None`, which changes nothing, while no leaf runs.
#[inline(always)]
fn lane_return_leaf(cx: *mut LaneContext, value: Value, form: LeafForm) -> Option<(Registers, Ip)> {
    // SAFETY: the running lane's context.
    let leaf = unsafe { std::ptr::addr_of_mut!((*cx).leaf) };
    // SAFETY: a running leaf's call record belongs to the caller's live view.
    let result_at = |_: &mut Concrete, call: u64| unsafe {
        let call: Ip = std::ptr::with_exposed_provenance(call as usize);
        u64::from((*call).a & quench_runtime::bytecode::REGISTER_MASK) * 8
    };
    let caller = quench_runtime::staged::call_leaf::step_leaf_return_of(
        &mut Concrete::pure(), leaf.expose_provenance() as u64, value.to_bits(), result_at, (),
    )?;
    let call: Ip = std::ptr::with_exposed_provenance(caller.call as usize);
    let registers = Registers(std::ptr::with_exposed_provenance_mut(caller.registers as usize));
    let base: Ip = std::ptr::with_exposed_provenance(caller.base as usize);
    // SAFETY: the running lane's context; the target stays cached.
    unsafe {
        debug_assert!((*leaf).form == form);
        ((*cx).registers, (*cx).base) = (registers, base);
    }
    Some((registers, following(call)))
}

/// `materialize_leaf_in_lane` of the running leaf, if one runs: where a leaf's records reach
/// what observes activations or frames (the lane's exit, a getter's activation, an operation
/// made in place), a leaf is the activation its call would have pushed. Its records reach
/// nothing else that does (`leaf_body`).
#[inline(always)]
pub(crate) fn materialize_running_leaf_in_lane(cx: *mut LaneContext) {
    if !unsafe { (*cx).leaf.call }.is_null() {
        materialize_leaf_in_lane(cx);
    }
}

/// A plain call record `i` of `target`, another program's leaf (`CallEntry::leaf`), run as
/// `script_call_leaf_prove_miss` runs its own program's: its records on a window at the lane
/// stack's top without an activation, in its program, which its return leaves
/// (`complete_leaf_in_lane`) and its materialization records as a crossing activation's
/// (`Completion::Cross`). `None`, which changes nothing, for a call the leaf form does not
/// make (more values than `FIXED_ARGUMENT_COPIES` or past the parameters, a receiver whose
/// binding reads the heap, no room, a call returning its completion, refused cached calls).
#[inline(always)]
pub(crate) fn cross_leaf_call_in_lane(
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
    // SAFETY: as `script_call_leaf_prove_miss`'s window and leaf.
    unsafe {
        let window = Registers(vm.lane_stack.values.top());
        let passed = r.0.add(usize::from(ImmediateLayout::call_window_base(i.imm())));
        let caller_program = (*cx).program;
        let entered = lane_enter_leaf(cx, AdmittedWindow::of(target, passed, argc), window, ip, r, target, form, argc, this, Some(caller_program), false);
        enter_program_in_lane(cx, target.program());
        Some(entered)
    }
}
