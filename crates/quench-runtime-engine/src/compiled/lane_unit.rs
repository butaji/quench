//! The unit a lane body specializes: one function with the callees its call sites' inline
//! caches name spliced in, transitively, under one budget (R3, Linear TOD-1232).
//!
//! A call whose record links one script target (`LaneInstruction::call_link`), to a callee
//! whose body the lane-body lowering runs entirely and every call of which is spliced in in
//! turn, is replaced by a call marker and the callee's renamed instructions. Its registers
//! follow the unit's; its `this` is the call's receiver register; its `Return` moves the
//! value to the call's result and jumps past the call. Each PC of the unit names its origin,
//! so a leave inside a callee makes the activations of its frame and the frames around it,
//! outermost first, through the lane's own call path, and continues at the innermost one's
//! record (`Vm::body_inlined_activation_in_lane`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::bytecode::{Constant, FieldBase, FieldLayout, FieldLookup, ImmediateLayout, ImmediateRole, InstructionField, Op, Operand, OperandKind, Register, RETURN_REGISTER, WideInstruction};
use quench_runtime::bytecode::Function;
use quench_runtime::compile::call_plan::{tail_site, CallPlan};
use quench_runtime::staged::{plan_entry, CallEntry, CalleeKind, DirectPlan, Identity, Operation, Resolved};
use quench_runtime::typed_ir::BindingTime;
use quench_runtime::value::Value;
#[path = "lane_unit/admission.rs"] pub(crate) mod admission;
pub(crate) use admission::{inlinable, InlineScope};
use admission::{restartable, returns_or_keeps};
use super::admission::{admit_callee, CalleeUse};
#[path = "lane_unit/weights.rs"] pub(crate) mod weights;
#[cfg(test)] #[path = "lane_unit/static_hints.rs"] pub(crate) mod static_hints;
#[cfg(test)] pub(crate) use static_hints::static_call_targets;

pub(crate) use quench_runtime::vm::engine_types::CalleeBinding;

pub(crate) use quench_runtime::vm::engine_types::CallRecovery;

pub(crate) use quench_runtime::vm::engine_types::CallTarget;

#[quench_macros::ext(CallTargetOps1)]
impl CallTarget {
    /// The forwarding native this site called the target through.
    pub(crate) fn through(self) -> Option<Value> {
        match self.callee { CalleeBinding::Forwarded { through, .. } => Some(through), _ => None }
    }
}

/// The unit PC a self tail call jumps to: the root's first instruction, which the unit reaches
/// through a jump at PC 0, so the entry is entered once and PC 1 is the loop header.
const SELF_TAIL_HEADER: u32 = 1;

/// Where a unit PC comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    /// The caller's instruction at this PC.
    Caller(u32),
    /// The instruction at `pc` of inlined `frame`.
    Inlined { frame: usize, pc: u32 },
}

pub(crate) use quench_runtime::vm::engine_types::InlinedCall;

#[quench_macros::ext(InlinedCallOps1)]
impl InlinedCall {
    /// The physical parameter homes initialized by the passed arguments.
    pub(crate) fn passed_registers(self, callee: &Function) -> impl Iterator<Item = (Register, Register)> + '_ {
        (0..self.arguments.min(callee.params)).filter_map(move |argument|
            callee.parameter_registers.map(|base| base + argument)
                .or_else(|| callee.promoted_register(argument))
                .map(|register| (self.base + register, self.window + argument)))
    }

    /// The parent's register 0: the caller's, or the enclosing callee's.
    pub(crate) fn parent_base(&self, frames: &[InlinedCall]) -> Register {
        self.parent.map_or(0, |parent| frames[parent].base)
    }
}

#[derive(Clone)]
pub(crate) struct Unit {
    pub(crate) instructions: Vec<WideInstruction>,
    pub(crate) origins: Vec<Origin>,
    pub(crate) frames: Vec<InlinedCall>,
    /// The unit PC of each frame's call marker, where the call's guards and bindings run.
    pub(crate) markers: Vec<usize>,
    pub(crate) caller_registers: u16,
    pub(crate) registers: u16,
    /// The caller PC past its last instruction, where falling off the unit resumes.
    pub(crate) end: u32,
}

impl Unit {
    /// `frame` and the frames around it, outermost first: the activations a leave inside it
    /// makes.
    pub(crate) fn chain(&self, frame: usize) -> Vec<usize> {
        let mut chain = vec![frame];
        while let Some(parent) = self.frames[*chain.last().expect("a chain has a frame")].parent {
            chain.push(parent);
        }
        chain.reverse();
        chain
    }

    /// Maximum simultaneous inlined activations, derived from the parent graph.
    pub(crate) fn recovery_depth(&self) -> usize {
        (0..self.frames.len()).map(|index| self.chain(index).len()).max().unwrap_or(0)
    }

    /// Project the original functions' recovery liveness onto unit registers. A suspended
    /// frame retains both its call inputs and continuation state, and so do the frames
    /// around it; callee facts include implicit observations such as legacy f.arguments.
    /// The unit's own CFG then refines the projection, so uses the splice introduces (the
    /// result move after a returning operation, for example) stay live. `None` for a unit
    /// wider than one register set, whose lowering projects each frame's own masks per
    /// query instead (`Lowering::is_live`).
    pub(crate) fn register_masks<'a>(
        &self, program: &'a quench_runtime::ResidualProgram, caller: u32,
    ) -> Option<std::borrow::Cow<'a, quench_runtime::compile::liveness::RegisterMasks>> {
        use quench_runtime::compile::liveness::{ROOT_MAP_REGISTERS, RegisterMasks, RegisterSet, register_bit};
        let caller_masks = program.register_root_masks(caller).filter(|masks| !masks.is_windowed())?;
        if self.frames.is_empty() && self.origins.iter().enumerate().all(|(pc, origin)|
            *origin == Origin::Caller(pc as u32)) {
            return Some(std::borrow::Cow::Borrowed(caller_masks));
        }
        if self.registers > ROOT_MAP_REGISTERS { return None; }
        // Frames follow their parents (`Emitter::emit`).
        let mut suspended: Vec<RegisterSet> = Vec::with_capacity(self.frames.len());
        let mut callees = Vec::with_capacity(self.frames.len());
        for frame in &self.frames {
            let (parent, outer) = match frame.parent {
                None => (caller_masks, RegisterSet::default()),
                Some(parent) => (callees[parent], suspended[parent]),
            };
            let call = frame.call as usize;
            let around = parent.get(call)? | (parent.get(call + 1).unwrap_or_default() & !frame.result.map_or_else(RegisterSet::default, |result| register_bit(result - frame.parent_base(&self.frames))));
            suspended.push(outer | around.checked_shl(u32::from(frame.parent_base(&self.frames)))?);
            callees.push(program.register_root_masks(frame.target.function).filter(|masks| !masks.is_windowed())?);
        }
        let mut masks = self.origins.iter().map(|origin| match *origin {
            Origin::Caller(pc) => caller_masks.get(pc as usize),
            Origin::Inlined { frame, pc } => Some(suspended[frame]
                | callees[frame].get(pc as usize)?.checked_shl(u32::from(self.frames[frame].base))?),
        }).collect::<Option<Vec<_>>>()?;
        masks.push(caller_masks.get(self.end as usize)?);
        let mut transfers: Vec<_> = self.instructions.iter().map(|instruction|
            quench_runtime::compile::liveness::transfer(*instruction, program,
                &program.field_sites, &program.superinstructions)).collect();
        // A marker defines its callee's registers: call admission binds every one.
        for (frame, pc) in self.frames.iter().zip(&self.markers) {
            transfers[*pc].1 = (0..frame.registers).fold(RegisterSet::default(), |mask, register| mask | register_bit(frame.base + register));
        }
        // A returning instruction continues at the function's terminal `Return` where it has
        // one, whose register then holds its value; otherwise it completes the activation.
        let terminal = self.instructions.len().checked_sub(1)
            .filter(|terminal| matches!(Operation::<u64, ()>::of(self.instructions[*terminal]), Some(Operation::Return { .. })));
        let mut starts = Vec::with_capacity(self.instructions.len() + 1);
        let mut successors = Vec::new();
        for (pc, instruction) in self.instructions.iter().enumerate() {
            starts.push(successors.len());
            if instruction.returns_from_frame() {
                if let Some(terminal) = terminal {
                    transfers[pc].1 |= register_bit(self.instructions[terminal].register_a());
                    successors.push(terminal);
                }
                continue;
            }
            if let Some(frame) = self.markers.iter().position(|marker| *marker == pc) {
                successors.extend(self.frames[frame].alternative);
            }
            // The one successor table: a branch table continues at every one of its entries.
            quench_runtime::bytecode::for_each_instruction_successor(pc, *instruction, |successor| successors.push(successor));
        }
        starts.push(successors.len());
        let edges = vec![quench_runtime::bytecode::ControlEdge::Normal; successors.len()];
        let table = quench_runtime::bytecode::Successors::from_parts(&starts, &successors, &edges);
        let live = quench_runtime::compile::liveness::analyze_graph(&transfers, &table, |pc| masks[pc]);
        Some(std::borrow::Cow::Owned(RegisterMasks::new(live)))
    }
}

pub(crate) use crate::compiled::tier_policy::MAX_UNIT_INLINED_INSTRUCTIONS;
/// How deep calls nest inside a unit: JSC's inlining depth (`maximumInliningDepth`).
const MAX_INLINING_DEPTH: usize = 5;

/// The unit PCs occupied by a call's alternatives, including each admission marker.
/// Returning calls continue at an existing completion; they add no synthetic `Return`.
fn call_length(alternatives: &[InlinedPlan]) -> u32 {
    alternatives.iter().map(|callee| 1 + callee.plan.length(false)).sum()
}

/// The canonical caller completion a terminal call may use without replaying the call.
fn terminal_return(code: &[WideInstruction]) -> Option<usize> {
    code.len().checked_sub(1).filter(|pc|
        matches!(Operation::<u64, ()>::of(code[*pc]), Some(Operation::Return { .. })) && code[*pc].result_flags() == 0)
}

/// Whether the staged decode makes `instruction` a throw of its first operand (`Throw`).
fn throws(instruction: WideInstruction) -> bool {
    matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Throw { .. }))
}

/// Whether an inlined callee's `instruction` renames to the unit's registers
/// (`renamed`): its operands are registers or constants. The plan decides it, so a
/// unit it plans always emits.
fn renamable(instruction: WideInstruction) -> bool {
    match instruction.op() {
        _ if quench_runtime::staged::Operation::<u64, ()>::is_unit_rewritten(instruction) => true,
        _ if site_field_read(instruction) => true,
        _ => {
            let plain = if inlined_returning(instruction) {
                WideInstruction::new(instruction.op(), instruction.result_register(), instruction.b(), instruction.c(), instruction.imm())
            } else {
                instruction
            };
            renamed(plain, &|register| register, 0, &|target| Some(target)).is_some()
        }
    }
}

/// Whether `instruction` reads a field through its lookup site (`FieldLookup::Site`, a nested
/// lookup's), returning or not: the site keeps its base, which the lowering renames, so the record
/// keeps its operands and only its result register renames.
fn site_field_read(instruction: WideInstruction) -> bool {
    matches!(crate::compiled::decoded_completed(instruction), Some(quench_runtime::staged::Operation::GetField { .. }))
        && matches!(instruction.field_lookup(), Some(FieldLookup::Site(_)))
}

/// The unit PCs an inlined callee instruction takes: a completion of the call (a `Return`, or
/// an instruction that computes and returns) adds the result's move and the jump past the call.
fn completion_length(instruction: WideInstruction) -> u32 {
    if matches!(Operation::<u64, ()>::of(instruction), Some(Operation::Return { .. })) {
        2
    } else if inlined_returning(instruction) {
        3
    } else {
        1
    }
}

fn decoded(function: &Function) -> Vec<WideInstruction> {
    function.code.iter().map(|packed| function.widened(*packed)).collect()
}

/// A function's code as the unit splices it: per PC, the call it inlines there, if any.
struct Plan {
    code: Vec<WideInstruction>,
    /// Per PC, the callees its call inlines, as a switch on the callee; empty for none.
    calls: Vec<Vec<InlinedPlan>>,
    /// Whether a call the plan does not inline has run (its site linked a target): an
    /// inlined callee's never-run call is an exit of the unit, one that runs is not.
    blocked: bool,
    /// Per PC, the register and identity of the forwarding native an inlined call through it
    /// guards; its instruction in `code` is then the call of the function it calls.
    through_call: Vec<Option<(Register, Value)>>,
    /// Per root PC, the instructions a self tail call there becomes (`self_tail`); empty for an inlined
    /// callee and for a root with no self tail call.
    tails: Vec<Option<Vec<WideInstruction>>>,
}

struct InlinedPlan {
    target: CallTarget,
    recovery: CallRecovery,
    /// The callee's register 0 in the unit.
    base: u32,
    registers: u16,
    plan: Plan,
}

impl Plan {
    /// The unit PCs the function takes where `root` keeps its own instructions; an inlined
    /// callee's completions add their moves and jumps.
    fn length(&self, root: bool) -> u32 {
        self.lengths(root).sum()
    }

    /// The unit PCs each instruction takes, in order.
    fn lengths(&self, root: bool) -> impl Iterator<Item = u32> + '_ {
        self.code.iter().zip(&self.calls).enumerate().map(move |(pc, (instruction, calls))| match calls.as_slice() {
            [] if root => self.tail(pc).map_or(1, |tail| tail.len() as u32),
            [] => completion_length(*instruction),
            alternatives => call_length(alternatives) + u32::from(self.keeps_call(pc, root)),
        })
    }

    /// The instructions root PC `pc` becomes, where it is a self tail call (`self_tail`).
    fn tail(&self, pc: usize) -> Option<&[WideInstruction]> {
        self.tails.get(pc)?.as_deref()
    }

    /// Whether the caller's call at `pc` keeps its own `Call` after its last alternative, which
    /// a callee none of several alternatives matched takes, not a leave of the unit.
    fn keeps_call(&self, pc: usize, root: bool) -> bool {
        root && self.calls[pc].len() > 1 && self.through_call[pc].is_none()
    }
}

struct Planner<'t, 'f> {
    program: &'t quench_runtime::ResidualProgram,
    targets: &'t dyn Fn(u32, u32) -> Vec<CallTarget>,
    /// The call plan of each call site, by function and PC, where one covers it: it decides the site's inlining.
    plans: &'t dyn Fn(u32, u32) -> Option<CallPlan>,
    body: &'t dyn Fn(u32) -> Option<&'f Function>,
    registers: u32,
    budget: usize,
    /// The longest callee body one call inlines (`LaneBody::max_inlined_instructions`).
    max_inlined: usize,
    /// The loop trip counts the caller's run observed, by function and loop header.
    trips: &'t dyn Fn(u32, u32) -> Option<u64>,
    /// The expected executions of the call being planned, relative to the caller's entry.
    weight: u64,
    /// The calls from the caller to the one being planned, by PC and callee.
    path: weights::SitePath,
    choice: weights::Choice,
}

impl Planner<'_, '_> {
    /// The plan of `function`'s `code`, its calls inlined where the budget allows. A callee
    /// is inlined only whole: every call of its own inlined too.
    fn plan(&mut self, function: u32, mut code: Vec<WideInstruction>, strict: bool, depth: usize) -> Plan {
        let mut blocked = false;
        let mut through_call = vec![None; code.len()];
        let mut called = Vec::new();
        let site_weights = weights::loop_weights(&code, &|header| (self.trips)(function, header));
        let calls = code
            .iter()
            .enumerate()
            .map(|(pc, instruction)| {
                // A call through a forwarding native inlines as the call of the function it calls.
                let through = self.through_call(function, pc as u32, *instruction);
                // Strict terminal calls retain the VM's proper-tail-call policy.
                // A root completion must name a real Return, never the completed Call.
                let (calls, ran) = if Operation::<u64, ()>::is_plain_call(*instruction) && instruction.returns_from_frame()
                    && (strict || (depth == 0 && terminal_return(&code).is_none())) {
                    (Vec::new(), true)
                } else { self.inline(function, pc as u32, through.map_or(*instruction, |(call, _)| call), depth, through.is_some(), self.weight.saturating_mul(site_weights[pc])) };
                blocked |= calls.is_empty() && ran;
                if let Some((call, native)) = through.filter(|_| !calls.is_empty()) {
                    called.push((pc, call));
                    through_call[pc] = Some(native);
                }
                calls
            })
            .collect();
        for (pc, call) in called {
            code[pc] = call;
        }
        Plan { code, calls, blocked, through_call, tails: Vec::new() }
    }

    /// The call at `pc` of `function` as the call of the function it forwards to, where its
    /// site called its targets through a forwarding native (`CalleeBinding::Forwarded`): the
    /// receiver is the callee and the first argument the receiver. With the native's register
    /// and identity, which the inlined call guards.
    fn through_call(&self, function: u32, pc: u32, instruction: WideInstruction) -> Option<(WideInstruction, (Register, Value))> {
        let imm = instruction.imm();
        let (base, count) = (ImmediateLayout::call_window_base(imm), ImmediateLayout::argument_count(imm));
        (Operation::<u64, ()>::is_plain_call(instruction) && count > 0 && !instruction.direct_eval() && !instruction.parameter_eval()).then_some(())?;
        let through = (self.targets)(function, pc).into_iter().find_map(CallTarget::through)?;
        let call = super::meaning_compiled::call_record(instruction.a(), instruction.register_c(), base,
            ImmediateLayout::call_immediate(base + 1, count - 1, false, false));
        Some((call, (instruction.register_b(), through)))
    }

    /// The callees the call at `pc` of `function` inlines: each target its site linked that
    /// inlines within the budget, as the call's switch; any other callee takes the call.
    /// With whether the call has run: a call the plan may not inline counts as run.
    fn inline(&mut self, function: u32, pc: u32, instruction: WideInstruction, depth: usize, forwarded: bool, weight: u64) -> (Vec<InlinedPlan>, bool) {
        if !Operation::<u64, ()>::is_plain_call(instruction) {
            return (Vec::new(), false);
        }
        if !returns_or_keeps(instruction) || instruction.direct_eval()
            || instruction.parameter_eval() || depth >= MAX_INLINING_DEPTH
        {
            return (Vec::new(), true);
        }
        // A forwarded call inlines the targets reached through the native; a plain call, the others.
        let linked: Vec<_> = (self.targets)(function, pc).into_iter().filter_map(|target| match target.callee {
            CalleeBinding::Forwarded { callee, .. } => forwarded.then_some(CallTarget { callee: CalleeBinding::Identity(callee), ..target }),
            _ => (!forwarded).then_some(target),
        }).collect();
        let ran = !linked.is_empty();
        // The site's plan decides what inlines where it covers the site: every arm inlines whole, and then the
        // linked targets its arms name (a class switch's arms, once one exists); else nothing. A forwarded call's
        // targets are its native's, which no plan names, so they keep the linked rule alone.
        let found = match (self.plans)(function, pc).filter(|_| !forwarded) {
            Some(plan) if plan.arms.iter().all(|arm| inlines_whole(self.program, arm.function)) => linked.into_iter()
                .filter(|target| plan.arms.iter().any(|arm| arm.function == target.function)).collect(),
            Some(_) => Vec::new(),
            None => linked,
        };
        // A call of an inlined callee that inlines none of its targets leaves the unit at every run (the lowering
        // makes calls from the caller's own frame only): it blocks its callee, which stays its own unit.
        if depth > 0 && found.is_empty() {
            return (Vec::new(), true);
        }
        let (targets, reserved, mark) = (found.len(), (self.registers, self.budget), self.survey_len());
        // The alternatives share the site's executions equally: the record keeps no split.
        let share = (weight / (targets.max(1) as u64)).max(1);
        // A target that does not inline gives back what it reserved (`inline_target`).
        let alternatives: Vec<_> = found.into_iter().filter_map(|target| self.inline_target(target, pc, instruction, depth, share)).collect();
        // A site of a callee that misses a target would leave the unit there: its callee stays its own unit.
        if depth > 0 && alternatives.len() < targets {
            (self.registers, self.budget) = reserved;
            self.forget_after(mark);
            return (Vec::new(), true);
        }
        (alternatives, ran)
    }

    /// Whether the callee's code fits the inlining budget and the call's arguments fit its registers.
    fn within_budget(&self, target: CallTarget, instruction: WideInstruction, callee: &Function, code: &[WideInstruction]) -> bool {
        matches!(self.entry(target, callee, code.len()), CallEntry::Inline(_))
            && ImmediateLayout::argument_count(instruction.imm()) <= callee.registers
    }

    /// The entry the call's callee takes, which the staged planner decides (`staged::plan_entry`): a
    /// linked identity is Stable, a function binding Static, and the callee's body inlines within
    /// the unit's remaining budget, capped by the policy's longest inlined body.
    fn entry(&self, target: CallTarget, callee: &Function, size: usize) -> CallEntry {
        let binding = match target.callee {
            CalleeBinding::Function => BindingTime::Static,
            CalleeBinding::Identity(_) | CalleeBinding::Forwarded { .. } => BindingTime::Stable,
        };
        let plan = DirectPlan { kind: CalleeKind::Script, identity: Identity::Guarded(target.identity().map_or(0, Value::to_bits)),
            sloppy: None, registers: callee.registers, returned: super::LANE_BODY_RETURNED, bound: None };
        let budget = self.budget.min(self.max_inlined);
        plan_entry(Some(Resolved { plan, binding, body: Some(size) }), budget)
    }

    /// The callee of `target`, whole, if it inlines within the budget (`inline_target` chooses).
    fn inline_body(&mut self, target: CallTarget, instruction: WideInstruction, depth: usize) -> Option<InlinedPlan> {
        let callee = (self.body)(target.function)?;
        let code = decoded(callee);
        if !self.within_budget(target, instruction, callee, &code) || !lowerable(&code)
            || admit_callee(self.program, callee, CalleeUse::Inlined(target.callee)).is_err() {
            return None;
        }
        let recovery = if super::lane_region::restartable_numeric_function(self.program, callee, code.iter().copied())
            || (target.callee == CalleeBinding::Function && restartable(self.program, callee, &code)) {
            CallRecovery::RestartCall
        } else { CallRecovery::Materialize };
        // The callee's registers and budget are reserved before its own calls plan, and given
        // back if one of them is not inlined.
        let (registers, budget) = (self.registers, self.budget);
        let base = self.registers;
        self.registers += u32::from(callee.registers);
        self.budget -= code.len();
        let plan = self.plan(target.function, code, callee.strict, depth + 1);
        if plan.blocked {
            (self.registers, self.budget) = (registers, budget);
            return None;
        }
        Some(InlinedPlan { target, recovery, base, registers: callee.registers, plan })
    }
}

/// Whether `function` inlines whole into a lane unit: its code renames (`lowerable`) and fits the longest
/// body a lane inlines. The inline form of a call plan's arm of this function (`call_plans`).
pub(crate) fn inlines_whole(program: &quench_runtime::ResidualProgram, function: u32) -> bool {
    let Some(callee) = program.function_get(function as usize) else { return false };
    let code = decoded(callee);
    code.len() <= super::tier_policy::STATIC_ROWS.lane_body.max_inlined_instructions && lowerable(&code)
}

/// Whether every instruction of an inlined callee renames, and one of them completes it (a
/// multi-instruction completion or a returning call).
fn lowerable(code: &[WideInstruction]) -> bool {
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
fn self_tail(function_id: u32, function: &Function, plans: &dyn Fn(u32, u32) -> Option<CallPlan>,
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
        tail.push(super::meaning_compiled::move_record(temp(index)?, window + index));
    }
    for index in 0..argc {
        tail.push(super::meaning_compiled::move_record(params[usize::from(index)], temp(index)?));
    }
    tail.extend(params[usize::from(argc)..].iter().map(|&param| load_undefined(param)));
    tail.extend((0..function.registers).filter(|register| !params.contains(register)).map(|register| load_undefined(register)));
    tail.push(super::meaning_compiled::jump_record(SELF_TAIL_HEADER));
    Some(tail)
}

/// The unit's instructions, origins and frames as a plan lays them out.
struct Emitter {
    unit: Unit,
}

impl Emitter {
    /// Emits `plan` at the unit's end. The caller's (`frame` `None`) keeps its registers and
    /// its own `Return`s; an inlined callee's are renamed to its `base`, its `this` is
    /// `receiver`, and its completions move to `result` and jump to `continuation`.
    #[allow(clippy::too_many_arguments)]
    fn emit(
        &mut self,
        plan: &Plan,
        frame: Option<usize>,
        base: Register,
        receiver: Register,
        result: Register,
        continuation: u32,
    ) -> Option<()> {
        // Unit PCs of each instruction of the function, and past its last.
        let root = frame.is_none();
        if root && plan.tails.iter().any(Option::is_some) {
            self.unit.instructions.push(super::meaning_compiled::jump_record(SELF_TAIL_HEADER));
            self.unit.origins.push(Origin::Caller(0));
        }
        let mut flat = Vec::with_capacity(plan.code.len() + 1);
        let mut at = self.unit.instructions.len() as u32;
        for length in plan.lengths(root) {
            flat.push(at);
            at += length;
        }
        flat.push(at);
        let target_of = |target: u32| flat.get(target as usize).copied();
        let register = |register: Register| base + register;
        for (pc, (instruction, calls)) in plan.code.iter().copied().zip(&plan.calls).enumerate() {
            let origin = match frame {
                None => Origin::Caller(pc as u32),
                Some(frame) => Origin::Inlined { frame, pc: pc as u32 },
            };
            if !calls.is_empty() {
                // A returning call's callee completes what makes the call: an inlined callee
                // completes the call that inlined it, so its result and continuation pass
                // through; the caller uses its original final Return so SourcePc recovery
                // can never execute an already-completed Call again.
                let returns = instruction.returns_from_frame();
                let (call_result, call_continuation) = match (returns, root) {
                    (true, false) => (result, continuation),
                    (true, true) => {
                        let terminal = terminal_return(&plan.code)?;
                        (register(plan.code[terminal].register_a()), flat[terminal])
                    }
                    (false, _) => (register(instruction.result_register()), flat[pc + 1]),
                };
                let keeps = plan.keeps_call(pc, root);
                for (alternative, call) in calls.iter().enumerate() {
                    let index = self.unit.frames.len();
                    let marker = self.unit.instructions.len();
                    self.unit.markers.push(marker);
                    let callee_base = Register::try_from(call.base).ok()?;
                    let next = (alternative + 1 < calls.len() + usize::from(keeps)).then(|| marker + 1 + call.plan.length(false) as usize);
                    let frame_record = InlinedCall {
                        recovery: call.recovery,
                        parent: frame,
                        call: pc as u32,
                        target: call.target,
                        callee_register: register(instruction.register_b()),
                        callee_entry_word: None,
                        receiver: register(instruction.register_c()),
                        window: register(ImmediateLayout::call_window_base(instruction.imm())),
                        arguments: ImmediateLayout::argument_count(instruction.imm()), result: (!returns).then_some(call_result),
                        base: callee_base,
                        registers: call.registers,
                        alternative: next,
                        function_call: plan.through_call[pc].map(|(native, through)| (register(native), through)),
                    };
                    self.unit.frames.push(frame_record);
                    // The marker: the call's guards and bindings (`Lowering::inline_call`); a
                    // callee other than this frame's goes to the next alternative, and past the
                    // last leaves at the call itself, before any of its effects.
                    self.unit.instructions.push(super::meaning_compiled::nop_record());
                    self.unit.origins.push(origin);
                    self.emit(&call.plan, Some(index), callee_base, frame_record.receiver, call_result, call_continuation)?;
                }
                self.unit.instructions.extend(keeps.then_some(instruction));
                self.unit.origins.extend(keeps.then_some(origin));
                continue;
            }
            if root {
                if let Some(tail) = plan.tail(pc) {
                    self.unit.instructions.extend_from_slice(tail);
                    self.unit.origins.extend(std::iter::repeat(origin).take(tail.len()));
                    continue;
                }
                // A caller instruction keeps its registers; a jump goes to its target's unit PC.
                let retargeted = if instruction.op().immediate_role() == quench_runtime::bytecode::ImmediateRole::JumpTarget {
                    WideInstruction::new(instruction.op(), instruction.a(), instruction.b(), instruction.c(), target_of(instruction.imm())?)
                } else {
                    instruction
                };
                self.unit.instructions.push(retargeted);
                self.unit.origins.push(origin);
                continue;
            }
            self.emit_inlined(instruction, origin, &register, receiver, result, continuation, &target_of)?;
        }
        Some(())
    }

    /// An inlined callee's instruction as its caller's unit holds it: registers renamed to the
    /// callee's `base`, its `this` read from `receiver`, and its completions moved to `result`
    /// and jumped to `continuation`.
    #[allow(clippy::too_many_arguments)]
    fn emit_inlined(&mut self, instruction: WideInstruction, origin: Origin, register: &dyn Fn(Register) -> Register,
        receiver: Register, result: Register, continuation: u32, target_of: &dyn Fn(u32) -> Option<u32>) -> Option<()> {
        if inlined_returning(instruction) {
            return self.emit_returning(instruction, origin, register, receiver, result, continuation, target_of);
        }
        let rewritten = match () {
            _ if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Return { .. })) => {
                self.unit.instructions.push(super::meaning_compiled::move_record(result, register(instruction.register_a())));
                self.unit.origins.push(origin);
                super::meaning_compiled::jump_record(continuation)
            }
            _ if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::LoadThis { .. })) => super::meaning_compiled::move_record(register(instruction.result_register()), receiver),
            // Exit-only operations (`inlinable`): their registers, renamed for the analyses;
            // the lowering leaves at each, and recovery runs the callee's own instruction.
            _ if Operation::<u64, ()>::is_throw(instruction)
                || matches!(Operation::<u64, ()>::decode_load_name(instruction, ()), Some(Operation::LoadName { receiver: None, .. })) =>
                WideInstruction::new(instruction.op(), register(instruction.a()), instruction.b(), instruction.c(), instruction.imm()),
            _ if super::meaning_compiled::is_construct(instruction) => super::meaning_compiled::construct_record(register(instruction.a()), register(instruction.b()),
                register(instruction.c()), instruction.imm()),
            _ if site_field_read(instruction) => {
                let flags = instruction.result_flags();
                super::meaning_compiled::get_field_record(flags | register(instruction.result_register()), instruction.b(), instruction.c(), instruction.imm())
            }
            _ => renamed(instruction, &register, receiver, &target_of)?,
        };
        self.unit.instructions.push(rewritten);
        self.unit.origins.push(origin);
        Some(())
    }

    /// A callee's instruction that computes the returned value, then moves it to `result` and
    /// jumps to `continuation` (`inlined_returning`).
    #[allow(clippy::too_many_arguments)]
    fn emit_returning(&mut self, instruction: WideInstruction, origin: Origin, register: &dyn Fn(Register) -> Register,
        receiver: Register, result: Register, continuation: u32, target_of: &dyn Fn(u32) -> Option<u32>) -> Option<()> {
        let computed = if site_field_read(instruction) {
            // A nested lookup keeps its site, whose base the lowering renames (`site_field_read`).
            WideInstruction::new(instruction.op(), register(instruction.result_register()), instruction.b(), instruction.c(), instruction.imm())
        } else {
            let plain = WideInstruction::new(instruction.op(), instruction.result_register(), instruction.b(), instruction.c(), instruction.imm());
            renamed(plain, &register, receiver, &target_of)?
        };
        self.unit.instructions.push(computed);
        self.unit.instructions.push(super::meaning_compiled::move_record(result, register(instruction.result_register())));
        self.unit.instructions.push(super::meaning_compiled::jump_record(continuation));
        self.unit.origins.extend([origin; 3]);
        Some(())
    }
}

/// `function` (`function_id`) with each call `targets` names (by function and PC), whose
/// callee `body` gives and qualifies, spliced in transitively under the default inlining
/// budget. A unit with no inlined call is the function itself.
pub(crate) fn unit<'f>(
    program: &quench_runtime::ResidualProgram,
    function_id: u32,
    function: &Function,
    targets: &dyn Fn(u32, u32) -> Vec<CallTarget>,
    body: &dyn Fn(u32) -> Option<&'f Function>,
) -> Option<Unit> {
    with_budget(program, function_id, function, MAX_UNIT_INLINED_INSTRUCTIONS, targets, body)
}

/// Plan the same unit under a smaller inlining budget after code-size rejection.
pub(crate) fn with_budget<'f>(
    program: &quench_runtime::ResidualProgram,
    function_id: u32,
    function: &Function,
    budget: usize,
    targets: &dyn Fn(u32, u32) -> Vec<CallTarget>,
    body: &dyn Fn(u32) -> Option<&'f Function>,
) -> Option<Unit> {
    with_trips(program, function_id, function, (budget, super::tier_policy::STATIC_ROWS.lane_body.max_inlined_instructions), targets, &|_, _| None, body)
}

/// `with_budget` weighing each call site by the loop trip counts `trips` gives (by function
/// and loop header; `None` where the loop has not run), which `weights` spends the budget by.
#[allow(clippy::too_many_arguments)]
pub(crate) fn with_trips<'f>(
    program: &quench_runtime::ResidualProgram,
    function_id: u32,
    function: &Function,
    (budget, max_inlined): (usize, usize),
    targets: &dyn Fn(u32, u32) -> Vec<CallTarget>,
    trips: &dyn Fn(u32, u32) -> Option<u64>,
    body: &dyn Fn(u32) -> Option<&'f Function>,
) -> Option<Unit> {
    with_plans(program, function_id, function, (budget, max_inlined), targets, &|_, _| None, trips, body)
}

/// `with_trips` where each call site's plan (`plans`, by function and PC) decides its inlining: a site a plan covers
/// inlines the targets it names (`Planner::inline`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn with_plans<'f>(
    program: &quench_runtime::ResidualProgram,
    function_id: u32,
    function: &Function,
    (budget, max_inlined): (usize, usize),
    targets: &dyn Fn(u32, u32) -> Vec<CallTarget>,
    plans: &dyn Fn(u32, u32) -> Option<CallPlan>,
    trips: &dyn Fn(u32, u32) -> Option<u64>,
    body: &dyn Fn(u32) -> Option<&'f Function>,
) -> Option<Unit> {
    let caller_registers = function.registers;
    let mut planner = Planner { program, targets: &|function, pc| targets(function, pc).into_iter().filter(|target| target.function != function_id).collect(), plans, body, registers: u32::from(caller_registers), budget, max_inlined, trips, weight: 1, path: Vec::new(), choice: weights::Choice::default() };
    let mut plan = planner.plan_weighted(function_id, function, budget);
    // The self tail calls of the root become unit instructions (`self_tail`); their argument temporaries sit above
    // the planned registers, and the undefined value is the program's `undefined` constant, absent from a program that has none.
    let undefined = program.constants.iter().position(|constant| matches!(constant, Constant::Undefined)).and_then(|index| u32::try_from(index).ok());
    let tails: Vec<Option<Vec<WideInstruction>>> = match undefined {
        Some(undefined) => (0..plan.code.len()).map(|pc| plan.calls[pc].is_empty().then(|| self_tail(function_id, function, plans,
            &plan.code, pc, planner.registers, undefined)).flatten()).collect(),
        None => Vec::new(),
    };
    let temporaries = if tails.iter().any(Option::is_some) { u32::from(function.params) } else { 0 };
    plan.tails = tails;
    let registers = u16::try_from(planner.registers + temporaries).ok()?;
    let mut emitter = Emitter {
        unit: Unit {
            instructions: Vec::with_capacity(plan.length(true) as usize),
            origins: Vec::with_capacity(plan.length(true) as usize),
            frames: Vec::new(),
            markers: Vec::new(),
            caller_registers,
            registers,
            end: plan.code.len() as u32,
        },
    };
    emitter.emit(&plan, None, 0, 0, 0, 0)?;
    Some(emitter.unit)
}

#[cfg(test)]
#[path = "lane_unit/tests.rs"]
pub(crate) mod tests;

/// Whether an inlined callee's instruction computes its result and returns it in one
/// (`RETURN_REGISTER` alone, of a value-producing instruction), which inlining splits into
/// the computation and the `Return` it makes.
pub(crate) fn inlined_returning(instruction: WideInstruction) -> bool {
    instruction.result_flags() == quench_runtime::bytecode::RETURN_REGISTER
        && !quench_runtime::staged::returns(instruction.op())
        && instruction.op().field_layout(InstructionField::A) == FieldLayout::ResultRegister
        && instruction.defined_register().is_some()
}


/// `instruction` with each register `register` renames, a `this` field base made the
/// `this` register, and a jump target `target` renames; `None` for a field it cannot
/// rename, or a register with flags.
pub(crate) fn renamed(
    instruction: WideInstruction,
    register: &dyn Fn(Register) -> Register,
    this: Register,
    target: &dyn Fn(u32) -> Option<u32>,
) -> Option<WideInstruction> {
    let op = instruction.op();
    let rename = |field: InstructionField, raw: u16| -> Option<u16> {
        Some(match op.field_layout(field) {
            FieldLayout::Unused
            | FieldLayout::BooleanFlag
            | FieldLayout::BinaryOperator
            | FieldLayout::CacheSiteIndex
            | FieldLayout::FieldLookupCacheSiteIndex => raw,
            FieldLayout::ResultRegister
            | FieldLayout::Register
            | FieldLayout::WriteRegister
            | FieldLayout::ReadWriteRegister => {
                (raw <= Operand::PAYLOAD_MASK).then_some(())?;
                register(raw)
            }
            FieldLayout::Operand | FieldLayout::NumericIndexOperand => {
                let operand = Operand(raw);
                match operand.kind()? {
                    OperandKind::Register => Operand::register(register(operand.payload())).0,
                    OperandKind::Constant => raw,
                    OperandKind::Field | OperandKind::Local => return None,
                }
            }
            FieldLayout::FieldBase => {
                let base = FieldBase(raw);
                if base == FieldBase::THIS {
                    FieldBase::register(this).0
                } else {
                    FieldBase::register(register(base.register_index()?)).0
                }
            }
            _ => return None,
        })
    };
    let a = rename(InstructionField::A, instruction.a())?;
    let b = rename(InstructionField::B, instruction.b())?;
    let c = rename(InstructionField::C, instruction.c())?;
    let imm = match op.immediate_role() {
        ImmediateRole::JumpTarget => target(instruction.imm())?,
        // A call's argument window renames as one block: each of its registers to the one
        // after the base's rename.
        _ if quench_runtime::staged::calls(op) => {
            let imm = instruction.imm();
            let base = ImmediateLayout::call_window_base(imm);
            let count = ImmediateLayout::argument_count(imm);
            let renamed_base = register(base);
            (0..count).all(|offset| register(base + offset) == renamed_base + offset).then_some(())?;
            ImmediateLayout::call_immediate(renamed_base, count, ImmediateLayout::direct_eval(imm), ImmediateLayout::parameter_eval(imm))
        }
        _ => instruction.imm(),
    };
    Some(WideInstruction::new(op, a, b, c, imm))
}

/// The function's full decoded code as a plain lane unit, whose PCs are the function's.
pub(crate) fn whole_function(function: &Function) -> Unit {
    let instructions: Vec<_> = function.instructions().collect();
    let count = instructions.len();
    Unit {
        instructions,
        origins: (0..count).map(|pc| Origin::Caller(pc as u32)).collect(),
        frames: Vec::new(), markers: Vec::new(),
        caller_registers: function.registers, registers: function.registers,
        end: count as u32,
    }
}
