//! Typed facts: the first slice of the typed IR (TOD-1241, docs/v2/specialization.md).
//!
//! An abstract interpretation of one function's bytecode over a type lattice, run to a
//! fixpoint across its control-flow graph before any code is lowered. It names, for every
//! block leader, the type each register holds on every path into it. A lowering reads the
//! facts to choose each register's representation at block entry, so a fact proved on one
//! side of a jump is not proved again on the other.
//!
//! The facts steer code quality, not soundness: a lowering conforms every edge to its
//! target's facts, proving at the edge whatever its own state lacks (`Conform`). A fact the
//! analysis gets wrong therefore costs a guard, never a wrong value.

use crate::bytecode::{Constant, ControlFlowLayout, Op, Operand, OperandKind, Register, WideInstruction};

/// What a register is known to hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Ty {
    /// Anything: only its tagged bits are meaningful.
    Tagged,
    /// An int32 Number.
    Int32,
}

impl Ty {
    /// The type both `self` and `other` satisfy.
    fn join(self, other: Self) -> Self {
        if self == other { self } else { Ty::Tagged }
    }
}

/// The registers' types on entry to each block leader; `None` at a PC that leads no block or
/// that no path from the entry reaches.
#[derive(Default)]
pub(super) struct Facts {
    entry: Vec<Option<Vec<Ty>>>,
}

impl Facts {
    /// The types on entry to the block led by `pc`.
    pub(super) fn at(&self, pc: usize) -> Option<&[Ty]> {
        self.entry.get(pc).and_then(|types| types.as_deref())
    }
}

/// Where control goes after an instruction under the lowering this models.
enum Flow {
    /// The next instruction.
    Next,
    /// The listed PCs.
    To(Vec<usize>),
    /// Leaves the function's native code.
    Leaves,
}

fn register(operand: Operand) -> Option<Register> {
    (operand.kind()? == OperandKind::Register).then(|| operand.register_index()).flatten()
}

fn int32_constant(constants: &[Constant], index: usize) -> bool {
    match constants.get(index) {
        Some(Constant::Number(number)) => {
            let int = *number as i32;
            f64::from(int) == *number && !(int == 0 && number.is_sign_negative())
        }
        _ => false,
    }
}

/// Applies `instruction` to `types`; returns where control goes next.
fn transfer(
    types: &mut [Ty],
    instruction: WideInstruction,
    constants: &[Constant],
    written: &dyn Fn(WideInstruction) -> Vec<Register>,
) -> Flow {
    let set = |types: &mut [Ty], register: Register, ty: Ty| {
        if let Some(slot) = types.get_mut(usize::from(register)) {
            *slot = ty;
        }
    };
    let op = instruction.op();
    match op {
        Op::LoadConst => {
            let ty = if int32_constant(constants, instruction.constant_index()) { Ty::Int32 } else { Ty::Tagged };
            set(types, instruction.result_register(), ty);
            Flow::Next
        }
        Op::Move => {
            let source = types.get(usize::from(instruction.register_b())).copied().unwrap_or(Ty::Tagged);
            set(types, instruction.result_register(), source);
            Flow::Next
        }
        Op::IncDec if instruction.numeric_local_target().is_none() => {
            // The operand is proved int32 or the instruction leaves.
            set(types, instruction.register_b(), Ty::Int32);
            set(types, instruction.result_register(), Ty::Int32);
            Flow::Next
        }
        _ if op.js_denotation() == crate::bytecode::JsDenotation::NumericBinary => {
            let operator = instruction.binary_operator();
            match super::super::operations::NumberBitwise::from_js_instruction(op, operator) {
                Some(bitwise) => {
                    // Bitwise operands are proved int32 or the instruction leaves.
                    for operand in [instruction.operand_b(), instruction.operand_c()] {
                        if let Some(register) = register(operand) {
                            set(types, register, Ty::Int32);
                        }
                    }
                    let result = if bitwise == super::super::operations::NumberBitwise::ShiftRightUnsigned {
                        Ty::Tagged
                    } else {
                        Ty::Int32
                    };
                    if !instruction.returns_from_frame() {
                        set(types, instruction.result_register(), result);
                    }
                }
                None => {
                    for register in written(instruction) {
                        set(types, register, Ty::Tagged);
                    }
                }
            }
            if instruction.returns_from_frame() { Flow::Leaves } else { Flow::Next }
        }
        Op::JumpBinaryFalse => {
            // A relation's operands are proved int32 on both edges, or it leaves.
            for operand in [instruction.operand_b(), instruction.operand_c()] {
                if let Some(register) = register(operand) {
                    set(types, register, Ty::Int32);
                }
            }
            Flow::To(vec![instruction.jump_target() as usize])
        }
        _ => {
            for register in written(instruction) {
                set(types, register, Ty::Tagged);
            }
            match op.control_flow_layout() {
                ControlFlowLayout::Fallthrough => Flow::Next,
                ControlFlowLayout::Jump => Flow::To(vec![instruction.jump_target() as usize]),
                ControlFlowLayout::ConditionalJump => Flow::To(vec![instruction.jump_target() as usize]),
                ControlFlowLayout::Call | ControlFlowLayout::BranchTable | ControlFlowLayout::Terminal => Flow::Leaves,
            }
        }
    }
}

/// The fixpoint of `transfer` over the blocks `leaders` mark, from every register `Tagged`
/// at PC 0.
pub(super) fn analyze(
    registers: usize,
    instructions: &[WideInstruction],
    leaders: &[bool],
    constants: &[Constant],
    written: &dyn Fn(WideInstruction) -> Vec<Register>,
) -> Facts {
    let mut entry: Vec<Option<Vec<Ty>>> = vec![None; instructions.len()];
    let mut pending = Vec::new();
    let merge = |entry: &mut Vec<Option<Vec<Ty>>>, pending: &mut Vec<usize>, pc: usize, types: &[Ty]| {
        let Some(slot) = entry.get_mut(pc) else { return };
        match slot {
            None => {
                *slot = Some(types.to_vec());
                pending.push(pc);
            }
            Some(known) => {
                let mut changed = false;
                for (known, incoming) in known.iter_mut().zip(types) {
                    let joined = known.join(*incoming);
                    changed |= joined != *known;
                    *known = joined;
                }
                if changed {
                    pending.push(pc);
                }
            }
        }
    };
    if !instructions.is_empty() {
        merge(&mut entry, &mut pending, 0, &vec![Ty::Tagged; registers]);
    }
    while let Some(leader) = pending.pop() {
        let mut types = entry[leader].clone().expect("a pending leader has facts");
        let mut pc = leader;
        loop {
            let instruction = instructions[pc];
            let conditional = instruction.op().control_flow_layout() == ControlFlowLayout::ConditionalJump;
            match transfer(&mut types, instruction, constants, written) {
                Flow::Leaves => break,
                Flow::To(targets) => {
                    for target in targets {
                        merge(&mut entry, &mut pending, target, &types);
                    }
                    if conditional {
                        merge(&mut entry, &mut pending, pc + 1, &types);
                    }
                    break;
                }
                Flow::Next => {
                    pc += 1;
                    if pc >= instructions.len() {
                        break;
                    }
                    if leaders[pc] {
                        merge(&mut entry, &mut pending, pc, &types);
                        break;
                    }
                }
            }
        }
    }
    for (pc, facts) in entry.iter_mut().enumerate() {
        if !leaders.get(pc).copied().unwrap_or(false) {
            *facts = None;
        }
    }
    Facts { entry }
}
