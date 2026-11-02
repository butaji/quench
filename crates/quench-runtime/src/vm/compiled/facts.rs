//! Typed facts: the first slice of the typed IR (TOD-1241, docs/v2/specialization.md).
//!
//! An abstract interpretation of one function's bytecode over a type lattice, run to a
//! fixpoint across its control-flow graph before any code is lowered. It names, for every
//! block leader, the type each register holds on every path into it. A lowering reads the
//! facts to choose each register's representation at block entry, so a fact proved on one
//! side of a jump is not proved again on the other.
//!
//! Entry guards discharge input requirements. Lowering conforms each edge to its target's
//! semantic type and machine view: an edge proves what its state lacks with one guard, and
//! one that cannot leaves at the target, so a fact the analysis gets wrong costs an exit,
//! never the body. Constants, definitions and joins use the one `crate::typed_ir` lattice.

use crate::bytecode::{Constant, ControlFlowLayout, Op, Operand, OperandKind, Register, WideInstruction};
use crate::value::Value;
use crate::typed_ir::Type;
use cranelift_codegen::bitset::CompoundBitSet;
use std::num::NonZeroUsize;

/// Scratch storage spills for larger functions without limiting region size.
const INLINE_ANALYSIS_ITEMS: usize = 16;

/// The registers' types on entry to each block leader; `None` at a PC that leads no block or
/// that no path from the entry reaches.
#[derive(Default)]
pub(super) struct Facts {
    offsets: Vec<Option<NonZeroUsize>>,
    types: Vec<Type>,
    registers: usize,
}

impl Facts {
    /// The types on entry to the block led by `pc`. Rows share one flat buffer;
    /// the PC table stores element offsets, including reachable zero-register rows.
    pub(super) fn at(&self, pc: usize) -> Option<&[Type]> {
        let start = self.offsets.get(pc)?.as_ref()?.get() - 1;
        Some(&self.types[start..start + self.registers])
    }

    fn merge(&mut self, pc: usize, incoming: &[Type]) -> bool {
        let Some(slot) = self.offsets.get_mut(pc) else { return false };
        let Some(offset) = *slot else {
            *slot = NonZeroUsize::new(self.types.len() + 1);
            self.types.extend_from_slice(incoming);
            return true;
        };
        let start = offset.get() - 1;
        let mut changed = false;
        for (known, incoming) in self.types[start..start + self.registers].iter_mut().zip(incoming) {
            let joined = known.join(*incoming);
            changed |= joined != *known;
            *known = joined;
        }
        changed
    }
}

/// Where control goes after an instruction under the lowering this models.
enum Flow {
    /// The next instruction.
    Next,
    /// The explicit jump target; conditional fallthrough is handled separately.
    To(usize),
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

/// A numeric constant is a static semantic value, regardless of machine view.
pub(super) fn constant_number(constants: &[Constant], operand: Operand) -> Option<f64> {
    (operand.kind() == Some(OperandKind::Constant)).then_some(())?;
    constants.get(usize::from(operand.payload())).and_then(immediate).and_then(Value::as_number)
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
                    let right = constant_number(constants, instruction.operand_c());
                    let result = if bitwise.result_is_int32(right) { Type::INT32 } else { Type::NUMBER };
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
                None if super::super::operations::NumberRelation::from_operator(operator).is_some() => {
                    prove_number(types, [instruction.operand_b(), instruction.operand_c()]);
                    if !instruction.returns_from_frame() {
                        set(types, instruction.result_register(), Type::language(crate::ValueType::Boolean));
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
            Flow::To(instruction.jump_target() as usize)
        }
        Op::JumpBinaryFalse => {
            let operands = [instruction.operand_b(), instruction.operand_c()];
            let integer = operands.iter().all(|operand| operand_type(constants, *operand,
                |register| types.get(usize::from(register)).copied().unwrap_or(Type::ANY)).is_subtype(Type::INT32));
            if integer { prove_int32(types, operands); } else { prove_number(types, operands); }
            Flow::To(instruction.jump_target() as usize)
        }
        Op::Call if super::guarded_call_instruction(instruction) => {
            set(types, instruction.result_register(), Type::ANY);
            Flow::Next
        }
        _ => {
            for register in written(instruction) {
                set(types, register, Type::ANY);
            }
            match op.control_flow_layout() {
                ControlFlowLayout::Fallthrough => Flow::Next,
                ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump => {
                    Flow::To(instruction.jump_target() as usize)
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
    leaders: &CompoundBitSet,
    constants: &[Constant],
    written: &dyn Fn(WideInstruction) -> Vec<Register>,
    own_shape: &dyn Fn(usize) -> Option<(Register, u32)>,
) -> Facts {
    let mut facts = Facts { offsets: vec![None; instructions.len()], types: Vec::new(), registers: initial.len() };
    let mut pending = smallvec::SmallVec::<[usize; INLINE_ANALYSIS_ITEMS]>::new();
    let mut types = smallvec::SmallVec::<[Type; INLINE_ANALYSIS_ITEMS]>::new();
    if !instructions.is_empty() && facts.merge(0, initial) {
        pending.push(0);
    }
    while let Some(leader) = pending.pop() {
        types.clear();
        types.extend_from_slice(facts.at(leader).expect("a pending leader has facts"));
        let mut pc = leader;
        loop {
            let instruction = instructions[pc];
            // A direct own read proves its receiver before defining its result.
            // If result and receiver alias, transfer kills that refinement.
            if let Some((register, shape)) = own_shape(pc) {
                set(&mut types, register, Type::direct_object(shape));
            }
            let conditional = instruction.op().control_flow_layout() == ControlFlowLayout::ConditionalJump;
            match transfer(&mut types, instruction, constants, written) {
                Flow::Leaves => break,
                Flow::To(target) => {
                    if facts.merge(target, &types) { pending.push(target); }
                    if conditional && facts.merge(pc + 1, &types) { pending.push(pc + 1); }
                    break;
                }
                Flow::Next => {
                    pc += 1;
                    if pc >= instructions.len() {
                        break;
                    }
                    if leaders.contains(pc) {
                        if facts.merge(pc, &types) { pending.push(pc); }
                        break;
                    }
                }
            }
        }
    }
    facts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_widen_only_the_reached_block_and_converge_after_storage_growth() {
        let mut facts = Facts { offsets: vec![None; 4], types: Vec::new(), registers: 2 };
        assert!(facts.merge(2, &[Type::INT32, Type::direct_object(7)]));
        assert!(facts.merge(0, &[Type::NUMBER, Type::ANY]));
        assert!(facts.merge(2, &[Type::NUMBER, Type::direct_object(8)]));
        let widened = [Type::NUMBER, Type::language(crate::ValueType::Object)];
        assert_eq!(facts.at(2), Some(widened.as_slice()));
        assert_eq!(facts.at(0), Some([Type::NUMBER, Type::ANY].as_slice()));
        assert!(facts.at(1).is_none());
        assert!(!facts.merge(2, &[Type::INT32, Type::direct_object(7)]));
        assert!(!facts.merge(4, &[Type::ANY, Type::ANY]));
    }

    #[test]
    fn zero_register_blocks_keep_reachability_without_allocating_type_rows() {
        let mut facts = Facts { offsets: vec![None; 3], types: Vec::new(), registers: 0 };
        assert!(facts.merge(2, &[]));
        assert!(facts.merge(0, &[]));
        assert!(!facts.merge(2, &[]));
        assert_eq!(facts.at(0), Some([].as_slice()));
        assert!(facts.at(1).is_none());
        assert_eq!(facts.at(2), Some([].as_slice()));
        assert!(facts.types.is_empty());
    }
}
