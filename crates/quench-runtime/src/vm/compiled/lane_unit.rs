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
/// graph, inlined, is the region an optimizing tier compiles as one function.
const MAX_UNIT_INLINED_INSTRUCTIONS: usize = 768;
/// How deep calls nest inside a unit.
const MAX_INLINING_DEPTH: usize = 4;

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
        Op::GetField => plain && matches!(instruction.field_lookup(), Some(FieldLookup::Atom { .. })),
        Op::SetThisField | Op::SetThisFieldStrict => plain,
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
fn call_length(instruction: WideInstruction, root: bool, callee: &Plan) -> u32 {
    1 + callee.length(false) + u32::from(root && instruction.returns_from_frame())
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
    calls: Vec<Option<Box<InlinedPlan>>>,
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
            .map(|(instruction, call)| match call {
                Some(call) => call_length(*instruction, root, &call.plan),
                None if root => 1,
                None => completion_length(*instruction),
            })
            .sum()
    }
}

struct Planner<'t, 'f> {
    targets: &'t dyn Fn(u32, u32) -> Option<CallTarget>,
    body: &'t dyn Fn(u32) -> Option<&'f Function>,
    registers: u32,
    budget: usize,
}

impl Planner<'_, '_> {
    /// The plan of `function`'s `code`, its calls inlined where the budget allows. A callee
    /// is inlined only whole: every call of its own inlined too.
    fn plan(&mut self, function: u32, code: Vec<WideInstruction>, depth: usize) -> Plan {
        let calls = code
            .iter()
            .enumerate()
            .map(|(pc, instruction)| self.inline(function, pc as u32, *instruction, depth).map(Box::new))
            .collect();
        Plan { code, calls }
    }

    fn inline(&mut self, function: u32, pc: u32, instruction: WideInstruction, depth: usize) -> Option<InlinedPlan> {
        if instruction.op() != Op::Call || !returns_or_keeps(instruction) || instruction.direct_eval()
            || instruction.parameter_eval() || depth >= MAX_INLINING_DEPTH
        {
            return None;
        }
        let target = (self.targets)(function, pc)?;
        let callee = (self.body)(target.function)?;
        let code = decoded(callee);
        if code.len() > MAX_INLINED_INSTRUCTIONS
            || code.len() > self.budget
            || ImmediateLayout::argument_count(instruction.imm()) > callee.registers
            || !code.iter().all(|instruction| inlinable(*instruction))
            || !code.iter().any(|instruction| completion_length(*instruction) > 1)
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
        let whole = plan.code.iter().zip(&plan.calls).all(|(instruction, call)| instruction.op() != Op::Call || call.is_some());
        if !whole {
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
        for (instruction, call) in plan.code.iter().zip(&plan.calls) {
            flat.push(at);
            at += match call {
                Some(call) => call_length(*instruction, root, &call.plan),
                None if root => 1,
                None => completion_length(*instruction),
            };
        }
        flat.push(at);
        let target_of = |target: u32| flat.get(target as usize).copied();
        let register = |register: Register| base + register;
        for (pc, (instruction, call)) in plan.code.iter().copied().zip(&plan.calls).enumerate() {
            let origin = match frame {
                None => Origin::Caller(pc as u32),
                Some(frame) => Origin::Inlined { frame, pc: pc as u32 },
            };
            if let Some(call) = call {
                let index = self.unit.frames.len();
                self.unit.markers.push(self.unit.instructions.len());
                let callee_base = Register::try_from(call.base).ok()?;
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
                };
                self.unit.frames.push(frame_record);
                // The marker: the call's guards and bindings (`Lowering::inline_call`); a guard
                // that fails leaves at the call itself, before any of its effects.
                self.unit.instructions.push(WideInstruction::new(Op::Nop, 0, 0, 0, 0));
                self.unit.origins.push(origin);
                // A returning call's callee completes what makes the call: an inlined callee
                // completes the call that inlined it, so its result and continuation pass
                // through; the caller completes with a `Return` of the call's register.
                let returns = instruction.returns_from_frame();
                let (call_result, call_continuation) = match (returns, root) {
                    (true, false) => (result, continuation),
                    (true, true) => (register(instruction.result_register()), flat[pc + 1] - 1),
                    (false, _) => (register(instruction.result_register()), flat[pc + 1]),
                };
                self.emit(&call.plan, Some(index), callee_base, frame_record.receiver, call_result, call_continuation)?;
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
                let plain = WideInstruction::new(instruction.op(), instruction.result_register(), instruction.b(), instruction.c(), instruction.imm());
                self.unit.instructions.push(super::osr_loop::renamed(plain, &register, receiver, &target_of)?);
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
                _ => super::osr_loop::renamed(instruction, &register, receiver, &target_of)?,
            };
            self.unit.instructions.push(rewritten);
            self.unit.origins.push(origin);
        }
        Some(())
    }
}

/// `function` (`function_id`) with each call `targets` names (by function and PC), whose
/// callee `body` gives and qualifies, spliced in, transitively. A unit with no inlined call
/// is the function itself.
pub(crate) fn unit<'f>(
    function_id: u32,
    function: &Function,
    targets: &dyn Fn(u32, u32) -> Option<CallTarget>,
    body: &dyn Fn(u32) -> Option<&'f Function>,
) -> Option<Unit> {
    let caller_registers = function.registers;
    let mut planner = Planner { targets, body, registers: u32::from(caller_registers), budget: MAX_UNIT_INLINED_INSTRUCTIONS };
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
