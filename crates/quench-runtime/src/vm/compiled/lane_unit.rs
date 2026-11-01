//! The unit a lane body specializes: one function with the callees its call sites' inline
//! caches name spliced in (R3, docs/v2/specialization.md).
//!
//! A call whose record links one script target (`LaneInstruction::call_link`), to a callee
//! whose body the lane-body lowering runs entirely and that calls nothing, is replaced by a
//! call marker and the callee's renamed instructions. Its registers follow the caller's;
//! its `this` is the call's receiver register; its `Return` moves the value to the call's
//! result and jumps past the call. Each PC of the unit names its origin, so a leave inside
//! a callee makes the callee's activation through the lane's own call path and continues
//! at the callee's record (`Vm::lane_body_inlined_activation`).

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

/// An inlined call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Frame {
    /// The caller's PC of the call.
    pub(crate) call: u32,
    pub(crate) target: CallTarget,
    /// The caller register holding the callee, guarded at the call.
    pub(crate) callee_register: Register,
    /// The caller register holding the call's receiver, the callee's `this`.
    pub(crate) receiver: Register,
    /// The caller's first argument register.
    pub(crate) window: Register,
    pub(crate) arguments: u16,
    /// The callee's register 0 in the unit.
    pub(crate) base: Register,
    pub(crate) registers: u16,
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

/// The longest callee body the unit inlines: each copy of it costs code and compile time at
/// every call site, and short bodies (accessors, field updates) are where a call costs most
/// relative to its work.
const MAX_INLINED_INSTRUCTIONS: usize = 48;

/// Whether the lane-body lowering runs `instruction` of an inlined callee: no call (one level),
/// no capture (its environment is the callee's), no returning fusion.
fn inlinable(instruction: WideInstruction) -> bool {
    let plain = instruction.result_flags() == 0;
    let value = plain || super::osr_loop::inlined_returning(instruction);
    match instruction.op() {
        Op::Return => plain,
        Op::Nop | Op::LoadConst | Op::Move | Op::LoadThis | Op::IncDec | Op::Jump | Op::JumpFalse | Op::JumpTrue
        | Op::JumpBinaryFalse => plain,
        Op::GetField => value && matches!(instruction.field_lookup(), Some(FieldLookup::Atom { .. })),
        Op::SetThisField | Op::SetThisFieldStrict => plain,
        op => value && op.js_denotation() == crate::bytecode::JsDenotation::NumericBinary,
    }
}

fn expanded_len(instruction: WideInstruction) -> usize {
    if instruction.op() == Op::Return { 2 }
    else if super::osr_loop::inlined_returning(instruction) { 3 }
    else { 1 }
}

fn decoded(function: &Function) -> Vec<WideInstruction> {
    function.code.iter().map(|packed| super::super::dispatch_fast::decoded_instruction(function, *packed)).collect()
}

/// `function` with each call `targets` names, whose callee `body` gives and qualifies, spliced
/// in. A unit with no inlined call is the function itself.
pub(crate) fn unit<'f>(
    function: &Function,
    targets: &[Option<CallTarget>],
    body: &dyn Fn(u32) -> Option<&'f Function>,
) -> Option<Unit> {
    let caller = decoded(function);
    let caller_registers = function.registers;
    // Which calls inline, and where each callee's registers start.
    let mut inlined: Vec<Option<(Frame, Vec<WideInstruction>)>> = vec![None; caller.len()];
    let mut registers = u32::from(caller_registers);
    for (pc, instruction) in caller.iter().copied().enumerate() {
        let Some(target) = targets.get(pc).copied().flatten() else { continue };
        if instruction.op() != Op::Call || instruction.result_flags() != 0 || instruction.direct_eval() || instruction.parameter_eval() {
            continue;
        }
        let Some(callee) = body(target.function) else { continue };
        let callee_code = decoded(callee);
        let arguments = ImmediateLayout::argument_count(instruction.imm());
        if callee_code.len() > MAX_INLINED_INSTRUCTIONS
            || arguments > callee.registers
            || !callee_code.iter().all(|instruction| inlinable(*instruction))
            || !callee_code.iter().any(|instruction| instruction.op() == Op::Return)
        {
            continue;
        }
        let Ok(base) = Register::try_from(registers) else { break };
        registers += u32::from(callee.registers);
        let frame = Frame {
            call: pc as u32,
            target,
            callee_register: instruction.register_b(),
            receiver: instruction.register_c(),
            window: ImmediateLayout::call_window_base(instruction.imm()),
            arguments,
            base,
            registers: callee.registers,
        };
        inlined[pc] = Some((frame, callee_code));
    }
    let registers = u16::try_from(registers).ok()?;
    // Unit PCs: a caller instruction takes one, an inlined call its marker, each callee
    // instruction one and each callee `Return` two (the result's move, the jump past the call).
    let mut caller_flat = Vec::with_capacity(caller.len() + 1);
    let mut length = 0usize;
    for site in &inlined {
        caller_flat.push(length as u32);
        length += match site {
            Some((_, code)) => 1 + code.iter().copied().map(expanded_len).sum::<usize>(),
            None => 1,
        };
    }
    caller_flat.push(length as u32);
    let caller_target = |target: u32| caller_flat.get(target as usize).copied();
    let mut unit = Unit {
        instructions: Vec::with_capacity(length),
        origins: Vec::with_capacity(length),
        frames: Vec::new(),
        markers: Vec::new(),
        caller_registers,
        registers,
        end: caller.len() as u32,
    };
    for (pc, instruction) in caller.iter().copied().enumerate() {
        let Some((frame, code)) = &inlined[pc] else {
            // A caller instruction keeps its registers; a jump goes to its target's unit PC.
            let retargeted = if instruction.op().immediate_role() == crate::bytecode::ImmediateRole::JumpTarget {
                WideInstruction::new(instruction.op(), instruction.a(), instruction.b(), instruction.c(), caller_target(instruction.imm())?)
            } else {
                instruction
            };
            unit.instructions.push(retargeted);
            unit.origins.push(Origin::Caller(pc as u32));
            continue;
        };
        let index = unit.frames.len();
        unit.markers.push(unit.instructions.len());
        unit.frames.push(*frame);
        // The marker: the call's guards and bindings (`Lowering::inline_call`); a guard that
        // fails leaves at the call itself, before any of its effects.
        unit.instructions.push(WideInstruction::new(Op::Nop, 0, 0, 0, 0));
        unit.origins.push(Origin::Caller(pc as u32));
        let continuation = caller_flat[pc + 1];
        let mut callee_flat = Vec::with_capacity(code.len());
        let mut at = unit.instructions.len() as u32;
        for instruction in code {
            callee_flat.push(at);
            at += expanded_len(*instruction) as u32;
        }
        let register = |register: Register| frame.base + register;
        let callee_target = |target: u32| callee_flat.get(target as usize).copied();
        for (callee_pc, instruction) in code.iter().copied().enumerate() {
            let origin = Origin::Inlined { frame: index, pc: callee_pc as u32 };
            if super::osr_loop::inlined_returning(instruction) {
                let plain = WideInstruction::new(instruction.op(), instruction.result_register(), instruction.b(), instruction.c(), instruction.imm());
                unit.instructions.push(super::osr_loop::renamed(plain, &register, frame.receiver, &callee_target)?);
                unit.instructions.push(WideInstruction::new(Op::Move, caller[pc].result_register(), register(instruction.result_register()), 0, 0));
                unit.instructions.push(WideInstruction::new(Op::Jump, 0, 0, 0, continuation));
                unit.origins.extend([origin, origin, origin]);
                continue;
            }
            let rewritten = match instruction.op() {
                Op::Return => {
                    let result = caller[pc].result_register();
                    unit.instructions.push(WideInstruction::new(Op::Move, result, register(instruction.register_a()), 0, 0));
                    unit.origins.push(origin);
                    WideInstruction::new(Op::Jump, 0, 0, 0, continuation)
                }
                Op::LoadThis => WideInstruction::new(Op::Move, register(instruction.result_register()), frame.receiver, 0, 0),
                _ => super::osr_loop::renamed(instruction, &register, frame.receiver, &callee_target)?,
            };
            unit.instructions.push(rewritten);
            unit.origins.push(origin);
        }
    }
    Some(unit)
}
