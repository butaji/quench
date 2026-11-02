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

impl Unit {
    /// Project the original functions' recovery liveness onto unit registers.
    /// A suspended caller retains both its call inputs and continuation state;
    /// callee facts include implicit observations such as legacy f.arguments.
    pub(crate) fn register_masks<'a>(
        &self, program: &'a crate::ResidualProgram, caller: u32,
    ) -> Option<std::borrow::Cow<'a, crate::compile::liveness::RegisterMasks>> {
        use crate::compile::liveness::{ROOT_MAP_REGISTERS, RegisterMasks};
        let caller = program.register_root_masks(caller)?;
        if self.frames.is_empty() && self.origins.iter().enumerate().all(|(pc, origin)|
            *origin == Origin::Caller(pc as u32)) {
            return Some(std::borrow::Cow::Borrowed(caller));
        }
        if self.registers > ROOT_MAP_REGISTERS { return None; }
        let frames = self.frames.iter().map(|frame| {
            Some((caller.get(frame.call as usize)? | caller.get(frame.call as usize + 1)?,
                program.register_root_masks(frame.target.function)?))
        }).collect::<Option<Vec<_>>>()?;
        let mut masks = self.origins.iter().map(|origin| match *origin {
            Origin::Caller(pc) => caller.get(pc as usize),
            Origin::Inlined { frame, pc } => {
                let (suspended, callee) = frames[frame];
                Some(suspended | callee.get(pc as usize)?.checked_shl(u32::from(self.frames[frame].base))?)
            }
        }).collect::<Option<Vec<_>>>()?;
        masks.push(caller.get(self.end as usize)?);
        // Source recovery alone misses uses introduced by the splice (the
        // result move after a returning operation, for example). Feed those
        // instructions through the same transfer/CFG solver as ordinary code.
        let mut transfers: Vec<_> = self.instructions.iter().map(|instruction|
            crate::compile::liveness::transfer(*instruction, program,
                &program.field_sites, &program.superinstructions)).collect();
        for (frame, pc) in self.frames.iter().zip(&self.markers) {
            transfers[*pc].1 = (0..frame.registers).fold(0, |mask, register|
                mask | crate::compile::liveness::register_bit(frame.base + register));
        }
        let mut starts = Vec::with_capacity(self.instructions.len() + 1);
        let mut successors = Vec::new();
        for (pc, instruction) in self.instructions.iter().enumerate() {
            starts.push(successors.len());
            if instruction.returns_from_frame() {
                let terminal = self.instructions.len().checked_sub(1)?;
                if self.instructions[terminal].op() != Op::Return { return None; }
                transfers[pc].1 |= crate::compile::liveness::register_bit(self.instructions[terminal].register_a());
                successors.push(terminal);
            } else {
                match instruction.op().control_flow_layout() {
                    crate::bytecode::ControlFlowLayout::Terminal => {},
                    crate::bytecode::ControlFlowLayout::Jump => successors.push(instruction.jump_target() as usize),
                    crate::bytecode::ControlFlowLayout::ConditionalJump => {
                        successors.push(instruction.jump_target() as usize);
                        successors.push(pc + 1);
                    }
                    _ => successors.push(pc + 1),
                }
            }
        }
        starts.push(successors.len());
        let live = crate::compile::liveness::analyze_graph(&transfers, &starts, &successors, |pc| masks[pc]);
        Some(std::borrow::Cow::Owned(RegisterMasks::new(live)))
    }

    /// A closed suffix entered with the caller's canonical homes at `header`.
    /// Source origins survive rebasing, including entry misses and liveness.
    #[cfg(feature = "aot")]
    pub(super) fn suffix(mut self, header: usize) -> Option<Self> {
        if !self.frames.is_empty() || header >= self.instructions.len() { return None; }
        self.instructions.drain(..header);
        self.origins.drain(..header);
        for instruction in &mut self.instructions {
            if instruction.op().immediate_role() == crate::bytecode::ImmediateRole::JumpTarget {
                let target = instruction.jump_target().checked_sub(u32::try_from(header).ok()?)?;
                instruction.set_jump_target(target);
            }
        }
        Some(self)
    }

    /// The closure whose depth-zero binding this instruction reads.
    pub(crate) fn capture_key(&self, pc: usize) -> (Option<usize>, u16) {
        let frame = match self.origins[pc] {
            Origin::Caller(_) => None,
            Origin::Inlined { frame, .. } => Some(frame),
        };
        (frame, self.instructions[pc].capture_slot())
    }
}

/// The longest callee body the unit inlines: each copy of it costs code and compile time at
/// every call site, and short bodies (accessors, field updates) are where a call costs most
/// relative to its work.
const MAX_INLINED_INSTRUCTIONS: usize = 48;

/// Whether the lane-body lowering runs `instruction` of an inlined callee: no call (one level),
/// depth-zero captures (admitted from the callee's environment), no fusion but a returning one, which splits
/// into the computation and the return (`osr_loop::inlined_returning`).
fn inlinable(instruction: WideInstruction) -> bool {
    let plain = instruction.result_flags() == 0;
    let value = plain || super::osr_loop::inlined_returning(instruction);
    match instruction.op() {
        Op::Return => plain,
        Op::Nop | Op::LoadConst | Op::Move | Op::LoadThis | Op::IncDec | Op::Jump | Op::JumpFalse | Op::JumpTrue
        | Op::JumpBinaryFalse => plain,
        Op::GetField => value && matches!(instruction.field_lookup(), Some(FieldLookup::Atom { .. })),
        Op::SetThisField | Op::SetThisFieldStrict => plain,
        Op::LoadCapture => plain && instruction.capture_depth() == 0,
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
            || !callee_code.iter().any(|instruction| expanded_len(*instruction) > 1)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remapped_liveness_covers_synthesized_reads_without_loading_callee_registers_at_entry() {
        let program = crate::Engine::specialize(
            "function leaf(x) { return x + 1; } function caller(x) { return this.leaf(x) + 2; }",
            "inline-liveness.js",
        ).unwrap();
        let id = |name: &str| program.functions.iter().position(|function|
            function.header().name.is_some_and(|atom| &program.atoms[atom as usize] == name)).unwrap() as u32;
        let caller = id("caller");
        let leaf = id("leaf");
        let body = program.function(caller as usize);
        let call = body.instructions().position(|instruction| instruction.op() == Op::Call).unwrap();
        let mut targets = vec![None; body.code.len()];
        targets[call] = Some(CallTarget { callee: Value::UNDEFINED, function: leaf });
        let mut unit = unit(body, &targets, &|function| program.function_get(function as usize)).unwrap();
        assert_eq!(unit.frames.len(), 1);
        let masks = unit.register_masks(&program, caller).expect("covered remapped graph");
        for register in unit.caller_registers..unit.registers {
            assert!(!masks.is_live(0, register), "callee register {register} is defined by call admission");
        }
        let (pc, instruction) = unit.instructions.iter().enumerate().find(|(pc, instruction)|
            instruction.op() == Op::Move && matches!(unit.origins[*pc], Origin::Inlined { .. })
                && instruction.result_register() < unit.caller_registers).expect("synthesized result move");
        assert!(masks.is_live(pc, instruction.register_b()), "return projection must retain its produced result");
        drop(masks);
        unit.registers = crate::compile::liveness::ROOT_MAP_REGISTERS + 1;
        assert!(unit.register_masks(&program, caller).is_none(), "uncovered widths retain conservative recovery");
    }
}
