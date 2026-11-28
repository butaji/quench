//! The callees a lane body calls by their entries, and the native frames of those calls.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

pub(crate) use quench_runtime::vm::engine_types::DirectCallee;
use crate::dispatch_fast::{body_tier_in_lane, capture_reads, capture_stores, construct_plan::holding_construct_plan, site_targets_in_lane, this_binding::ThisBinding, LaneView, ScriptCallTarget, SkippedForwarder};
use quench_runtime::bytecode::ImmediateLayout;

#[quench_macros::ext(DirectCalleeOps1)]
impl DirectCallee {
    /// The type summary of the callee's published body (`lane_region::TypeSummary`): its entry
    /// parameter types, which the call's arguments must hold for its proven entry, and its result,
    /// which types the call's result. `None` where the callee has no published body or its body has none.
    pub(crate) fn summary(&self) -> Option<lane_region::TypeSummary> {
        self.method.as_ref()?.lease()?.summary().cloned()
    }

    /// The callee's dense capture slots, in the one word a body's entry takes them in.
    pub(crate) fn capture_word(&self) -> u64 {
        super::entry_cell::capture_word(self.capture_values, self.capture_len, self.capture_stores)
    }
}

/// The environment and captures of the closure the native header at `window` entered (`CONTEXT_CALLEE`): its
/// descriptor's, or those of the closure its site rebound to (`DirectCallee::bound`), another of the same function.
pub(crate) fn header_closure(vm: &mut Vm, code: &ResidualProgram, callee: &DirectCallee, window: *mut Value) -> (Value, *const usize) {
    // SAFETY: the header's words lie below its window.
    let entered = unsafe { *window.sub(quench_runtime::staged::frame::CONTEXT_CALLEE) };
    // A site that reads its bound words may have entered a closure at its own closure's reused locator: resolve it.
    if entered == callee.callee && !callee.bound.read.get() { return (callee.target.env, callee.target.capture); }
    let target = vm.resolve_script_call_target(code, entered).expect("a rebound closure resolves to its function's target");
    (target.env, target.capture)
}

/// The environment a running native frame's captures resolve in: its descriptor's, or where it entered another closure
/// (a linked header of a rebound site), its window's, which the link wrote (`header_closure`).
pub(crate) fn frame_env(frame: quench_runtime::vm::engine_types::ActivationView) -> Value {
    // SAFETY: a frame names the caller-owned callee, whose descriptor outlives the run; a header's window is linked.
    let callee = unsafe { &*frame.callee };
    match frame.kind {
        quench_runtime::vm::engine_types::ActivationKind::Script(script) if script.callee != callee.callee || callee.bound.read.get() => unsafe { *(frame.window as *const Value).sub(quench_runtime::staged::frame::CONTEXT_ENV) },
        _ => callee.target.env,
    }
}

/// Rebinds the running region's direct calls at `ip` (`LaneContext::region_calls`) to `target`, another closure of the
/// function they call (`DirectCallee::bound`): the guard of a closure made per call of the caller misses once, not on
/// every call. Only a closure the lane would call the same way rebinds: the same cell, receiver binding, window and
/// capture slots, and only where the code reads the bound words (a b-jit record rebinds itself, `rebind_region_call`).
pub(crate) fn rebind_closure(cx: *mut quench_runtime::vm::engine_types::LaneContext, ip: quench_runtime::vm::engine_types::Ip, target: &crate::dispatch_fast::ScriptCallTarget, argc: u16) {
    // SAFETY: the running region's code (and its `CallCode`) outlives its run, which named it.
    let Some(calls) = (unsafe { ((*cx).region_calls as *const quench_runtime::vm::engine_types::CallCode).as_ref() }).filter(|_| unsafe { (*cx).region_payload } != 0) else { return };
    let (call, word) = (ip.addr(), target.callee.to_bits());
    let mut sites = calls.direct.iter().filter(|direct| direct.call == call && direct.bound.read.get() && direct.target.function == target.function).peekable();
    if sites.peek().is_none() { return; }
    // SAFETY: the running lane's context; its decoded site is the call being made.
    let (vm, Some((_, function, pc, _))) = (unsafe { &*(*cx).vm }, unsafe { (*cx).body_site }) else { return };
    let Some(caller) = (unsafe { &*(*cx).views }).get(function as usize).and_then(quench_runtime::vm::program_store::derived_lane_view) else { return };
    let mut fresh = None;
    for direct in sites {
        if direct.bound.callee.get() == word { continue; }
        let Some(fresh) = fresh.get_or_insert_with(|| crate::dispatch_fast::direct_callee_in_lane(vm, caller, function, pc as usize, argc, *target)).as_ref() else { return };
        if fresh.call != call || (fresh.cell, fresh.sloppy, fresh.registers, fresh.capture_len, fresh.capture_stores) != (direct.cell, direct.sloppy, direct.registers, direct.capture_len, direct.capture_stores) { continue; }
        direct.bound.callee.set(word);
        direct.bound.captures.set(fresh.capture_word());
    }
}

/// The callees a call site reached that a body may call by their entries, the latest first.
pub(crate) type DirectCandidates = smallvec::SmallVec<[DirectCallee; tier_policy::POLYMORPHISM]>;

/// Whether code that runs `inlined` as their callees' rows and calls `direct` by their entries
/// names a callee that `live` says died, or a callee body that retired: a collection that frees one
/// retires the code (`ProgramStore::retire_dead_native_calls`).
pub(crate) fn names_dead_callee(inlined: &[InlinedCall], direct: &[DirectCallee], live: impl Fn(Value) -> bool) -> bool {
    inlined.iter().any(|call| call.target.identity().is_some_and(|callee| !live(callee)))
        || direct.iter().any(|callee| !live(callee.callee) || callee.method.as_ref().is_some_and(|method| !method.is_published()))
}

/// The registers past the caller's `caller_registers` that `inlined` occupy: the scratch window a
/// leave inside one reads (`Vm::scratch_in_lane`), and the room the frames past the caller's registers
/// need.
pub(crate) fn window_width(inlined: &[InlinedCall], caller_registers: u16) -> usize {
    frames_window(inlined.iter().map(|frame| (frame.base, frame.registers)), caller_registers)
}

/// `window_width` of frames given by their first register and their count, for a region whose
/// frames are not `InlinedCall`s (`Inlining::window`).
pub(crate) fn frames_window(frames: impl Iterator<Item = (quench_runtime::bytecode::Register, u16)>, caller_registers: u16) -> usize {
    frames.map(|(base, registers)| usize::from(base - caller_registers) + usize::from(registers)).max().unwrap_or(0)
}

/// Per PC of `body`, the callees its call site reached where the callee's own lane body
/// specialized and the lane would activate it in place, with an ordinary `this`, from a
/// plain call of at most its parameters: what a lane body calls by its compiled entry
/// (`compiled::DirectCallee`), the latest first.
pub(crate) fn direct_callees_in_lane(vm: &Vm, view: &LaneView, function: u32, program: &quench_runtime::bytecode::ResidualProgram, body: &quench_runtime::bytecode::Function) -> Vec<crate::compiled::DirectCandidates> {
    body.code.iter().enumerate().map(|(pc, packed)| {
        let instruction = body.widened(*packed);
        let direct = quench_runtime::staged::calls(instruction.op()) && !instruction.direct_eval() && !instruction.parameter_eval()
            && !quench_runtime::compile::call_plan::tail_site(instruction.returns_from_frame(), body.permits_js_tail_call_at(pc as u32));
        let mut callees = crate::compiled::DirectCandidates::new();
        if quench_runtime::staged::construction_family(instruction.op()) {
            callees.extend(construct_callee_in_lane(vm, view, function, pc, ImmediateLayout::argument_count(instruction.imm())));
            return callees;
        }
        let Some(argc) = direct_argc(program, instruction) else { return callees };
        callees.extend(body_tier_in_lane::site_targets(vm, view, pc).filter(|_| direct)
            .filter_map(|target| direct_callee_in_lane(vm, view, function, pc, argc, target)));
        // A site that made a closure of one function per run calls the latest, and rebinds to the next (`BoundClosure`).
        if callees.len() == crate::compiled::tier_policy::POLYMORPHISM && callees.iter().all(|callee| callee.target.function == callees[0].target.function) { callees.truncate(1); }
        callees
    }).collect()
}

/// The arguments a call instruction passes its direct row: a method call's are its method site's count, which only the
/// site names (`None` where the program holds no such site).
pub(crate) fn direct_argc(program: &quench_runtime::bytecode::ResidualProgram, instruction: WideInstruction) -> Option<u16> {
    if instruction.op() as u16 == quench_runtime::staged::METHOD_CALL_ROW {
        quench_runtime::staged::DecodeContext::of(program).method_site(instruction.method_site_index()).map(|site| site.argument_count)
    } else {
        Some(ImmediateLayout::argument_count(instruction.imm()))
    }
}

/// The `compiled::DirectCallee` of the construction at `pc` of `view` through a forwarding
/// constructor: the forwarded initializer the site last ran (`construct_keeping_activation_in_lane`)
/// as the callee, the constructor as the identity it guards and the activation it skips.
fn construct_callee_in_lane(vm: &Vm, view: &LaneView, function: u32, pc: usize, argc: u16) -> Option<crate::compiled::DirectCallee> {
    for site in site_targets_in_lane(vm, view, pc as u32) {
        let crate::compiled::CalleeBinding::Forwarded { through, callee } = site.callee else { continue };
        // SAFETY: as `Vm::script_call_entry`; both targets are copied before anything resolves one.
        let (constructor, initializer) = unsafe { (*vm.script_call_entry(through)?, *vm.script_call_entry(callee)?) };
        let method = *vm.lane_stack.forwarded_methods.get(constructor.forwarded_method as usize)?;
        if !constructor.forwards() || !constructor.constructs() || holding_construct_plan(vm, through).is_none_or(|plan| plan.forwarded.is_none()) {
            return None;
        }
        let mut direct = direct_callee_in_lane(vm, view, function, pc, argc, initializer)?;
        direct.callee = through;
        direct.bound = quench_runtime::vm::engine_types::BoundClosure::new(through.to_bits(), direct.capture_word());
        direct.forwarder = Some(SkippedForwarder { function: constructor.function, callee: through, env: constructor.env, method });
        return Some(direct);
    }
    None
}

/// The `compiled::DirectCallee` of `target`, the callee the call at `pc` of `view` passes
/// `argc` arguments, where the lane would activate it in place.
pub(crate) fn direct_callee_in_lane(vm: &Vm, view: &LaneView, function: u32, pc: usize, argc: u16, target: ScriptCallTarget) -> Option<crate::compiled::DirectCallee> {
    // No tier's direct row binds an arrow's lexical `this` (Cranelift cannot), so an arrow calls direct only where it
    // never reads `this`, and as strict.
    let sloppy = match target.this_binding {
        ThisBinding::Sloppy => Some(vm.realm.globals),
        ThisBinding::Strict => None,
        ThisBinding::Lexical => {
            vm.programs.residual(target.admission.program()).filter(|code| !quench_runtime::bytecode::activation_reads_this(code, code.function(target.function as usize)))?;
            None
        }
        _ => return None,
    };
    (target.admission.current(vm.lane_stack.admission)
        && target.entry.activates() && !target.view.is_null() && argc <= target.params).then_some(())?;
    // SAFETY: a resolved target's view outlives its program.
    let view_of = unsafe { &*target.view };
    let (method, body) = view_of.pinned_callee(target.function == function);
    // SAFETY: a live environment's slots never move (`ScriptCallTarget::capture`).
    let (capture_values, capture_len) = unsafe { quench_runtime::tier_surface::EnvironmentSlots::native_storage_view(capture_reads(target.capture)) }
        .unwrap_or((std::ptr::null(), 0));
    Some(crate::compiled::DirectCallee {
        target, method, body, call: view.records().wrapping_add(pc).addr(), caller_base: view.records().addr(), cell: view_of.entry_cell(),
        callee: target.callee, registers: target.registers, sloppy, capture_values, capture_len, capture_stores: capture_stores(target.capture).is_some(), forwarder: None,
        bound: quench_runtime::vm::engine_types::BoundClosure::new(target.callee.to_bits(), crate::compiled::entry_cell::capture_word(capture_values, capture_len, capture_stores(target.capture).is_some())),
    })
}

/// The most closures one region's records rebind to (`rebind_region_call`): past it the site's calls keep the
/// helper, so code calling a fresh closure each time does not grow its descriptors without end.
const REGION_REBINDS: usize = 64;

/// Rebinds the running loop region's record of `call` (`LaneContext::region_calls`) to `target`, a closure of the
/// function the record was planned for whose identity guard missed: the record's run-time words become the new
/// closure's (its identity, captures, receiver binding, context words and descriptor), as `BoundCalls::bind` makes
/// an image's, so its row calls this closure by its entry from now on instead of this helper. A closure made per
/// call of the region's function (`const f = (x) => ..` above the loop) is the common case.
///
/// # Safety
/// `target` is the resolved script target of the call at `ip`, a call of the running region's activation.
pub(crate) unsafe fn rebind_region_call(cx: *mut quench_runtime::vm::engine_types::LaneContext, vm: &Vm, ip: quench_runtime::vm::engine_types::Ip, argc: u16, target: &ScriptCallTarget) {
    let calls = unsafe { (*cx).region_calls } as *const quench_runtime::vm::engine_types::CallCode;
    // SAFETY: the region's code (and its `CallCode`) outlives its run, which named it.
    let Some(calls) = (unsafe { calls.as_ref() }) else { return };
    let site = ip.addr() as u64;
    let Some(record) = calls.records.iter().find(|record| record.call == site && record.callee != 0) else { return };
    if record.callee == target.callee.to_bits() || record.packed & u64::from(u32::MAX) != u64::from(target.function) { return; }
    let mut rebound = calls.rebound.borrow_mut();
    if rebound.len() >= REGION_REBINDS { return; }
    // SAFETY: the context's decoded site is the call being made.
    let Some((_, function, pc, _)) = (unsafe { (*cx).body_site }) else { return };
    let Some(view) = (unsafe { &*(*cx).views }).get(function as usize).and_then(quench_runtime::vm::program_store::derived_lane_view) else { return };
    let Some(mut direct) = direct_callee_in_lane(vm, view, function, pc as usize, argc, *target) else { return };
    if direct.call as u64 != site || direct.cell as u64 != record.cell { return; }
    (direct.method, direct.body) = (None, None);
    rebound.push(Box::new(direct));
    let Some(direct) = rebound.last() else { return };
    let (env, capture) = direct.target.context_words();
    let words = std::ptr::from_ref(&**record).cast_mut();
    // SAFETY: the record is the region's, read in place by its code on this thread, none of which runs while
    // its helper does; the descriptor is boxed in `rebound`, which keeps it for the code's life.
    unsafe {
        (*words).captures = direct.capture_word();
        (*words).global_this = direct.sloppy.map_or(0, Value::to_bits);
        (*words).descriptor = std::ptr::from_ref(&**direct) as u64;
        (*words).env = env;
        (*words).capture = capture;
        (*words).callee = direct.callee.to_bits();
    }
}
