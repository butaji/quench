use super::*;

/// A function's code as the unit splices it: per PC, the call it inlines there, if any.
pub(super) struct Plan {
    pub(super) code: Vec<WideInstruction>,
    /// Per PC, the callees its call inlines, as a switch on the callee; empty for none.
    pub(super) calls: Vec<Vec<InlinedPlan>>,
    /// Per root PC, the instructions a self tail call there becomes (`self_tail`); empty for an inlined
    /// callee and for a root with no self tail call.
    pub(super) tails: Vec<Option<Vec<WideInstruction>>>,
}

pub(super) struct InlinedPlan {
    pub(super) target: CallTarget,
    pub(super) recovery: CallRecovery,
    /// The callee's register 0 in the unit.
    pub(super) base: u32,
    pub(super) registers: u16,
    pub(super) plan: Plan,
}

impl Plan {
    /// The unit PCs the function takes where `root` keeps its own instructions; an inlined
    /// callee's completions add their moves and jumps.
    pub(super) fn length(&self, root: bool) -> u32 {
        self.lengths(root).sum()
    }

    /// The unit PCs each instruction takes, in order.
    pub(super) fn lengths(&self, root: bool) -> impl Iterator<Item = u32> + '_ {
        self.code.iter().zip(&self.calls).enumerate().map(move |(pc, (instruction, calls))| match calls.as_slice() {
            [] if root => self.tail(pc).map_or(1, |tail| tail.len() as u32),
            [] => completion_length(*instruction),
            alternatives => call_length(alternatives) + u32::from(self.keeps_call(pc, root)),
        })
    }

    /// The instructions root PC `pc` becomes, where it is a self tail call (`self_tail`).
    pub(super) fn tail(&self, pc: usize) -> Option<&[WideInstruction]> {
        self.tails.get(pc)?.as_deref()
    }

    /// Whether the caller's call at `pc` keeps its own `Call` after its last alternative, which
    /// a callee none of several alternatives matched takes, not a leave of the unit.
    pub(super) fn keeps_call(&self, pc: usize, root: bool) -> bool {
        root && self.calls[pc].len() > 1
    }
}

pub(super) struct Planner<'t, 'f> {
    pub(super) program: &'t quench_runtime::ResidualProgram,
    /// The root's inlined call sites, as (PC, callee): the host's decision (`tier_policy::inline_plan`), which the planner
    /// lowers and never re-decides.
    pub(super) inlined: &'t [(u32, u32)],
    /// The callees each call site reaches: its call plan's arms (`CallPlans::targets`), which carry each arm's binding.
    pub(super) targets: &'t dyn Fn(u32, u32) -> Vec<CallTarget>,
    pub(super) body: &'t dyn Fn(u32) -> Option<&'f Function>,
    /// The unit's root function: a call to it is never inlined.
    pub(super) root: u32,
    pub(super) registers: u32,
}

impl Planner<'_, '_> {
    /// The plan of `function`'s `code`, each call its site's inlined callees (`inline`), in PC order.
    pub(super) fn plan(&mut self, function: u32, code: Vec<WideInstruction>) -> Plan {
        let calls = code.iter().enumerate().map(|(pc, instruction)| self.inline(function, pc as u32, *instruction)).collect();
        Plan { code, calls, tails: Vec::new() }
    }

    /// The callees the call at `pc` of `function` inlines, as the call's alternatives: the root's inlined sites
    /// (`inlined`), each callee's target of the site's call plan, lowered whole (`inline_body`). A callee the lowering
    /// refuses keeps the call. A callee's own calls are never inlined: the host's scope admits no callee with a call.
    pub(super) fn inline(&mut self, function: u32, pc: u32, instruction: WideInstruction) -> Vec<InlinedPlan> {
        if function != self.root || !Operation::<u64, ()>::is_plain_call(instruction) {
            return Vec::new();
        }
        let wanted: Vec<u32> = self.inlined.iter().filter(|(at, _)| *at == pc).map(|&(_, callee)| callee).collect();
        if wanted.is_empty() {
            return Vec::new();
        }
        let found: Vec<CallTarget> = (self.targets)(function, pc).into_iter()
            .filter(|target| target.function != self.root && wanted.contains(&target.function)).collect();
        found.into_iter().filter_map(|target| self.inline_body(target)).collect()
    }

    /// The callee of `target`, lowered whole, if its body renames and its activation is admitted (`lowerable`,
    /// `admit_callee`: the engine's lowering, which the host's plan does not decide). Its registers are reserved here.
    pub(super) fn inline_body(&mut self, target: CallTarget) -> Option<InlinedPlan> {
        let callee = (self.body)(target.function)?;
        let code = decoded(callee);
        if !lowerable(&code) || admit_callee(self.program, callee, CalleeUse::Inlined(target.callee)).is_err() {
            return None;
        }
        let recovery = if super::super::lane_region::restartable_numeric_function(self.program, callee, code.iter().copied())
            || (target.callee == CalleeBinding::Function && restartable(self.program, callee, &code)) {
            CallRecovery::RestartCall
        } else { CallRecovery::Materialize };
        let base = self.registers;
        self.registers += u32::from(callee.registers);
        let plan = self.plan(target.function, code);
        Some(InlinedPlan { target, recovery, base, registers: callee.registers, plan })
    }
}

/// Whether `function` inlines whole into a lane unit: the inlining rule admits it (`tier_policy::inlines`) and its
/// code renames (`lowerable`). The inline form of a call plan's arm of this function (`call_plans`).
pub(crate) fn inlines_whole(graph: Option<&CallGraph>, program: &quench_runtime::ResidualProgram, function: u32) -> bool {
    let Some(callee) = program.function_get(function as usize) else { return false };
    graph.is_some_and(|graph| super::super::tier_policy::inlines(graph, program, function)) && lowerable(&decoded(callee))
}

/// Whether every instruction of an inlined callee renames, and one of them completes it (a
/// multi-instruction completion or a returning call).
pub(super) fn lowerable(code: &[WideInstruction]) -> bool {
    code.iter().all(|instruction| inlinable(*instruction, InlineScope::Lane) && renamable(*instruction))
        && code.iter().any(|instruction| completion_length(*instruction) > 1
            || (Operation::<u64, ()>::is_plain_call(*instruction) && instruction.returns_from_frame()))
}

/// The instructions the self tail call at `pc` of the root `function` (`function_id`) becomes, where it
/// re-enters the function as a fresh activation would: each argument copied to a temporary at `temps`
/// and then to its parameter register, each missing parameter and every other register undefined (the
/// constant `undefined`), then a jump to `SELF_TAIL_HEADER`. None where the call is not a self tail call:
/// a plain call of the function itself whose activation nothing observes past the call, in a body that
/// reads neither `this` nor `arguments` and captures no environment.
pub(super) fn self_tail(program: &quench_runtime::ResidualProgram, function_id: u32, function: &Function, plans: &dyn Fn(u32, u32) -> Option<CallPlan>,
    code: &[WideInstruction], pc: usize, temps: u32, undefined: u32) -> Option<Vec<WideInstruction>> {
    let instruction = *code.get(pc)?;
    let call = pc as u32;
    if !Operation::<u64, ()>::is_plain_call(instruction) || !instruction.returns_from_frame()
        || instruction.result_flags() != RETURN_REGISTER || instruction.direct_eval() || instruction.parameter_eval()
        || !tail_site(true, function.permits_js_tail_call_at(call)) {
        return None;
    }
    if function.is_async || function.is_generator || function.is_class_constructor || function.derived_constructor
        || function.rest || function.rest_alias || function.arguments_slot.is_some() {
        return None;
    }
    let plan = plans(function_id, call)?;
    let self_arm = matches!(&*plan.arms, [arm] if arm.class.is_none() && arm.function == function_id);
    let activation = function.call_plan(program);
    if !self_arm || !activation.registers_only || activation.addresses_locals || activation.legacy_reflection
        || activation.admits_direct_eval || activation.forwards_arguments || activation.keeps_arguments {
        return None;
    }
    if code.iter().any(|other| other.op().captures_environment() || other.op().reads_arguments()
        || other.direct_eval() || other.parameter_eval() || matches!(Operation::<u64, ()>::of(*other), Some(Operation::LoadThis { .. }))) {
        return None;
    }
    let argc = ImmediateLayout::argument_count(instruction.imm());
    if argc > function.params { return None; }
    let window = ImmediateLayout::call_window_base(instruction.imm());
    let params: Vec<Register> = (0..function.params)
        .map(|index| function.parameter_registers.map_or_else(|| function.promoted_register(index), |base| Some(base + index)))
        .collect::<Option<_>>()?;
    let temp = |index: u16| Register::try_from(temps + u32::from(index)).ok();
    let load_undefined = |register: Register| WideInstruction::new(Op::LoadConst, register, 0, 0, undefined);
    let mut tail = Vec::with_capacity(2 * usize::from(argc) + usize::from(function.registers) + 1);
    for index in 0..argc {
        tail.push(super::super::meaning_compiled::move_record(temp(index)?, window + index));
    }
    for index in 0..argc {
        tail.push(super::super::meaning_compiled::move_record(params[usize::from(index)], temp(index)?));
    }
    tail.extend(params[usize::from(argc)..].iter().map(|&param| load_undefined(param)));
    tail.extend((0..function.registers).filter(|register| !params.contains(register)).map(|register| load_undefined(register)));
    tail.push(super::super::meaning_compiled::jump_record(SELF_TAIL_HEADER));
    Some(tail)
}

