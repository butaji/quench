//! Typed facts: the first slice of the typed IR (TOD-1241, docs/v2/specialization.md).
//!
//! An abstract interpretation of one function's bytecode over a type lattice, run to a
//! fixpoint across its control-flow graph before any code is lowered. It names, for every
//! block leader, the type each register holds on every path into it. A lowering reads the
//! facts to choose each register's representation at block entry, so a fact proved on one
//! side of a jump is not proved again on the other.
//!
//! Entry guards discharge input requirements. Lowering conforms each edge to its target's
//! semantic type and machine view; an incompatible join declines publication. Constants,
//! definitions and joins use the same Type lattice as the shared OSR/AOT region compiler.

use crate::bytecode::{Constant, ControlFlowLayout, Op, Operand, OperandKind, Register, WideInstruction};
use crate::value::Value;
use crate::specialize::Type;

/// The registers' types on entry to each block leader; `None` at a PC that leads no block or
/// that no path from the entry reaches.
#[derive(Default)]
pub(super) struct Facts {
    entry: Vec<Option<Vec<Type>>>,
}

impl Facts {
    /// The types on entry to the block led by `pc`.
    pub(super) fn at(&self, pc: usize) -> Option<&[Type]> {
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

/// The value a constant denotes where it is an immediate (no heap allocation): numbers,
/// booleans, `null` and `undefined`.
pub(super) fn immediate(constant: &Constant) -> Option<Value> {
    match constant {
        Constant::Number(number) => Some(Value::number(*number)),
        Constant::Boolean(value) => Some(Value::boolean(*value)),
        Constant::Null => Some(Value::NULL),
        Constant::Undefined => Some(Value::UNDEFINED),
        _ => None,
    }
}

/// The semantic type of an operand, independent of its selected machine view.
pub(super) fn operand_type(constants: &[Constant], operand: Operand, register_type: impl Fn(Register) -> Type) -> Type {
    match operand.kind() {
        Some(OperandKind::Register) => register_type(operand.payload()),
        Some(OperandKind::Constant) => constants.get(usize::from(operand.payload())).and_then(immediate)
            .map_or(Type::ANY, Type::immediate),
        _ => Type::ANY,
    }
}

fn prove_number(types: &mut [Type], operands: [Operand; 2]) {
    for register in operands.into_iter().filter_map(register) {
        if !types.get(usize::from(register)).is_some_and(|ty| ty.is_subtype(Type::NUMBER)) {
            set(types, register, Type::NUMBER);
        }
    }
}

/// Whether an immediate is an int32 Number in its tagged encoding.
pub(super) fn is_int32(value: Value) -> bool {
    Type::immediate(value).is_subtype(Type::INT32)
}

/// Proves each register operand int32: the instruction leaves where one is not.
fn prove_int32(types: &mut [Type], operands: [Operand; 2]) {
    for register in operands.into_iter().filter_map(register) {
        set(types, register, Type::INT32);
    }
}

fn set(types: &mut [Type], register: Register, ty: Type) {
    if let Some(slot) = types.get_mut(usize::from(register)) {
        *slot = ty;
    }
}

/// Applies `instruction` to `types`; returns where control goes next.
fn transfer(
    types: &mut [Type],
    instruction: WideInstruction,
    constants: &[Constant],
    written: &dyn Fn(WideInstruction) -> Vec<Register>,
) -> Flow {
    let op = instruction.op();
    match op {
        Op::LoadConst => {
            let ty = constants.get(instruction.constant_index()).and_then(immediate)
                .map_or(Type::ANY, Type::immediate);
            set(types, instruction.result_register(), ty);
            Flow::Next
        }
        Op::Move => {
            let source = types.get(usize::from(instruction.register_b())).copied().unwrap_or(Type::ANY);
            set(types, instruction.result_register(), source);
            Flow::Next
        }
        Op::IncDec if instruction.numeric_local_target().is_none() => {
            // The operand is proved int32 or the instruction leaves.
            set(types, instruction.register_b(), Type::INT32);
            set(types, instruction.result_register(), Type::INT32);
            Flow::Next
        }
        _ if op.js_denotation() == crate::bytecode::JsDenotation::NumericBinary => {
            let operator = instruction.binary_operator();
            match super::super::operations::NumberBitwise::from_js_instruction(op, operator) {
                Some(bitwise) => {
                    prove_int32(types, [instruction.operand_b(), instruction.operand_c()]);
                    let result = if bitwise == super::super::operations::NumberBitwise::ShiftRightUnsigned {
                        Type::NUMBER
                    } else {
                        Type::INT32
                    };
                    if !instruction.returns_from_frame() {
                        set(types, instruction.result_register(), result);
                    }
                }
                None if super::super::operations::NumberArithmetic::from_js_instruction(op, operator).is_some() => {
                    prove_number(types, [instruction.operand_b(), instruction.operand_c()]);
                    if !instruction.returns_from_frame() {
                        set(types, instruction.result_register(), Type::NUMBER);
                    }
                }
                None => {
                    for register in written(instruction) {
                        set(types, register, Type::ANY);
                    }
                }
            }
            if instruction.returns_from_frame() { Flow::Leaves } else { Flow::Next }
        }
        Op::JumpBinaryFalse if super::nullish_comparison(constants, instruction,
            |register| types.get(usize::from(register)).copied().unwrap_or(Type::ANY)).is_some() => {
            Flow::To(vec![instruction.jump_target() as usize])
        }
        Op::JumpBinaryFalse => {
            let operands = [instruction.operand_b(), instruction.operand_c()];
            let integer = operands.iter().all(|operand| operand_type(constants, *operand,
                |register| types.get(usize::from(register)).copied().unwrap_or(Type::ANY)).is_subtype(Type::INT32));
            if integer { prove_int32(types, operands); } else { prove_number(types, operands); }
            Flow::To(vec![instruction.jump_target() as usize])
        }
        _ => {
            for register in written(instruction) {
                set(types, register, Type::ANY);
            }
            match op.control_flow_layout() {
                ControlFlowLayout::Fallthrough => Flow::Next,
                ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump => {
                    Flow::To(vec![instruction.jump_target() as usize])
                }
                ControlFlowLayout::Call | ControlFlowLayout::BranchTable | ControlFlowLayout::Terminal => Flow::Leaves,
            }
        }
    }
}

/// The fixpoint of `transfer` over the CFG, seeded by checked region-entry facts.
pub(super) fn analyze(
    initial: &[Type],
    instructions: &[WideInstruction],
    leaders: &[bool],
    constants: &[Constant],
    written: &dyn Fn(WideInstruction) -> Vec<Register>,
) -> Facts {
    let mut entry: Vec<Option<Vec<Type>>> = vec![None; instructions.len()];
    let mut pending = Vec::new();
    let merge = |entry: &mut Vec<Option<Vec<Type>>>, pending: &mut Vec<usize>, pc: usize, types: &[Type]| {
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
        merge(&mut entry, &mut pending, 0, initial);
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
    Facts { entry }
}
