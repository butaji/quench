//! Tier entry of a lane body: the tier prologue and resumption rows, the concluded and resumed exits of a body, direct tier calls, and the direct targets of a call site.
use super::*;

/// Where the lane continues after a frameless activation's tier prologue (`tier_prologue_in_lane`): at the record
/// a compiled body's result completes (`Vm::js_tier_activation_entry`), result in its register, else at its first
/// record. Shared by the prologue's staged row (`EntryHost`) and its hand body.
#[inline(always)]
pub(in super::super) fn tier_prologue_resumed(cx: *mut LaneContext, r: Registers, view: Carried, function: u32, params: usize) -> Resumed {
    let vm = unsafe { vm(cx) };
    // SAFETY: the activation just set the context to its own function's.
    let base = unsafe { (*cx).base };
    // SAFETY: the program's views outlive the lane run.
    let own = unsafe { &*(*cx).views }.get(function as usize).and_then(quench_runtime::vm::program_store::derived_lane_view);
    // A function of a program with a loaded AOT image enters the image's body at its first entry.
    // SAFETY: the prologue is this record, written when the tier claimed the function (`claim_for_tier`).
    if let Some(own) = own && vm.images_run_in_lane() && vm.programs.ahead_images(unsafe { (*cx).program }, unsafe { &*(*cx).code }).is_some() {
        let prologue = own.prologue.get().cast_const();
        return body_calls::enter_published_body(cx, prologue, r, view, function, unsafe { (*prologue).b } != 0);
    }
    // SAFETY: the activation's registers hold its parameters from its first.
    let parameters = unsafe { std::slice::from_raw_parts(r.0, params) };
    let entered = own
        .and_then(|own| tier_direct_in_lane(vm, own, parameters))
        .or_else(|| tier_enter_in_lane(cx, function, own, parameters));
    if let Some((pc, register, value)) = entered {
        r.set(register, value);
        // SAFETY: the tier returns a PC of the function, whose records are contiguous.
        return Resumed::At(r, unsafe { base.add(pc) }, Carried::script_in(cx, view));
    }
    Resumed::At(r, base, Carried::script_in(cx, view))
}

/// The protocol row of a frameless activation's tier prologue record (`TierPrologue`): the staged prologue
/// row (`PrologueFrame::tier_prologue`), with `tier_prologue_in_lane` as the body it completes where it leaves.
pub(in super::super) const fn tier_prologue_row() -> Handler {
    protocol_row!({ quench_runtime::bytecode::TIER_PROLOGUE as u16 }, ACC_NONE, { protocol_family::ENTRY | protocol_misses::entry::TIER_PROLOGUE as u16 })
}

/// The protocol row of a specialized body's prologue record (`BodyEntry`): the staged prologue row
/// (`PrologueFrame::body_entry`), with `body_entry_in_lane` as the body it completes where it leaves.
pub(in super::super) const fn body_entry_row() -> Handler {
    protocol_row!({ quench_runtime::bytecode::BODY_ENTRY as u16 }, ACC_NONE, { protocol_family::ENTRY | protocol_misses::entry::BODY_ENTRY as u16 })
}

/// The receiver word a body is entered with: the activation's `this` (bound by `direct_call_this_in_lane`)
/// where the body reads it (`LaneView::binds_this`), else `DELETED`, at which its `LoadThis` leaves.
#[inline(always)]
pub(in super::super) fn entry_receiver_in_lane(cx: *mut LaneContext, binds_this: bool) -> Value {
    // SAFETY: the context is the running activation's, or the leaf's callee.
    if binds_this { unsafe { this_in_lane(cx, (*cx).registers) }.unwrap_or(Value::DELETED) } else { Value::DELETED }
}

/// Where the lane continues after a run of `method`, `own`'s body, that ran on the
/// activation `cx` holds, with registers `r`, and ended at `exit`: a returned run completes
/// the activation (no leave to resolve, no frame to materialize), any other leave resumes
/// where it names (`body_left_in_lane`).
#[inline(always)]
pub(in super::super) fn body_concluded_in_lane(cx: *mut LaneContext, r: Registers, ip: Ip, own: &LaneView, method: &crate::compiled::LaneMethod, left: crate::compiled::LaneExit) -> Resumed {
    let base = unsafe { (*cx).base };
    if let crate::compiled::LaneExit::Returned { pc, value } = left {
        record_end_in_lane(cx, own, method, left);
        // SAFETY: as `leave_counts_in_lane`; a completed run carries no warming callee.
        unsafe { (*cx).callee_warming = false };
        // SAFETY: the record that completes the activation is one of its function's.
        return returned_in_lane(cx, unsafe { base.add(pc) }, value);
    }
    let counts = leave_counts_in_lane(cx, left);
    let resumed = body_left_in_lane(cx, r, own, method, left);
    if !counts { note_warming_leave_in_lane(cx, own) }
    body_resumed_in_lane(cx, ip, resumed)
}

/// Where the lane continues after a lane body's resolved leave `resumed`, which left at `ip`.
#[inline(always)]
pub(in super::super) fn body_resumed_in_lane(cx: *mut LaneContext, ip: Ip, resumed: LaneBodyLeft) -> Resumed {
    match resumed {
        LaneBodyLeft::Terminated => Resumed::Exit(exit(ip)),
        LaneBodyLeft::Resume(_, resume, _) if body_calls::left_lane(resume) => Resumed::Exit(resume),
        LaneBodyLeft::Resume(registers, resume, carried) => Resumed::At(registers, resume, carried),
        LaneBodyLeft::Raised(record) => Resumed::Exit(raised_exit_in_lane(cx, record)),
        LaneBodyLeft::Completed(record, value) => returned_in_lane(cx, record, value),
    }
}

/// `lane_return_value!` as the resumption it continues at.
#[inline(always)]
pub(in super::super) fn returned_in_lane(cx: *mut LaneContext, record: Ip, value: Value) -> Resumed {
    complete_plain_activation_in_lane::<NOT_LEAF>(cx, value).unwrap_or_else(|| return_value_general_in_lane(cx, record, value))
}

/// The tier prologue's run of the compiled body `own` keeps for a body that reads only its
/// parameters (`LaneView::tier_direct`): the `Return` to continue at, its register and the result.
#[inline(always)]
pub(in super::super) fn tier_direct_in_lane(vm: &mut Vm, own: &LaneView, parameters: &[Value]) -> Option<(usize, quench_runtime::bytecode::Register, Value)> {
    let direct = own.tier_direct.take()?;
    let entered = vm.run_direct_tier_entry(&direct, parameters);
    if !direct.retired() { own.tier_direct.set(Some(direct)); }
    entered
}

/// The tier prologue's entry where no direct run completes the activation (`Vm::js_tier_activation_entry`).
/// A completed entry keeps the direct run the compiled body admits; once the tier declines the
/// function, its activations start at the first record again (`LaneView::release_from_tier`).
/// A lane body runs on the activation itself, so it takes the function from the frame-entering
/// tier; a program with a loaded AOT image entered its body already (`tier_prologue_resumed`).
#[cold]
#[inline(never)]
pub(in super::super) fn tier_enter_in_lane(cx: *mut LaneContext, function: u32, own: Option<&LaneView>, parameters: &[Value]) -> Option<(usize, quench_runtime::bytecode::Register, Value)> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the activation just set the context to its own function's.
    let (code, program) = unsafe { (&*(*cx).code, (*cx).program) };
    if quench_runtime::vm::tier_policy::lane_bodies_claim(&vm.programs) && let Some(own) = own {
        if !specialize_in_lane(cx, own, function, parameters) { own.release_from_tier(); }
        return None;
    }
    // A function only lane bodies claimed (`claim_for_tier`) has no JS-tier entry to try.
    let may_claim = vm.js_tier_may_claim(code, program, function);
    if may_claim && let Some(entered) = vm.js_tier_activation_entry(code, program, function, parameters) {
        if let Some(own) = own && let Some(direct) = vm.direct_tier_entry(code, program, function) { own.tier_direct.set(Some(direct)); }
        return Some(entered);
    }
    if let Some(own) = own && !may_claim { own.release_from_tier(); }
    None
}

/// The bodies the call at `pc` of function `function` may call, as the closed world proves them
/// (`FunctionFacts::proven_call_targets`), each bound as a plain function of the running program:
/// what a lane body inlines (`lane_unit::unit`). Where no target is proven, the body's own inlined
/// calls (`lane_unit::InlinedCall`) are the targets, else none: the general call.
pub(crate) fn site_targets_in_lane(vm: &Vm, view: &LaneView, function: u32, pc: u32) -> Vec<crate::compiled::CallTarget> {
    let proven: Vec<crate::compiled::CallTarget> = vm.programs.aot_facts(vm.active_program).map_or_else(Vec::new, |facts| {
        facts.function_facts(function).proven_call_targets(pc).into_iter()
            .map(|body| crate::compiled::CallTarget { callee: crate::compiled::CalleeBinding::Function, function: body })
            .take(crate::compiled::tier_policy::POLYMORPHISM)
            .collect()
    });
    if !proven.is_empty() {
        return proven;
    }
    view.body_view_in_lane().map_or_else(Vec::new, |method| {
        method.inlined.iter()
            .filter(|frame| frame.parent.is_none() && frame.call == pc)
            .map(|frame| frame.target)
            .collect()
    })
}

/// The dense capture slots of `callee`'s environment (`Vm::callee_capture_slots`).
pub(in super::super) fn callee_captures_in_lane(cx: *mut LaneContext, callee: Value) -> (usize, usize) {
    unsafe { vm(cx) }.callee_capture_slots(callee)
}

/// Whether a lane body's run may call bodies by their compiled entries: the lane may make
/// their activations should it leave inside one (`body_call_helper`'s own conditions).
pub(crate) fn direct_calls_in_lane(cx: *mut LaneContext) -> bool {
    let vm = unsafe { vm(cx) };
    let script_calls = unsafe { (*cx).script_calls };
    // A direct callee's registers follow the caller's window, where an activation that keeps its
    // locals there (`context_locals_in_window_in_lane`) has them: such a run calls through the lane.
    script_calls && !vm.with_scope_entered && vm.construct_target.is_none() && !unsafe { context_locals_in_window_in_lane(cx as usize) }
}
