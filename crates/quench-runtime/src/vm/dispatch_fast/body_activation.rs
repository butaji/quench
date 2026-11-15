//! Inlined body activations: the activation native code left inside an inlined frame becomes, and its carried values.

use super::*;

/// The activation native code that left inside inlined `frame` of `run`, at its callee's
/// PC `pc`, continues in: the activations the calls of the frame and the frames around it
/// make (`lane_push_activation`), outermost first, each holding the registers native code
/// held for it (`Vm::lane_scratch`); the innermost's record at `pc`. Each frame's call
/// guard proved its callee the inlined one, whose resolution is a function of it; a cache
/// that dropped its entry resolves it again.
pub(super) fn lane_body_inlined_activation<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    base: Ip,
    run: InlinedRun<'_>,
    frame: usize,
    pc: u32,
) -> (Registers, Ip, Carried) {
    if run.framed {
        Vm::<H>::lane_adopt_window(r.0.cast_const(), usize::from(run.caller_registers));
    }
    let mut chain = vec![frame];
    while let Some(parent) = run.frames[*chain.last().expect("a chain has a frame")].parent {
        chain.push(parent);
    }
    let (mut registers, mut records, mut view) = (r, base, std::ptr::null::<LaneView>());
    for depth in (0..chain.len()).rev() {
        (registers, records, view) = enter_inlined_frame(cx, &run, &chain, depth, (registers, records), pc);
    }
    // SAFETY: the innermost activation's view holds a record for each of its function's PCs.
    let resume = unsafe { records.add(pc as usize) };
    (registers, resume, lane_carried_at(cx, view, registers, pc))
}

/// The activation of the inlined frame `chain[depth]` of `run`, made from the caller's
/// `registers` and `records`: its call's arguments pass from them, and the registers native code
/// held for it are restored where the frame's exit wrote them. Returns the activation's registers,
/// records and view.
fn enter_inlined_frame<H: Host>(
    cx: *mut LaneContext<H>,
    run: &InlinedRun<'_>,
    chain: &[usize],
    depth: usize,
    (registers, records): (Registers, Ip),
    pc: u32,
) -> (Registers, Ip, *const LaneView) {
    let frames = run.frames;
    let call = frames[chain[depth]];
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    // Cache eviction is not invalidation of an immutable callee binding: a lifecycle
    // edge retires the body first (`ProgramStore::retire_dead_native_calls`). Resolve
    // only when recovery needs the activation, rather than scanning at entry.
    let callee = call.target.identity().expect("a lane body's inlined callee has its identity");
    let target = vm.script_call_entry(callee)
        // SAFETY: copy the current cache row before any cache mutation.
        .map(|target| unsafe { *target })
        .or_else(|| vm.resolve_script_call_target(code, callee))
        .expect("an inlined callee resolves");
    // SAFETY: the record is the call's in its parent's view.
    let (target, record) = (&target, unsafe { records.add(call.call as usize) });
    let parent = call.parent_base(frames);
    let this = lane_call_this(vm, target, registers.get(call.receiver - parent)).expect("a direct-object receiver binds itself");
    // The frame's own window and count: a call through `call` passes past its receiver
    // (`lane_unit::Frame::function_call`).
    // SAFETY: the call's window of `registers` holds its values.
    let passed = unsafe { registers.0.add(usize::from(call.window - parent)).cast_const() };
    let (callee, _) = lane_push_passed::<H, false, false>(cx, registers, record, target, passed, call.arguments, this, false);
    // The frame's registers live where it stands, the leave's PC or its inner frame's
    // call; the exit wrote only those (`lane_region`), the rest keep the activation's.
    let at = match depth.checked_sub(1) { Some(inner) => frames[chain[inner]].call, None => pc };
    let live = code.register_root_masks(call.target.function);
    let scratch = usize::from(call.base - run.caller_registers);
    Vm::<H>::lane_scratch(scratch, usize::from(call.registers), |register, value| {
        if live.is_none_or(|live| live.is_live(at as usize, register)) {
            callee.set(register, value);
        }
    });
    // The activation's records start at the context's base, past any tier prologue its
    // entry record is.
    (callee, unsafe { (*cx).base }, target.view)
}

/// What an activation carries into the record at `pc` of its function's `view`: the script
/// state, and the accumulator where the record reads one.
pub(super) fn lane_carried_at<H: Host>(cx: *mut LaneContext<H>, view: *const LaneView, registers: Registers, pc: u32) -> Carried {
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
/// activation `lane_script_call`'s common form makes, then the callee's own lane body. A
/// callee whose body returns completes the call (`lane_return_plain`), the result in the
/// call's register; one that leaves, or has no body, leaves the lane at its record in that
/// activation (`LaneContext::callee_resume`). A call the common form does not make misses,
/// before any of its effects.
///
/// # Safety
/// `context` is the running lane's context, whose activation's registers hold the caller's
/// state; nothing the call runs allocates or collects.
/// A lane body's read of a capture `packed` names (depth in its high half, slot in its low),
/// by the lane's own `LoadCapture` lookup (`lane_read_capture_value`): its value, or
/// `Value::DELETED` for none or an uninitialized binding, which leaves the body at the read.
/// `ArgumentsLength` of a lane body's entry function (`lane_arguments_length`): the count a
/// keeping activation kept, else the count an unmaterialized binding answers. `DELETED` where
/// the lane's own handler must read the materialized object, which the body's leave reaches.
pub(in crate::vm) unsafe extern "C" fn lane_body_arguments_length<H: Host>(_vm: u64, _residual: u64, context: u64, slot: u64) -> u64 {
    if !crate::DYNAMIC_EXECUTION_ENABLED || context == 0 { return Value::DELETED.to_bits(); }
    let cx = context as usize as *mut LaneContext<H>;
    let value = match unsafe { lane_keeping_activation(cx) } {
        Some((argc, _)) => Value::integer(i32::from(argc)),
        None => match unsafe { lane_unmaterialized_arguments(cx, slot as usize) } {
            Some((_, count)) => Value::number(count as f64),
            None => Value::DELETED,
        },
    };
    value.to_bits()
}

/// `ArgumentGet` of that binding at an integer index a keeping activation kept or an
/// unmaterialized binding holds (`lane_argument_get`); any other key leaves at the record.
pub(in crate::vm) unsafe extern "C" fn lane_body_argument_get<H: Host>(_vm: u64, _residual: u64, context: u64, slot: u64, index: u64) -> u64 {
    if !crate::DYNAMIC_EXECUTION_ENABLED || context == 0 { return Value::DELETED.to_bits(); }
    let cx = context as usize as *mut LaneContext<H>;
    let index = Value::from_bits(index);
    if let Some((argc, function)) = unsafe { lane_keeping_activation(cx) }
        && let Some(index) = index.as_int().and_then(|index| u16::try_from(index).ok()).filter(|index| *index < argc)
    {
        // SAFETY: the context's program outlives the lane run, and the keeping activation
        // holds its `argc` values after the function's registers (`lane_keep_arguments`).
        let registers = unsafe { &*(*cx).code }.function(function as usize).registers;
        return unsafe { (*cx).registers }.get(registers + index).to_bits();
    }
    match unsafe { lane_unmaterialized_arguments(cx, slot as usize) } {
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

pub(in crate::vm) unsafe extern "C" fn lane_body_capture<H: Host>(_vm: u64, _residual: u64, context: u64, packed: u64) -> u64 {
    // As `lane_body_call`: static regions recover captures through their semantic operation,
    // and an ahead body has no lane context to read them from.
    if !crate::DYNAMIC_EXECUTION_ENABLED || context == 0 { return Value::DELETED.to_bits(); }
    let cx = context as usize as *mut LaneContext<H>;
    let (depth, slot) = ((packed >> u16::BITS) as u16, packed as u16);
    let value = match native_frames::innermost_callee_env(cx) {
        Some(env) => {
            let vm = unsafe { vm(cx) };
            // SAFETY: as `Vm::walked_capture_slots`; no collection runs in the lane.
            vm.walked_capture_slots(env, false, depth).and_then(|(slots, root)| vm.plain_capture(unsafe { &*slots }, root, slot).0)
        }
        None => lane_read_capture_value::<H, true>(cx, depth, slot).0,
    };
    value
        .filter(|value| value.to_bits() & crate::value::ValueLayout::TAG_MASK != crate::value::ValueLayout::INTERNAL_TAG)
        .unwrap_or(Value::DELETED)
        .to_bits()
}

/// A lane body's store of `value` to the capture `packed` names (depth in its high half,
/// slot in its low), by the lane's own stores (`lane_store_own_capture`, else
/// `lane_capture_store`): 1 where it stored, else 0, which leaves the body at the store,
/// before it.
///
/// # Safety
/// As `lane_body_capture`.
pub(in crate::vm) unsafe extern "C" fn lane_body_capture_store<H: Host>(_vm: u64, _residual: u64, context: u64, packed: u64, value: u64) -> u64 {
    if !crate::DYNAMIC_EXECUTION_ENABLED || context == 0 { return 0; }
    let cx = context as usize as *mut LaneContext<H>;
    let (depth, slot, value) = ((packed >> u16::BITS) as u16, packed as u16, Value::from_bits(value));
    let stored = match native_frames::innermost_callee_env(cx) {
        Some(env) => {
            // SAFETY: the context's program outlives the lane run.
            let (code, program) = unsafe { (&*(*cx).code, (*cx).program) };
            lane_environment_store(unsafe { vm(cx) }, code, program, env, depth, slot, value)
        }
        None => (depth == 0 && lane_store_own_capture(cx, slot, value)) || lane_capture_store(cx, depth, slot, value),
    };
    u64::from(stored)
}

/// Whether a body's run that ended at `exit` counts toward its retirement and
/// reoptimization: a leave into a callee that only lacked a body while warming toward one
/// (`LaneContext::callee_warming`) does not, as the next run may find the callee's body.
pub(super) fn lane_leave_counts<H: Host>(cx: *mut LaneContext<H>, exit: super::compiled::LaneExit) -> bool {
    // SAFETY: the running lane's context, read and reset by the run that ended.
    let warming = unsafe { std::mem::take(&mut (*cx).callee_warming) };
    !(warming && exit == super::compiled::LaneExit::Callee)
}

/// Where the lane continues after a run of `method`, `own`'s body, on the activation whose
/// registers are `r`: the activations of the native frames it left inside first
/// (`native_frames::advance_into_frames`), then the record its leave names in the innermost, or the
/// innermost's completion; `lane_body_left` adds the leave as feedback (`LaneMethod::feedback`).
pub(super) enum LaneBodyLeft {
    /// Irreversible cancellation leaves the lane without dispatching another record.
    Terminated,
    Resume(Registers, Ip, Carried),
    /// The activation's body raised an exception (`Vm::lane_raised`) at the record: the
    /// lane unwinds from it, or the call that made the activation pops it (`lane_pop_raised`).
    Raised(Ip),
    /// The activation completes with the value, by the record.
    Completed(Ip, Value),
}



/// Records that a run of `own`'s body ended at `exit`, in the diagnostics report's leave table;
/// nothing the lane does next reads it.
#[inline(always)]
pub(super) fn lane_record_end<H: Host>(cx: *mut LaneContext<H>, own: &LaneView, method: &super::compiled::LaneMethod, exit: super::compiled::LaneExit) {
    if unsafe { vm(cx) }.programs.js_jit_diagnostics_enabled() {
        own.tier().body_stats.ended(exit, &method.exits, &method.inlined, super::compiled::take_lane_exit_line());
    }
}

/// `LaneView::note_lane_body_end`, counting the retirement it makes in the VM's tier events.
#[inline(always)]
pub(super) fn lane_note_body_end<H: Host>(cx: *mut LaneContext<H>, own: &LaneView, exit: super::compiled::LaneExit, left_new: impl FnOnce((Option<u32>, usize))) {
    if own.note_lane_body_end(exit, false, left_new) {
        unsafe { vm(cx) }.programs.tier_events().record(Event::Retired);
    }
}

/// Counts an entry whose body the lane could not leave inside of (`lane_body_admits`): in the VM's
/// tier events and, with diagnostics, the view's report.
#[cold]
pub(super) fn lane_body_refused<H: Host>(vm: &Vm<H>, own: &LaneView) {
    vm.programs.tier_events().record(Event::Refused);
    if vm.programs.js_jit_diagnostics_enabled() {
        own.tier().body_stats.refused.set(own.tier().body_stats.refused.get() + 1);
    }
}

pub(super) fn lane_body_left<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    own: &LaneView,
    method: &super::compiled::LaneMethod,
    exit: super::compiled::LaneExit,
) -> (LaneBodyLeft, super::compiled::LaneExit) {
    lane_record_end(cx, own, method, exit);
    if exit == super::compiled::LaneExit::Terminated {
        // A fault can bypass the native exit's register snapshots. Discard its
        // native frames instead of materializing stale homes for guest execution.
        native_frames::discard_frames(cx);
        return (LaneBodyLeft::Terminated, exit);
    }
    let feedback = method.feedback(exit, unsafe { &*(*cx).code });
    (lane_frames_resumed(cx, r, (own, Some(method)), exit), feedback)
}

/// The activation a leave resumes in: the one the leaving body ran in for a native call's callee, else
/// the innermost frame a native-frame advance made the running one, where there is one.
pub(super) fn lane_resumed_activation<'a, H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    (own, root): (&'a LaneView, Option<&'a super::compiled::LaneMethod>),
    callee: bool,
) -> (Registers, &'a LaneView, Option<&'a super::compiled::LaneMethod>) {
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
pub(super) fn lane_frames_resumed<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    (own, root): (&LaneView, Option<&super::compiled::LaneMethod>),
    exit: super::compiled::LaneExit,
) -> LaneBodyLeft {
    let callee = matches!(exit, super::compiled::LaneExit::Callee);
    let (r, own, method) = lane_resumed_activation(cx, r, (own, root), callee);
    // SAFETY: the running activation's records start at the context's base.
    let base = unsafe { (*cx).base };
    match exit.resumed() {
        // SAFETY: the record that raised is one of the activation's function.
        super::compiled::LaneExit::Threw { pc } => LaneBodyLeft::Raised(unsafe { base.add(pc) }),
        super::compiled::LaneExit::Terminated | super::compiled::LaneExit::Entry | super::compiled::LaneExit::Stub => unreachable!("termination leaves before recovery; an entry leave or an unentered call resumes at its first record"),
        super::compiled::LaneExit::At(pc) => {
            lane_body_resume_at(cx, r, own, loop_body::header_for_tier_up(cx, own, pc).unwrap_or(pc))
        }
        super::compiled::LaneExit::Inlined { frame, pc } => {
            let method = method.expect("a leave inside an inlined callee is a lane body's");
            let (registers, resume, carried) = lane_body_inlined_activation(cx, r, base, InlinedRun::of_body(method), frame, pc);
            LaneBodyLeft::Resume(registers, resume, carried)
        }
        // A native call made its callee's activation and left the lane there.
        super::compiled::LaneExit::Callee => {
            let mut carried = Carried::script(cx);
            // SAFETY: `lane_body_call` set both before the body left.
            let (registers, resume) = unsafe { ((*cx).registers, (*cx).callee_resume) };
            carried.acc = Slot::of(unsafe { (*cx).callee_acc });
            LaneBodyLeft::Resume(registers, resume, carried)
        }
        // SAFETY: the record that completes the activation is one of its function's.
        super::compiled::LaneExit::Returned { pc, value } => LaneBodyLeft::Completed(unsafe { base.add(pc) }, value),
    }
}
