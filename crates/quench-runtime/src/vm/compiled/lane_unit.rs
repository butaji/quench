//! The unit a lane body specializes: one function with the callees its call sites' inline
//! caches name spliced in, transitively, under one budget (R3, docs/v2/specialization.md).
//!
//! A call whose record links one script target (`LaneInstruction::call_link`), to a callee
//! whose body the lane-body lowering runs entirely and every call of which is spliced in in
//! turn, is replaced by a call marker and the callee's renamed instructions. Its registers
//! follow the unit's; its `this` is the call's receiver register; its `Return` moves the
//! value to the call's result and jumps past the call. Each PC of the unit names its origin,
//! so a leave inside a callee makes the activations of its frame and the frames around it,
//! outermost first, through the lane's own call path, and continues at the innermost one's
//! record (`Vm::lane_body_inlined_activation`).

use crate::bytecode::{FieldLookup, ImmediateLayout, Op, Register, WideInstruction};
use crate::bytecode::Function;
use crate::value::Value;

/// The script target a call record's inline cache links, as the unit inlines it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CallTarget {
    pub(crate) callee: Value,
    pub(crate) function: u32,
}

/// Where a unit PC comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    /// The caller's instruction at this PC.
    Caller(u32),
    /// The instruction at `pc` of inlined `frame`.
    Inlined { frame: usize, pc: u32 },
}

/// An inlined call. Its registers are the unit's: the caller's are its first, and each
/// callee's start at its `base`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Frame {
    /// The frame whose function makes the call; `None` for the caller's.
    pub(crate) parent: Option<usize>,
    /// The PC of the call in its parent's function.
    pub(crate) call: u32,
    pub(crate) target: CallTarget,
    /// The register holding the callee, guarded at the call.
    pub(crate) callee_register: Register,
    /// The register holding the call's receiver, the callee's `this`.
    pub(crate) receiver: Register,
    /// The call's first argument register.
    pub(crate) window: Register,
    pub(crate) arguments: u16,
    /// The callee's register 0.
    pub(crate) base: Register,
    pub(crate) registers: u16,
    /// The unit PC of the call's next alternative's marker, where a callee other than this
    /// frame's goes; `None` for the last, which leaves at the call.
    pub(crate) alternative: Option<usize>,
}

impl Frame {
    /// The parent's register 0: the caller's, or the enclosing callee's.
    pub(crate) fn parent_base(&self, frames: &[Frame]) -> Register {
        self.parent.map_or(0, |parent| frames[parent].base)
    }
}

pub(crate) struct Unit {
    pub(crate) instructions: Vec<WideInstruction>,
    pub(crate) origins: Vec<Origin>,
    pub(crate) frames: Vec<Frame>,
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
}

/// The longest callee body the unit inlines: each copy of it costs code and compile time at
/// every call site, and short bodies (accessors, field updates) are where a call costs most
/// relative to its work.
const MAX_INLINED_INSTRUCTIONS: usize = 96;
/// The callee instructions one unit inlines in all, the region's budget: a hot loop's call
/// graph, inlined, is the region an optimizing tier compiles as one function. Its compile
/// cost grows with it (Richards' 600-instruction units cost ~63M instructions to compile),
/// so it is TurboFan's cumulative inlining budget (`max_inlined_bytecode_size_cumulative`).
pub(super) const MAX_UNIT_INLINED_INSTRUCTIONS: usize = 920;
/// How deep calls nest inside a unit: JSC's inlining depth (`maximumInliningDepth`).
const MAX_INLINING_DEPTH: usize = 5;

/// Whether the lane-body lowering runs `instruction` of an inlined callee: a capture of its
/// own environment only, no fusion but a returning one, which splits into the
/// computation and the return (`osr_loop::inlined_returning`). A call is the plan's to admit:
/// only one it inlines in turn.
fn inlinable(instruction: WideInstruction) -> bool {
    let plain = instruction.result_flags() == 0 || super::osr_loop::inlined_returning(instruction);
    match instruction.op() {
        Op::Return => plain,
        Op::Nop | Op::LoadConst | Op::Move | Op::LoadThis | Op::IncDec | Op::Jump | Op::JumpFalse | Op::JumpTrue
        | Op::JumpBinaryFalse => plain,
        // The callee's own captures, which its frame reads from its environment
        // (`Lowering::frame_captures`).
        Op::LoadCapture => plain && ImmediateLayout::capture_depth(instruction.imm()) == 0,
        // A nested lookup's chain names its base by the function's registers; the lowering
        // renames it through the frame (`Lowering::get_field`), and its sink stores to `this`.
        Op::GetField => match instruction.field_lookup() {
            Some(FieldLookup::Atom { .. }) => plain,
            Some(FieldLookup::Site(_)) => plain || instruction.result_flags() == crate::bytecode::SET_THIS_REGISTER,
            None => false,
        },
        Op::SetThisField | Op::SetThisFieldStrict | Op::SetField | Op::SetFieldStrict | Op::GetIndex | Op::SetIndex => plain,
        Op::Call => returns_or_keeps(instruction) && !instruction.direct_eval() && !instruction.parameter_eval(),
        op => plain && op.js_denotation() == crate::bytecode::JsDenotation::NumericBinary,
    }
}

/// Whether a call keeps its result in its register or returns it: the two a unit inlines.
fn returns_or_keeps(instruction: WideInstruction) -> bool {
    matches!(instruction.result_flags(), 0 | crate::bytecode::RETURN_REGISTER)
}

/// The unit PCs an inlined call takes past its marker and its callee's: in the caller, a
/// returning call's completion is a `Return` of its register; an inlined callee's returning
/// call completes the callee itself.
fn call_length(instruction: WideInstruction, root: bool, alternatives: &[InlinedPlan]) -> u32 {
    alternatives.iter().map(|callee| 1 + callee.plan.length(false)).sum::<u32>()
        + u32::from(root && instruction.returns_from_frame())
}

/// Whether an inlined callee's `instruction` renames to the unit's registers
/// (`osr_loop::renamed`): its operands are registers or constants. The plan decides it, so a
/// unit it plans always emits.
fn renamable(instruction: WideInstruction) -> bool {
    match instruction.op() {
        Op::Call | Op::Return | Op::LoadThis => true,
        Op::GetField if matches!(instruction.field_lookup(), Some(FieldLookup::Site(_))) => true,
        _ => {
            let plain = if super::osr_loop::inlined_returning(instruction) {
                WideInstruction::new(instruction.op(), instruction.result_register(), instruction.b(), instruction.c(), instruction.imm())
            } else {
                instruction
            };
            super::osr_loop::renamed(plain, &|register| register, 0, &|target| Some(target)).is_some()
        }
    }
}

/// The unit PCs an inlined callee instruction takes: a completion of the call (a `Return`, or
/// an instruction that computes and returns) adds the result's move and the jump past the call.
fn completion_length(instruction: WideInstruction) -> u32 {
    if instruction.op() == Op::Return {
        2
    } else if super::osr_loop::inlined_returning(instruction) {
        3
    } else {
        1
    }
}

fn decoded(function: &Function) -> Vec<WideInstruction> {
    function.code.iter().map(|packed| super::super::dispatch_fast::decoded_instruction(function, *packed)).collect()
}

/// A function's code as the unit splices it: per PC, the call it inlines there, if any.
struct Plan {
    code: Vec<WideInstruction>,
    /// Per PC, the callees its call inlines, as a switch on the callee; empty for none.
    calls: Vec<Vec<InlinedPlan>>,
    /// Whether a call the plan does not inline has run (its site linked a target): an
    /// inlined callee's never-run call is an exit of the unit, one that runs is not.
    blocked: bool,
}

struct InlinedPlan {
    target: CallTarget,
    /// The callee's register 0 in the unit.
    base: u32,
    registers: u16,
    plan: Plan,
}

impl Plan {
    /// The unit PCs the function takes where `root` keeps its own instructions; an inlined
    /// callee's completions add their moves and jumps.
    fn length(&self, root: bool) -> u32 {
        self.code
            .iter()
            .zip(&self.calls)
            .map(|(instruction, calls)| match calls.as_slice() {
                [] if root => 1,
                [] => completion_length(*instruction),
                alternatives => call_length(*instruction, root, alternatives),
            })
            .sum()
    }
}

struct Planner<'t, 'f> {
    targets: &'t dyn Fn(u32, u32) -> Vec<CallTarget>,
    body: &'t dyn Fn(u32) -> Option<&'f Function>,
    registers: u32,
    budget: usize,
}

impl Planner<'_, '_> {
    /// The plan of `function`'s `code`, its calls inlined where the budget allows. A callee
    /// is inlined only whole: every call of its own inlined too.
    fn plan(&mut self, function: u32, code: Vec<WideInstruction>, depth: usize) -> Plan {
        let mut blocked = false;
        let calls = code
            .iter()
            .enumerate()
            .map(|(pc, instruction)| {
                let (calls, ran) = self.inline(function, pc as u32, *instruction, depth);
                blocked |= calls.is_empty() && ran;
                calls
            })
            .collect();
        Plan { code, calls, blocked }
    }

    /// The callees the call at `pc` of `function` inlines: every target its site linked, or
    /// none, so a callee the switch names never leaves it for a call.
    /// With whether the call has run: a call the plan may not inline counts as run.
    fn inline(&mut self, function: u32, pc: u32, instruction: WideInstruction, depth: usize) -> (Vec<InlinedPlan>, bool) {
        if instruction.op() != Op::Call {
            return (Vec::new(), false);
        }
        if !returns_or_keeps(instruction) || instruction.direct_eval()
            || instruction.parameter_eval() || depth >= MAX_INLINING_DEPTH
        {
            return (Vec::new(), true);
        }
        let (registers, budget) = (self.registers, self.budget);
        let found = (self.targets)(function, pc);
        let ran = !found.is_empty();
        let alternatives: Option<Vec<InlinedPlan>> = found
            .into_iter()
            .map(|target| self.inline_target(target, instruction, depth))
            .collect();
        match alternatives {
            Some(alternatives) if !alternatives.is_empty() => (alternatives, true),
            _ => {
                (self.registers, self.budget) = (registers, budget);
                (Vec::new(), ran)
            }
        }
    }

    fn inline_target(&mut self, target: CallTarget, instruction: WideInstruction, depth: usize) -> Option<InlinedPlan> {
        let callee = (self.body)(target.function)?;
        let code = decoded(callee);
        if code.len() > MAX_INLINED_INSTRUCTIONS
            || code.len() > self.budget
            || ImmediateLayout::argument_count(instruction.imm()) > callee.registers
            || !code.iter().all(|instruction| inlinable(*instruction) && renamable(*instruction))
            || !code.iter().any(|instruction| completion_length(*instruction) > 1
                || (instruction.op() == Op::Call && instruction.returns_from_frame()))
        {
            return None;
        }
        // The callee's registers and budget are reserved before its own calls plan, and given
        // back if one of them is not inlined.
        let (registers, budget) = (self.registers, self.budget);
        let base = self.registers;
        self.registers += u32::from(callee.registers);
        self.budget -= code.len();
        let plan = self.plan(target.function, code, depth + 1);
        if plan.blocked {
            (self.registers, self.budget) = (registers, budget);
            return None;
        }
        Some(InlinedPlan { target, base, registers: callee.registers, plan })
    }
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
        let mut flat = Vec::with_capacity(plan.code.len() + 1);
        let mut at = self.unit.instructions.len() as u32;
        for (instruction, calls) in plan.code.iter().zip(&plan.calls) {
            flat.push(at);
            at += match calls.as_slice() {
                [] if root => 1,
                [] => completion_length(*instruction),
                alternatives => call_length(*instruction, root, alternatives),
            };
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
                // through; the caller completes with a `Return` of the call's register.
                let returns = instruction.returns_from_frame();
                let (call_result, call_continuation) = match (returns, root) {
                    (true, false) => (result, continuation),
                    (true, true) => (register(instruction.result_register()), flat[pc + 1] - 1),
                    (false, _) => (register(instruction.result_register()), flat[pc + 1]),
                };
                for (alternative, call) in calls.iter().enumerate() {
                    let index = self.unit.frames.len();
                    let marker = self.unit.instructions.len();
                    self.unit.markers.push(marker);
                    let callee_base = Register::try_from(call.base).ok()?;
                    let next = (alternative + 1 < calls.len()).then(|| marker + 1 + call.plan.length(false) as usize);
                    let frame_record = Frame {
                        parent: frame,
                        call: pc as u32,
                        target: call.target,
                        callee_register: register(instruction.register_b()),
                        receiver: register(instruction.register_c()),
                        window: register(ImmediateLayout::call_window_base(instruction.imm())),
                        arguments: ImmediateLayout::argument_count(instruction.imm()),
                        base: callee_base,
                        registers: call.registers,
                        alternative: next,
                    };
                    self.unit.frames.push(frame_record);
                    // The marker: the call's guards and bindings (`Lowering::inline_call`); a
                    // callee other than this frame's goes to the next alternative, and past the
                    // last leaves at the call itself, before any of its effects.
                    self.unit.instructions.push(WideInstruction::new(Op::Nop, 0, 0, 0, 0));
                    self.unit.origins.push(origin);
                    self.emit(&call.plan, Some(index), callee_base, frame_record.receiver, call_result, call_continuation)?;
                }
                if returns && root {
                    self.unit.instructions.push(WideInstruction::new(Op::Return, instruction.result_register(), 0, 0, 0));
                    self.unit.origins.push(origin);
                }
                continue;
            }
            if root {
                // A caller instruction keeps its registers; a jump goes to its target's unit PC.
                let retargeted = if instruction.op().immediate_role() == crate::bytecode::ImmediateRole::JumpTarget {
                    WideInstruction::new(instruction.op(), instruction.a(), instruction.b(), instruction.c(), target_of(instruction.imm())?)
                } else {
                    instruction
                };
                self.unit.instructions.push(retargeted);
                self.unit.origins.push(origin);
                continue;
            }
            if super::osr_loop::inlined_returning(instruction) {
                let computed = if instruction.op() == Op::GetField && instruction.b() == crate::bytecode::FieldBase::NESTED {
                    // A nested lookup keeps its site, whose base the lowering renames.
                    WideInstruction::new(instruction.op(), register(instruction.result_register()), instruction.b(), instruction.c(), instruction.imm())
                } else {
                    let plain = WideInstruction::new(instruction.op(), instruction.result_register(), instruction.b(), instruction.c(), instruction.imm());
                    super::osr_loop::renamed(plain, &register, receiver, &target_of)?
                };
                self.unit.instructions.push(computed);
                self.unit.instructions.push(WideInstruction::new(Op::Move, result, register(instruction.result_register()), 0, 0));
                self.unit.instructions.push(WideInstruction::new(Op::Jump, 0, 0, 0, continuation));
                self.unit.origins.extend([origin; 3]);
                continue;
            }
            let rewritten = match instruction.op() {
                Op::Return => {
                    self.unit.instructions.push(WideInstruction::new(Op::Move, result, register(instruction.register_a()), 0, 0));
                    self.unit.origins.push(origin);
                    WideInstruction::new(Op::Jump, 0, 0, 0, continuation)
                }
                Op::LoadThis => WideInstruction::new(Op::Move, register(instruction.result_register()), receiver, 0, 0),
                Op::GetField if matches!(instruction.field_lookup(), Some(FieldLookup::Site(_))) => {
                    let flags = instruction.result_flags();
                    WideInstruction::new(Op::GetField, flags | register(instruction.result_register()), instruction.b(), instruction.c(), instruction.imm())
                }
                _ => super::osr_loop::renamed(instruction, &register, receiver, &target_of)?,
            };
            self.unit.instructions.push(rewritten);
            self.unit.origins.push(origin);
        }
        Some(())
    }
}

/// `function` (`function_id`) with each call `targets` names (by function and PC), whose
/// callee `body` gives and qualifies, spliced in, transitively, up to `budget` inlined
/// instructions. A unit with no inlined call is the function itself.
pub(crate) fn unit<'f>(
    function_id: u32,
    function: &Function,
    budget: usize,
    targets: &dyn Fn(u32, u32) -> Vec<CallTarget>,
    body: &dyn Fn(u32) -> Option<&'f Function>,
) -> Option<Unit> {
    let caller_registers = function.registers;
    let mut planner = Planner { targets, body, registers: u32::from(caller_registers), budget };
    let plan = planner.plan(function_id, decoded(function), 0);
    let registers = u16::try_from(planner.registers).ok()?;
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
