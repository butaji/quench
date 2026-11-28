//! The callees a lane body calls by their entries, and the native frames of those calls.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

pub(crate) use quench_runtime::vm::engine_types::{DirectArm, DirectBody};
use crate::dispatch_fast::{capture_reads, capture_stores, this_binding::ThisBinding, LaneView, ScriptCallTarget};
use quench_runtime::bytecode::ImmediateLayout;

#[quench_macros::ext(DirectArmOps1)]
impl DirectArm {
    /// The type summary of the arm's published body (`lane_region::TypeSummary`): its entry
    /// parameter types, which the call's arguments must hold for its proven entry, and its result,
    /// which types the call's result. `None` where the body is not published or has none.
    pub(crate) fn summary(&self) -> Option<lane_region::TypeSummary> {
        self.body.method.as_ref()?.lease()?.summary().cloned()
    }

    /// The global object a sloppy body binds for an `undefined` or `null` receiver, which binds a direct object
    /// itself (`receiver_this_in_lane`): the realm's, folded in the arm's operands; `None` for a strict body or an
    /// arrow that never reads `this`, which take any receiver as it is.
    pub(crate) fn sloppy(&self) -> Option<Value> {
        matches!(self.body.target.this_binding, ThisBinding::Sloppy).then_some(self.closure.lexical_this)
    }
}

/// The closure the native header at `window` entered, the callee word its call wrote from the callee register
/// (`CONTEXT_CALLEE`; a spilled header's words stay below its window), with the environment and captures pointer read
/// from that closure (`Vm::resolve_script_call_target`).
pub(crate) fn entered_closure(vm: &mut Vm, code: &ResidualProgram, window: *mut Value) -> (Value, Value, *const usize) {
    // SAFETY: the header's words lie below its window.
    let entered = unsafe { *window.sub(quench_runtime::staged::frame::CONTEXT_CALLEE) };
    let target = vm.resolve_script_call_target(code, entered).expect("a closure a direct call entered resolves to its body's target");
    (entered, target.closure.env, target.closure.capture)
}

/// The environment a running native frame's captures resolve in: its window's, which its link wrote from the closure
/// its call entered (`entered_closure`); `top_native` names linked frames alone.
pub(crate) fn frame_env(frame: quench_runtime::vm::engine_types::ActivationView) -> Value {
    // SAFETY: a linked frame's window holds its context words.
    unsafe { *(frame.window as *const Value).sub(quench_runtime::staged::frame::CONTEXT_ENV) }
}

/// The arms of a direct call site, one per body, ascending body id (`direct_callees_in_lane`).
pub(crate) type DirectCandidates = smallvec::SmallVec<[DirectArm; tier_policy::POLYMORPHISM]>;

/// Whether code that runs `inlined` as their callees' rows and calls `direct` by their entries
/// names a callee that `live` says died (an arm's folded closure), or a callee body that retired: a collection that frees one
/// retires the code (`ProgramStore::retire_dead_native_calls`).
pub(crate) fn names_dead_callee(inlined: &[InlinedCall], direct: &[DirectArm], live: impl Fn(Value) -> bool) -> bool {
    inlined.iter().any(|call| call.target.identity().is_some_and(|callee| !live(callee)))
        || direct.iter().any(|arm| !live(arm.closure.callee) || arm.body.method.as_ref().is_some_and(|method| !method.is_published()))
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

/// Per PC of `body`, the arms of its call site: one per body the closed world says the call may reach
/// (`FunctionFacts::call_targets`), ascending body id, at most `POLYMORPHISM` (an open, empty or wider set calls
/// generically), each where the lane would activate the body in place, with an ordinary `this`, from a plain call of at
/// most its parameters, and the body's one closure is created (`ProgramStore::closure_cell`): the arm calls that closure,
/// whose operands it folds (`DirectArm`). Every tier builds its direct calls from these (b-jit plans, u-jit lane units, OSR).
pub(crate) fn direct_callees_in_lane(vm: &mut Vm, view: &LaneView, function: u32, program: &quench_runtime::bytecode::ResidualProgram, body: &quench_runtime::bytecode::Function) -> Vec<crate::compiled::DirectCandidates> {
    let Some(facts) = vm.programs.aot_facts(vm.active_program) else { return vec![DirectCandidates::new(); body.code.len()] };
    let facts = facts.function_facts(function);
    let sites: Vec<(usize, u16, Vec<u32>)> = body.code.iter().enumerate().filter_map(|(pc, packed)| {
        let instruction = body.widened(*packed);
        let direct = quench_runtime::staged::calls(instruction.op()) && !instruction.direct_eval() && !instruction.parameter_eval()
            && !quench_runtime::compile::call_plan::tail_site(instruction.returns_from_frame(), body.permits_js_tail_call_at(pc as u32))
            // A construction through a forwarding constructor names no direct callee: its forwarding resolution is no plan's.
            && !quench_runtime::staged::construction_family(instruction.op());
        let argc = direct.then(|| direct_argc(program, instruction)).flatten()?;
        let mut bodies = facts.call_targets(pc as u32);
        bodies.sort_unstable();
        bodies.dedup();
        (bodies.len() <= crate::compiled::tier_policy::POLYMORPHISM).then_some((pc, argc, bodies))
    }).collect();
    let mut candidates = vec![DirectCandidates::new(); body.code.len()];
    for (pc, argc, bodies) in sites {
        candidates[pc] = bodies.into_iter().filter_map(|callee| {
            let closure = vm.programs.closure_cell(vm.active_program, callee)?;
            let target = vm.resolve_call_target_in_lane(program, closure)?;
            direct_arm_in_lane(vm, view, function, pc, argc, target)
        }).collect();
    }
    candidates
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

/// The arm that calls `target`, the closure the call at `pc` of `view` passes `argc` arguments, where the lane would
/// activate its body in place: the body (`DirectBody`) and the closure's operands, folded.
pub(crate) fn direct_arm_in_lane(vm: &Vm, view: &LaneView, function: u32, pc: usize, argc: u16, target: ScriptCallTarget) -> Option<DirectArm> {
    // No tier's direct row binds an arrow's lexical `this` (Cranelift cannot), so an arrow calls direct only where it
    // never reads `this`, and as strict.
    let lexical_this = match target.body.this_binding {
        ThisBinding::Sloppy => vm.realm.globals,
        ThisBinding::Strict => Value::UNDEFINED,
        ThisBinding::Lexical => {
            vm.programs.residual(target.body.admission.program()).filter(|code| !quench_runtime::bytecode::activation_reads_this(code, code.function(target.body.function as usize)))?;
            Value::UNDEFINED
        }
        _ => return None,
    };
    (target.body.admission.current(vm.lane_stack.admission)
        && target.body.entry.activates() && !target.body.view.is_null() && argc <= target.body.params).then_some(())?;
    // SAFETY: a resolved target's view outlives its program.
    let view_of = unsafe { &*target.body.view };
    let (method, body) = view_of.pinned_callee(target.body.function == function);
    // SAFETY: a live environment's slots never move (`ScriptCallTarget::capture`).
    let (values, len) = unsafe { quench_runtime::tier_surface::EnvironmentSlots::native_storage_view(capture_reads(target.closure.capture)) }
        .unwrap_or((std::ptr::null(), 0));
    Some(DirectArm {
        body: DirectBody { target: target.body, method, body, call: view.records().wrapping_add(pc).addr(), caller_base: view.records().addr(), cell: view_of.entry_cell() },
        closure: crate::dispatch_fast::ClosureOperands { lexical_this, ..target.closure },
        captures: crate::compiled::entry_cell::capture_word(values, len, capture_stores(target.closure.capture).is_some()),
    })
}
