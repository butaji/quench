//! Lane body exits: admission and recovery of a body, the inlined activation of a body and its carried state, the leave, record, and resume rows of a body, the argument and capture shims a compiled body calls, and the region's calls back into the lane.
use super::*;

/// Whether a run of `own`'s specialized body may start: the one thing per run it cannot
/// prove at specialization, that the lane can make the activation of a call it inlines
/// should it leave inside one (`body_inlined_activation_in_lane`). `pending` activations are
/// the running body's own, which its call has not pushed yet (`enter_cell_body_in_lane`).
#[inline(always)]
pub(in super::super) fn body_admits_in_lane(cx: *mut LaneContext, method: &crate::compiled::LaneMethod, pending: usize) -> bool {
    room_admits_in_lane(cx, method.recovery_space(), pending)
}

/// Whether the lane has the room `room` names: the activations and values a leave inside an inlined
/// callee of native code about to run makes (`body_inlined_activation_in_lane`), past the `pending`
/// activations the running body's own call has not pushed.
#[inline(always)]
pub(in super::super) fn room_admits_in_lane(cx: *mut LaneContext, room: LaneRecoverySpace, pending: usize) -> bool {
    let vm = unsafe { vm(cx) };
    // The values past the stack's top hold the frame slots the rows write past a lane activation's
    // registers (`LaneRecoverySpace::reaching`), whether or not the code inlines an activation.
    (room.activations == 0 && (!vm.lane_stack.has_frames() || vm.lane_stack.values.room() >= room.values))
        || (vm.lane_stack.activation_limit.get().saturating_sub(vm.lane_stack.activation_count() + pending) >= room.activations
            && vm.lane_stack.values.room() >= room.values
            && unsafe { (*cx).script_calls }
            && !vm.with_scope_entered)
}

/// A compiled region's recovery demand may exceed the calls the lane has observed.
/// Materialize before growing either Vec: live lane windows borrow their storage.
/// The caller must exit immediately and resume from its original semantic record.
#[cold]
#[inline(never)]
pub(in super::super) fn body_prepare_recovery_in_lane(cx: *mut LaneContext, method: &crate::compiled::LaneMethod) -> bool {
    room_prepare_in_lane(cx, method.recovery_space())
}

/// Grows the lane stacks to the room `room` names (`room_admits_in_lane`), materializing the lane's
/// activations first; whether it grew them. The caller must exit at once and resume from its
/// original semantic record.
#[cold]
#[inline(never)]
pub(in super::super) fn room_prepare_in_lane(cx: *mut LaneContext, room: LaneRecoverySpace) -> bool {
    let vm = unsafe { vm(cx) };
    if !unsafe { (*cx).script_calls } || vm.with_scope_entered { return false; }
    let values = vm.lane_stack.values.room() < room.values;
    let activations = vm.lane_stack.frame_capacity() - vm.lane_stack.activation_count() < room.activations;
    if !values && !activations { return false; }
    vm.materialize_lane_activations(cx);
    if values { vm.lane_stack.reserve_values(room.values); }
    if activations { vm.lane_stack.records_mut().reserve(room.activations); }
    vm.lane_stack.refresh_activation_limit();
    true
}

/// The activation native code that left inside inlined `frame` of `run`, at its callee's
/// PC `pc`, continues in: the activations the calls of the frame and the frames around it
/// make (`push_activation_in_lane`), outermost first, each holding the registers native code
/// held for it (`Vm::scratch_in_lane`); the innermost's record at `pc`. Each frame's call
/// guard proved its callee the inlined one, whose resolution is a function of it; a cache
/// that dropped its entry resolves it again.
pub(in super::super) fn body_inlined_activation_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    base: Ip,
    run: InlinedRun<'_>,
    frame: usize,
    pc: u32,
) -> (Registers, Ip, Carried) {
    let frames = run.frames;
    if run.framed {
        // Only the frames' own reach: the scratch window may be wider (another code's), past this storage.
        Vm::adopt_window_in_lane(r.0.cast_const(), usize::from(run.caller_registers), crate::compiled::window_width(frames, run.caller_registers));
    }
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let caller_registers = run.caller_registers;
    super::super::inlined_views::materialize(cx, r, base, frames, frame, pc,
        // Cache eviction is not invalidation of an immutable callee binding: a lifecycle edge retires
        // the body first (`ProgramStore::retire_dead_native_calls`), so a miss resolves the callee again.
        // Resolved now: the activation recovery makes is one whose runs all ran inlined, so none
        // counted toward the callee's view (`resolve_call_target_in_lane`).
        |vm, code, callee| vm.resolve_call_target_in_lane(code, callee),
        // The registers native code held for the frame, which the leave's PC `at` keeps live; the
        // rest the activation keeps (`lane_region`'s liveness).
        |call, at, sink| {
            let live = code.register_root_masks(call.target.function);
            Vm::scratch_in_lane(usize::from(call.base - caller_registers), usize::from(call.registers), |register, value| {
                if live.is_none_or(|live| live.is_live(at as usize, register)) {
                    sink(register, value);
                }
            });
        })
}

/// What an activation carries into the record at `pc` of its function's `view`: the script
/// state, and the accumulator where the record reads one.
pub(in super::super) fn carried_at_in_lane(cx: *mut LaneContext, view: *const LaneView, registers: Registers, pc: u32) -> Carried {
    let mut carried = Carried::script(cx);
    // SAFETY: a resolved target's view outlives the lane run.
    if let Some(&register) = unsafe { &*view }.accumulator_registers.get(pc as usize)
        && register != NO_ACCUMULATOR
    {
        carried.acc = Slot::of(registers.get(register));
    }
    carried
}

/// A lane body's call at `call_pc` of the running activation (`lane_region`'s `Call`): the
/// activation `script_call_prove_miss`'s common form makes, then the callee's own lane body. A
/// callee whose body returns completes the call (`call_return::step_activation_return_of`), the result in the
/// call's register; one that leaves, or has no body, leaves the lane at its record in that
/// activation (`LaneContext::callee_resume`). A call the common form does not make misses,
/// before any of its effects.
///
/// # Safety
/// `context` is the running lane's context, whose activation's registers hold the caller's
/// state; nothing the call runs allocates or collects.
/// A lane body's read of a capture `packed` names (depth in its high half, slot in its low),
/// by the lane's own `LoadCapture` lookup (`read_capture_value`): its value, or
/// `Value::DELETED` for none or an uninitialized binding, which leaves the body at the read.
/// `ArgumentsLength` of a lane body's entry function (`lane_arguments_length`): the count a
/// keeping activation kept, else the count an unmaterialized binding answers. `DELETED` where
/// the lane's own handler must read the materialized object, which the body's leave reaches.
pub(crate) unsafe extern "C" fn body_arguments_length_in_lane(_vm: u64, _residual: u64, context: u64, slot: u64) -> u64 {
    if context == 0 { return Value::DELETED.to_bits(); }
    let cx = context as usize as *mut LaneContext;
    let value = match unsafe { keeping_activation_in_lane(cx) } {
        Some((argc, _)) => Value::integer(i32::from(argc)),
        None => match unsafe { unmaterialized_arguments_in_lane(cx, slot as usize) } {
            Some((_, count)) => Value::number(count as f64),
            None => Value::DELETED,
        },
    };
    value.to_bits()
}

/// `ArgumentGet` of that binding at an integer index a keeping activation kept or an
/// unmaterialized binding holds; any other key leaves at the record.
pub(crate) unsafe extern "C" fn body_argument_get_in_lane(_vm: u64, _residual: u64, context: u64, slot: u64, index: Value) -> u64 {
    if context == 0 { return Value::DELETED.to_bits(); }
    let cx = context as usize as *mut LaneContext;
    if let Some((argc, function)) = unsafe { keeping_activation_in_lane(cx) }
        && let Some(index) = index.as_int().and_then(|index| u16::try_from(index).ok()).filter(|index| *index < argc)
    {
        // SAFETY: the context's program outlives the lane run, and the keeping activation
        // holds its `argc` values after the function's registers (`staged::call_keeping::keep_passed`).
        let registers = unsafe { &*(*cx).code }.function(function as usize).registers;
        return unsafe { (*cx).registers }.get(registers + index).to_bits();
    }
    match unsafe { unmaterialized_arguments_in_lane(cx, slot as usize) } {
        Some((values, count)) => index
            .as_int()
            .and_then(|index| usize::try_from(index).ok())
            .filter(|index| *index < count)
            // SAFETY: `values` holds `count` kept values, and the index is below it.
            .map_or(Value::DELETED, |index| unsafe { *values.add(index) })
            .to_bits(),
        None => Value::DELETED.to_bits(),
    }
}

pub(crate) unsafe extern "C" fn body_capture_in_lane(_vm: u64, _residual: u64, context: u64, packed: u64) -> u64 {
    if context == 0 { return Value::DELETED.to_bits(); }
    let cx = context as usize as *mut LaneContext;
    let (depth, slot) = ((packed >> u16::BITS) as u16, packed as u16);
    let value = match native_frames::innermost_callee_env(cx) {
        Some(env) => {
            let vm = unsafe { vm(cx) };
            // SAFETY: as `Vm::walked_capture_slots`; no collection runs in the lane.
            vm.walked_capture_slots(env, false, depth).and_then(|(slots, root)| vm.plain_capture(unsafe { &*slots }, root, slot))
        }
        None => read_capture_value::<true>(cx, depth, slot),
    };
    value
        .filter(|value| value.to_bits() & quench_runtime::value::ValueLayout::TAG_MASK != quench_runtime::value::ValueLayout::INTERNAL_TAG)
        .unwrap_or(Value::DELETED)
        .to_bits()
}

/// A lane body's store of `value` to the capture `packed` names (depth in its high half,
/// slot in its low), by the lane's own stores (`store_own_capture`, else
/// `store_to_capture`): 1 where it stored, else 0, which leaves the body at the store,
/// before it.
///
/// # Safety
/// As `body_capture_in_lane`.
pub(crate) unsafe extern "C" fn body_capture_store_in_lane(_vm: u64, _residual: u64, context: u64, packed: u64, value: Value) -> u64 {
    if context == 0 { return 0; }
    let cx = context as usize as *mut LaneContext;
    let (depth, slot, value) = ((packed >> u16::BITS) as u16, packed as u16, value);
    let stored = match native_frames::innermost_callee_env(cx) {
        Some(env) => {
            // SAFETY: the context's program outlives the lane run.
            let (code, program) = unsafe { (&*(*cx).code, (*cx).program) };
            store_to_environment(unsafe { vm(cx) }, code, program, env, depth, slot, value)
        }
        None => (depth == 0 && store_own_capture(cx, slot, value)) || store_to_capture(cx, depth, slot, value),
    };
    u64::from(stored)
}

/// Whether a body's run that ended at `exit` counts toward its retirement and
/// reoptimization: a leave into a callee that only lacked a body while warming toward one
/// (`LaneContext::callee_warming`) does not, as the next run may find the callee's body. Only
/// a `Callee` leave reads the flag: the callee's own code may run nested bodies before the
/// leave resolves, whose other ends must not take it.
pub(in super::super) fn leave_counts_in_lane(cx: *mut LaneContext, exit: crate::compiled::LaneExit) -> bool {
    // SAFETY: the running lane's context, read and reset by the run that left into the callee.
    exit != crate::compiled::LaneExit::Callee || !unsafe { std::mem::take(&mut (*cx).callee_warming) }
}

/// With diagnostics, counts a leave of `own`'s body into a warming callee, which
/// `leave_counts_in_lane` keeps out of the body's leave counts, in the view's report.
pub(in super::super) fn note_warming_leave_in_lane(cx: *mut LaneContext, own: &LaneView) {
    if crate::compiled::admission::diagnosing(unsafe { vm(cx) }) {
        own.tier().body_stats.warming_leaves.set(own.tier().body_stats.warming_leaves.get() + 1);
    }
}

/// Records a run of `own`'s body that ended at `exit` in the diagnostics leave table.
#[inline(always)]
pub(in super::super) fn record_end_in_lane(cx: *mut LaneContext, own: &LaneView, method: &crate::compiled::LaneMethod, exit: crate::compiled::LaneExit) {
    crate::compiled::body_leave::body_ended(cx, exit);
    if crate::compiled::admission::diagnosing(unsafe { vm(cx) }) {
        own.tier().body_stats.ended(exit, &method.exits, &method.inlined, crate::compiled::take_lane_exit_line());
    }
}

/// Counts an entry whose body the lane could not leave inside of (`body_admits_in_lane`): in the VM's
/// tier events and, with diagnostics, the view's report.
#[cold]
pub(in super::super) fn body_refused_in_lane(vm: &Vm, own: &LaneView) {
    vm.programs.tier_events().record(Event::Refused);
    if crate::compiled::admission::diagnosing(vm) {
        own.tier().body_stats.refused.set(own.tier().body_stats.refused.get() + 1);
    }
}

pub(in super::super) fn body_left_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    own: &LaneView,
    method: &crate::compiled::LaneMethod,
    exit: crate::compiled::LaneExit,
) -> LaneBodyLeft {
    record_end_in_lane(cx, own, method, exit);
    if exit == crate::compiled::LaneExit::Terminated {
        // A fault can bypass the native exit's register snapshots. Discard its
        // native frames instead of materializing stale homes for guest execution.
        native_frames::discard_frames(cx);
        return LaneBodyLeft::Terminated;
    }
    frames_resumed_in_lane(cx, r, (own, Some(method)), exit)
}

/// The activation a leave resumes in: the one the leaving body ran in for a native call's callee, else
/// the innermost frame a native-frame advance made the running one, where there is one.
pub(in super::super) fn resumed_activation_in_lane<'a>(
    cx: *mut LaneContext,
    r: Registers,
    (own, root): (&'a LaneView, Option<&'a crate::compiled::LaneMethod>),
    callee: bool,
) -> (Registers, &'a LaneView, Option<&'a crate::compiled::LaneMethod>) {
    if callee {
        return (r, own, root);
    }
    match native_frames::advance_into_frames(cx) {
        // SAFETY: the advance made the innermost frame the running activation.
        Some((own, method)) => (unsafe { (*cx).registers }, own, Some(method)),
        None => (r, own, root),
    }
}

/// Where the lane continues after native code of the activation with registers `r`, a body of
/// `root`'s or a region's, left with `exit`: in the innermost of the native frames it left inside, which
/// the lane links and continues in first (`native_frames::advance_into_frames`; a leave into a callee's
/// activation already linked them and went on in it), then at the record `exit` names there, or the
/// innermost's completion. `own` is the activation's view; `root` the body that ran, where one did,
/// whose inlined frames an `exit` that was not inside a native frame names.
pub(in super::super) fn frames_resumed_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    (own, root): (&LaneView, Option<&crate::compiled::LaneMethod>),
    exit: crate::compiled::LaneExit,
) -> LaneBodyLeft {
    let callee = matches!(exit, crate::compiled::LaneExit::Callee);
    let (r, own, method) = resumed_activation_in_lane(cx, r, (own, root), callee);
    // SAFETY: the running activation's records start at the context's base.
    let base = unsafe { (*cx).base };
    match exit.resumed() {
        // SAFETY: the record that raised is one of the activation's function.
        crate::compiled::LaneExit::Threw { pc } => LaneBodyLeft::Raised(unsafe { base.add(pc) }),
        crate::compiled::LaneExit::Terminated | crate::compiled::LaneExit::Entry | crate::compiled::LaneExit::Stub => unreachable!("termination leaves before recovery; an entry leave or an unentered call resumes at its first record"),
        crate::compiled::LaneExit::At(pc) => {
            body_resume_at_in_lane(cx, r, own, pc)
        }
        crate::compiled::LaneExit::Inlined { frame, pc } => {
            let method = method.expect("a leave inside an inlined callee is a lane body's");
            let (registers, resume, carried) = body_inlined_activation_in_lane(cx, r, base, InlinedRun::of_body(method), frame, pc);
            LaneBodyLeft::Resume(registers, resume, carried)
        }
        // A native call made its callee's activation and left the lane there.
        crate::compiled::LaneExit::Callee => {
            let mut carried = Carried::script(cx);
            // SAFETY: `body_call_helper` set both before the body left.
            let (registers, resume) = unsafe { ((*cx).registers, (*cx).callee_resume) };
            carried.acc = Slot::of(unsafe { (*cx).callee_acc });
            LaneBodyLeft::Resume(registers, resume, carried)
        }
        // SAFETY: the record that completes the activation is one of its function's.
        crate::compiled::LaneExit::Returned { pc, value } => LaneBodyLeft::Completed(unsafe { base.add(pc) }, value),
    }
}

/// Reloads the heap bases the running body's rows read through the payload (`Vm::refresh_body_bases`), as
/// every call, construct and step helper of the body ends: the callee's run or an allocation may have
/// moved them.
///
/// # Safety
/// `context` is the running lane's.
pub(crate) unsafe fn refresh_body_bases(context: u64) {
    let cx = context as usize as *mut LaneContext;
    // SAFETY: the caller's contract.
    unsafe { vm(cx).refresh_body_bases() };
}

/// Names `payload`, the dispatch of the native loop region that runs on the activation of the lane
/// `context`, to the helpers its rows call (`region_call_in_lane`), and returns the one it replaces:
/// a region's calls run lane code that may enter regions of its own.
///
/// # Safety
/// `context` is the running lane's.
pub(crate) unsafe fn region_payload_in_lane(context: usize, payload: usize) -> usize {
    // SAFETY: the caller's contract.
    std::mem::replace(unsafe { &mut (*(context as *mut LaneContext)).region_payload }, payload)
}

/// The helper a loop region's call row runs (`LaneHelper::RegionCall`): the call at `call_pc` of the
/// activation the lane context names, whose registers the region runs on, as `body_call_helper`
/// makes it, then the heap bases the region's payload holds reloaded (`LaneContext::region_payload`)
/// besides the body frame's, which the callee's run or an allocation may have moved.
///
/// # Safety
/// `context` is the running lane's, which the region entered (`region_payload_in_lane`), and `call_pc` a call record of its function.
pub(crate) unsafe extern "C" fn region_call_in_lane(_vm: u64, _residual: u64, context: u64, call_pc: u64) -> u64 {
    // SAFETY: the caller's contract.
    let (status, in_place) = unsafe { body_calls::body_call_status(context, call_pc) };
    let cx = context as usize as *mut LaneContext;
    let payload = unsafe { (*cx).region_payload };
    if in_place {
        // An in-place native pushed nothing: the stacks stand, only the heap bases may have moved.
        // SAFETY: a region's payload lives for its run.
        if let Some(payload) = unsafe { (payload as *mut quench_runtime::vm::engine_types::JsStepDispatch).as_mut() } { payload.refresh_object_bases(unsafe { super::super::vm(cx) }); }
    } else {
        // SAFETY: as above.
        unsafe { refresh_region_bases_of(context) };
    }
    status
}

/// Reloads the heap bases of the native loop region the lane context `context` names (`region_payload_in_lane`), where
/// one runs: the one refresh every lane helper a region's rows call ends with (`region_call_in_lane`, `body_general_step`,
/// `vm_operation_step`), so a row after any of them reads current bases (`Vm::refresh_region_bases`). Zero payload: no-op.
///
/// # Safety
/// `context` is the running lane's.
pub(crate) unsafe fn refresh_region_bases_of(context: u64) {
    let cx = context as usize as *mut LaneContext;
    // SAFETY: the caller's contract.
    let payload = unsafe { (*cx).region_payload };
    if payload != 0 {
        unsafe { super::super::vm(cx).refresh_region_bases(payload, direct_calls_in_lane(cx)) };
    }
}
