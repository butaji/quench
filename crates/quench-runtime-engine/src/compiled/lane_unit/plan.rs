use super::*;

/// A function's code as the unit splices it: per PC, the call it inlines there, if any.
pub(super) struct Plan {
    pub(super) code: Vec<WideInstruction>,
    /// Per PC, the callees its call inlines, as a switch on the callee; empty for none.
    pub(super) calls: Vec<Vec<InlinedPlan>>,
    /// Whether a call the plan does not inline has run (its site has a call plan): an
    /// inlined callee's never-run call is an exit of the unit, one that runs is not.
    pub(super) blocked: bool,
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
    /// The program's call graph, which the inlining rule reads (`tier_policy::inlines`, `charges_growth`); None where the
    /// program has none, so no callee inlines.
    pub(super) graph: Option<&'t CallGraph>,
    /// The callees each call site inlines: its call plan's arms (`CallPlans::targets`), less those the caller excludes.
    pub(super) targets: &'t dyn Fn(u32, u32) -> Vec<CallTarget>,
    /// The call plan of each call site, by function and PC: a site inlines only where its plan's arms all inline.
    pub(super) plans: &'t dyn Fn(u32, u32) -> Option<CallPlan>,
    pub(super) body: &'t dyn Fn(u32) -> Option<&'f Function>,
    /// The unit's root function: a call to it is never inlined.
    pub(super) root: u32,
    pub(super) registers: u32,
    /// The growth the unit has left to add, in callee instructions (`tier_policy::INLINE_GROWTH_PERCENT`).
    pub(super) budget: usize,
}

impl Planner<'_, '_> {
    /// The plan of `function`'s `code`, its calls inlined where the rule and the growth cap allow, in PC order. A callee
    /// is inlined only whole: every call of its own inlined too.
    pub(super) fn plan(&mut self, function: u32, code: Vec<WideInstruction>, strict: bool, depth: usize) -> Plan {
        let mut blocked = false;
        let calls = code
            .iter()
            .enumerate()
            .map(|(pc, instruction)| {
                // Strict terminal calls retain the VM's proper-tail-call policy.
                // A root completion must name a real Return, never the completed Call.
                let (calls, ran) = if Operation::<u64, ()>::is_plain_call(*instruction) && instruction.returns_from_frame()
                    && (strict || (depth == 0 && terminal_return(&code).is_none())) {
                    (Vec::new(), true)
                } else { self.inline(function, pc as u32, *instruction, depth) };
                blocked |= calls.is_empty() && ran;
                calls
            })
            .collect();
        Plan { code, calls, blocked, tails: Vec::new() }
    }

    /// The callees the call at `pc` of `function` inlines: every arm of its call plan, as the call's switch, where each
    /// arm inlines whole (`inlines_whole`) and the growth cap admits it. A site with no plan, or with an arm that does not
    /// inline, takes the call. With whether the call has run: a call its plan covers counts as run.
    pub(super) fn inline(&mut self, function: u32, pc: u32, instruction: WideInstruction, depth: usize) -> (Vec<InlinedPlan>, bool) {
        if !Operation::<u64, ()>::is_plain_call(instruction) {
            return (Vec::new(), false);
        }
        if !returns_or_keeps(instruction) || instruction.direct_eval()
            || instruction.parameter_eval() || depth >= MAX_INLINING_DEPTH
        {
            return (Vec::new(), true);
        }
        let plan = (self.plans)(function, pc);
        let ran = plan.is_some();
        let found: Vec<CallTarget> = match plan {
            Some(plan) if plan.arms.iter().all(|arm| inlines_whole(self.graph, self.program, arm.function)) => (self.targets)(function, pc)
                .into_iter().filter(|target| target.function != self.root).collect(),
            _ => Vec::new(),
        };
        // A call of an inlined callee that inlines none of its targets leaves the unit at every run (the lowering
        // makes calls from the caller's own frame only): it blocks its callee, which stays its own unit.
        if depth > 0 && found.is_empty() {
            return (Vec::new(), true);
        }
        let (targets, reserved) = (found.len(), (self.registers, self.budget));
        // A target that does not inline gives back what it reserved (`inline_body`).
        let alternatives: Vec<_> = found.into_iter().filter_map(|target| self.inline_body(target, instruction, depth)).collect();
        // A site of a callee that misses a target would leave the unit there: its callee stays its own unit.
        if depth > 0 && alternatives.len() < targets {
            (self.registers, self.budget) = reserved;
            return (Vec::new(), true);
        }
        (alternatives, ran)
    }

    /// Whether inlining `callee` is charged to the growth cap: it copies its body (`tier_policy::charges_growth`).
    pub(super) fn charged(&self, callee: u32) -> bool {
        self.graph.is_some_and(|graph| charges_growth(graph, callee))
    }

    /// Whether the callee's code fits the growth the unit has left: a callee that adds no growth always fits.
    pub(super) fn fits(&self, callee: u32, size: usize) -> bool {
        !self.charged(callee) || size <= self.budget
    }

    /// Whether the callee's code fits the inlining cap and the call's arguments fit its registers.
    pub(super) fn within_budget(&self, target: CallTarget, instruction: WideInstruction, callee: &Function, code: &[WideInstruction]) -> bool {
        matches!(self.entry(target, callee, code.len()), CallEntry::Inline(_))
            && ImmediateLayout::argument_count(instruction.imm()) <= callee.registers
    }

    /// The entry the call's callee takes, which the staged planner decides (`staged::plan_entry`): a
    /// call plan's arm is Static, and the callee inlines where the rule admits it (`tier_policy::inlines`) and
    /// its `size` fits the growth cap (`fits`).
    pub(super) fn entry(&self, target: CallTarget, callee: &Function, size: usize) -> CallEntry {
        let binding = match target.callee {
            CalleeBinding::Function => BindingTime::Static,
            CalleeBinding::Identity(_) | CalleeBinding::Forwarded { .. } => BindingTime::Stable,
        };
        let plan = DirectPlan { kind: CalleeKind::Script, identity: Identity::Guarded(target.identity().map_or(0, Value::to_bits)),
            sloppy: None, registers: callee.registers, returned: super::super::LANE_BODY_RETURNED, bound: None };
        let inlines = self.graph.is_some_and(|graph| super::super::tier_policy::inlines(graph, self.program, target.function))
            && self.fits(target.function, size);
        plan_entry(Some(Resolved { plan, binding }), inlines)
    }

    /// The callee of `target`, whole, if the rule admits it within the growth cap (`entry`) and its body renames and is
    /// admitted. An inlined callee copies its body into the unit, charged to the cap unless it has one call site.
    pub(super) fn inline_body(&mut self, target: CallTarget, instruction: WideInstruction, depth: usize) -> Option<InlinedPlan> {
        let callee = (self.body)(target.function)?;
        let code = decoded(callee);
        if !self.within_budget(target, instruction, callee, &code) || !lowerable(&code)
            || admit_callee(self.program, callee, CalleeUse::Inlined(target.callee)).is_err() {
            return None;
        }
        let recovery = if super::super::lane_region::restartable_numeric_function(self.program, callee, code.iter().copied())
            || (target.callee == CalleeBinding::Function && restartable(self.program, callee, &code)) {
            CallRecovery::RestartCall
        } else { CallRecovery::Materialize };
        // The callee's registers and growth are reserved before its own calls plan, and given
        // back if one of them is not inlined.
        let (registers, budget) = (self.registers, self.budget);
        let base = self.registers;
        self.registers += u32::from(callee.registers);
        if self.charged(target.function) { self.budget -= code.len(); }
        let plan = self.plan(target.function, code, callee.strict, depth + 1);
        if plan.blocked {
            (self.registers, self.budget) = (registers, budget);
            return None;
        }
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
pub(super) fn self_tail(function_id: u32, function: &Function, plans: &dyn Fn(u32, u32) -> Option<CallPlan>,
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
    if !self_arm || !plan.registers_only || plan.addresses_locals || plan.legacy_reflection
        || plan.admits_direct_eval || plan.forwards_arguments || plan.keeps_arguments {
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

