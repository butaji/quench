//! Typed facts: the first slice of the typed IR (TOD-1241, Linear TOD-1232).
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

/// A guarded block input contract the fixed point keeps: the register holds the type on
/// entry to the leader, which every native edge into it proves (`Lowering::conform`) and
/// every in-loop definition preserves (`loop_int32`).
pub(super) type Contract = (usize, Register, Type);

/// The registers' types on entry to each block leader; `None` at a PC that leads no block or
/// that no path from the entry reaches.
#[derive(Default)]
pub(super) struct Facts {
    offsets: Vec<Option<NonZeroUsize>>,
    types: Vec<Type>,
    registers: usize,
    reached: CompoundBitSet,
    /// Join of the entry's backedge inputs, without the unknown external input.
    backedge: smallvec::SmallVec<[Type; INLINE_ANALYSIS_ITEMS]>,
    contracts: Vec<Contract>,
}

impl Facts {
    pub(super) fn reached(&self, pc: usize) -> bool { self.reached.contains(pc) }
    pub(super) fn backedge(&self) -> &[Type] { &self.backedge }
    /// The types on entry to the block led by `pc`. Rows share one flat buffer;
    /// the PC table stores element offsets, including reachable zero-register rows.
    pub(super) fn at(&self, pc: usize) -> Option<&[Type]> {
        let start = self.offsets.get(pc)?.as_ref()?.get() - 1;
        Some(&self.types[start..start + self.registers])
    }

    /// Select a guarded block input contract. Every native edge must conform
    /// before the lowering consumes this row; this is not a source inference.
    pub(super) fn require(&mut self, pc: usize, register: Register, required: Type) {
        let start = self.offsets[pc].expect("reachable contract block").get() - 1;
        let known = &mut self.types[start + usize::from(register)];
        if !known.is_subtype(required) { *known = required; }
    }

    /// Project a block's entry facts to an instruction using the same transfer
    /// semantics as the fixed point. This is a representation candidate; the
    /// lowering must still discharge entry and update requirements.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn before(&self, pc: usize, register: Register, instructions: &[WideInstruction],
        leaders: &CompoundBitSet, constants: &[Constant],
        written: &dyn Fn(WideInstruction) -> Vec<Register>, native_calls: bool) -> Type {
        let Some(start) = (0..=pc).rev().find(|pc| leaders.contains(*pc)) else { return Type::ANY };
        let Some(entry) = self.at(start) else { return Type::ANY };
        let mut types: smallvec::SmallVec<[Type; INLINE_ANALYSIS_ITEMS]> = entry.iter().copied().collect();
        for instruction in &instructions[start..pc] {
            if matches!(transfer(&mut types, *instruction, constants, written, native_calls), Flow::Leaves) {
                return Type::ANY;
            }
        }
        types.get(usize::from(register)).copied().unwrap_or(Type::ANY)
    }

    /// Project each reachable definition's post-state through the same transfer
    /// as the fixed point. Replay each block once, retaining no per-PC cache.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn for_each_definition(&self, instructions: &[WideInstruction], leaders: &CompoundBitSet,
        constants: &[Constant], written: &dyn Fn(WideInstruction) -> Vec<Register>, native_calls: bool,
        mut visit: impl FnMut(usize, Register, Type)) {
        let mut types = smallvec::SmallVec::<[Type; INLINE_ANALYSIS_ITEMS]>::new();
        for start in (0..instructions.len()).filter(|pc| leaders.contains(*pc)) {
            let Some(entry) = self.at(start) else { continue };
            types.clear();
            types.extend_from_slice(entry);
            for (pc, instruction) in instructions.iter().copied().enumerate().skip(start) {
                if pc != start && leaders.contains(pc) { break; }
                let flow = transfer(&mut types, instruction, constants, written, native_calls);
                for register in written(instruction) {
                    visit(pc, register, types.get(usize::from(register)).copied().unwrap_or(Type::ANY));
                }
                if !matches!(flow, Flow::Next) { break; }
            }
        }
    }

    fn merge(&mut self, pc: usize, incoming: &[Type]) -> bool {
        let Some(slot) = self.offsets.get_mut(pc) else { return false };
        let (start, mut changed) = match *slot {
            None => {
                *slot = NonZeroUsize::new(self.types.len() + 1);
                self.types.extend_from_slice(incoming);
                (self.types.len() - incoming.len(), true)
            }
            Some(offset) => {
                let start = offset.get() - 1;
                let mut changed = false;
                for (known, incoming) in self.types[start..start + self.registers].iter_mut().zip(incoming) {
                    let joined = known.join(*incoming);
                    changed |= joined != *known;
                    *known = joined;
                }
                (start, changed)
            }
        };
        for &(_, register, required) in self.contracts.iter().filter(|(at, _, _)| *at == pc) {
            let known = &mut self.types[start + usize::from(register)];
            if !known.is_subtype(required) {
                *known = required;
                changed = true;
            }
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

/// Whether a binary `operator` yields a Boolean however it completes: an equality, a
/// relation, `instanceof` or `in`.
pub(super) fn boolean_binary(operator: u32) -> bool {
    use oxc_ast::ast::BinaryOperator;
    super::super::operations::equality_operator(operator).is_some()
        || super::super::operations::NumberRelation::from_operator(operator).is_some()
        || [BinaryOperator::Instanceof, BinaryOperator::In].iter().any(|relation| *relation as u32 == operator)
}

/// Applies `instruction` to `types`; returns where control goes next. A call continues where
/// the region makes native calls (`native_calls`), past which no object keeps its shape.
fn transfer(
    types: &mut [Type],
    instruction: WideInstruction,
    constants: &[Constant],
    written: &dyn Fn(WideInstruction) -> Vec<Register>,
    native_calls: bool,
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
                    // ToInt32 of any Number runs natively; anything else leaves.
                    prove_number(types, [instruction.operand_b(), instruction.operand_c()]);
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
                    // An equality decides operands of any type (`lane_region::relation`) and
                    // proves nothing of them; every other relation compares Numbers.
                    if super::super::operations::equality_operator(operator).is_none() {
                        prove_number(types, [instruction.operand_b(), instruction.operand_c()]);
                    }
                    if !instruction.returns_from_frame() {
                        set(types, instruction.result_register(), Type::language(crate::ValueType::Boolean));
                    }
                }
                None => {
                    for register in written(instruction) {
                        set(types, register, Type::ANY);
                    }
                    if boolean_binary(operator) && !instruction.returns_from_frame() {
                        set(types, instruction.result_register(), Type::language(crate::ValueType::Boolean));
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
            // An equality, loose or strict, and `instanceof` decide operands of any type
            // (`lane_region::relation`: two objects compare by identity) and prove nothing of
            // them; every other relation compares Numbers.
            let operands = [instruction.operand_b(), instruction.operand_c()];
            let operand_types = operands.map(|operand| operand_type(constants, operand,
                |register| types.get(usize::from(register)).copied().unwrap_or(Type::ANY)));
            let operator = instruction.binary_operator_field();
            let numeric = super::super::operations::NumberRelation::from_operator(operator).is_some()
                && super::super::operations::equality_operator(operator).is_none();
            if operand_types.iter().all(|ty| ty.is_subtype(Type::INT32)) { prove_int32(types, operands); }
            else if numeric { prove_number(types, operands); }
            Flow::To(instruction.jump_target() as usize)
        }
        Op::Call if super::guarded_call_instruction(instruction) => {
            set(types, instruction.result_register(), Type::ANY);
            Flow::Next
        }
        Op::Call if native_calls => {
            for ty in types.iter_mut() {
                *ty = ty.without_object_shape();
            }
            set(types, instruction.result_register(), Type::ANY);
            Flow::Next
        }
        _ => {
            for register in written(instruction) {
                set(types, register, Type::ANY);
            }
            match op.control_flow_layout() {
                ControlFlowLayout::Fallthrough if matches!(op, Op::Nop | Op::LoadThis | Op::LoadCapture
                    | Op::GetField | Op::GetIndex | Op::SetField | Op::SetFieldStrict
                    | Op::SetThisField | Op::SetThisFieldStrict | Op::SetIndex) => Flow::Next,
                ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump => {
                    Flow::To(instruction.jump_target() as usize)
                }
                _ => Flow::Leaves,
            }
        }
    }
}

/// The fixpoint of `transfer` over the CFG, seeded by checked region-entry facts.
pub(super) fn analyze(
    entry: usize,
    initial: &[Type],
    instructions: &[WideInstruction],
    leaders: &CompoundBitSet,
    constants: &[Constant],
    written: &dyn Fn(WideInstruction) -> Vec<Register>,
    own_shape: &dyn Fn(usize) -> Option<(Register, u32)>,
    native_calls: bool,
    contracts: &[Contract],
) -> Facts {
    let mut facts = Facts { offsets: vec![None; instructions.len()], types: Vec::new(), registers: initial.len(),
        reached: CompoundBitSet::with_capacity(instructions.len()), backedge: smallvec::SmallVec::new(),
        contracts: contracts.to_vec() };
    let mut pending = smallvec::SmallVec::<[usize; INLINE_ANALYSIS_ITEMS]>::new();
    let mut types = smallvec::SmallVec::<[Type; INLINE_ANALYSIS_ITEMS]>::new();
    if entry < instructions.len() && facts.merge(entry, initial) {
        pending.push(entry);
    }
    while let Some(leader) = pending.pop() {
        types.clear();
        types.extend_from_slice(facts.at(leader).expect("a pending leader has facts"));
        let mut pc = leader;
        loop {
            facts.reached.insert(pc);
            let instruction = instructions[pc];
            // A direct own read proves its receiver before defining its result.
            // If result and receiver alias, transfer kills that refinement.
            if let Some((register, shape)) = own_shape(pc) {
                set(&mut types, register, Type::direct_object(shape));
            }
            let conditional = instruction.op().control_flow_layout() == ControlFlowLayout::ConditionalJump;
            match transfer(&mut types, instruction, constants, written, native_calls) {
                Flow::Leaves => break,
                Flow::To(target) => {
                    if target == entry && target <= pc {
                        if facts.backedge.is_empty() { facts.backedge.extend_from_slice(&types); }
                        else { for (known, incoming) in facts.backedge.iter_mut().zip(&types) { *known = known.join(*incoming); } }
                    }
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

/// Prove a checked integer entry contract by induction. Every native write
/// and every backedge must preserve the entry hypothesis. Rejecting one input
/// removes that assumption from all dependent proofs on the next iteration.
#[allow(clippy::too_many_arguments)]
pub(super) fn inductive_int32(initial: &mut [Type], mut candidates: CompoundBitSet,
    body: std::ops::RangeInclusive<usize>, instructions: &[WideInstruction], leaders: &CompoundBitSet,
    constants: &[Constant], written: &dyn Fn(WideInstruction) -> Vec<Register>,
    own_shape: &dyn Fn(usize) -> Option<(Register, u32)>, native_calls: bool) -> Option<Facts> {
    // Each unsuccessful iteration removes at least one candidate. Compile
    // work is bounded by candidate count, without a semantic iteration cap.
    while !candidates.is_empty() {
        let mut inputs = initial.to_vec();
        for register in candidates.iter() { inputs[register] = Type::INT32; }
        let facts = analyze(*body.start(), &inputs, instructions, leaders, constants, written, own_shape, native_calls, &[]);
        let mut rejected: CompoundBitSet = CompoundBitSet::with_capacity(initial.len());
        for register in candidates.iter() {
            if facts.backedge().get(register) != Some(&Type::INT32) { rejected.insert(register); }
        }
        facts.for_each_definition(instructions, leaders, constants, written, native_calls, |pc, register, ty| {
            let index = usize::from(register);
            if body.contains(&pc) && candidates.contains(index) && ty != Type::INT32 { rejected.insert(index); }
        });
        if rejected.is_empty() {
            initial.copy_from_slice(&inputs);
            return Some(facts);
        }
        for register in rejected.iter() { candidates.remove(register); }
    }
    None
}

/// Prove int32 header contracts of inner loops by induction: each candidate (a loop header
/// and a register) holds an int32 there when every definition in its loop's body yields one
/// under all the contracts. Rejecting one removes it from the next iteration's hypothesis.
#[allow(clippy::too_many_arguments)]
pub(super) fn loop_int32(entry: usize, initial: &[Type], mut candidates: Vec<(Contract, std::ops::RangeInclusive<usize>)>,
    instructions: &[WideInstruction], leaders: &CompoundBitSet, constants: &[Constant],
    written: &dyn Fn(WideInstruction) -> Vec<Register>, own_shape: &dyn Fn(usize) -> Option<(Register, u32)>,
    native_calls: bool) -> Option<Facts> {
    while !candidates.is_empty() {
        let contracts: Vec<Contract> = candidates.iter().map(|(contract, _)| *contract).collect();
        let facts = analyze(entry, initial, instructions, leaders, constants, written, own_shape, native_calls, &contracts);
        let mut rejected = vec![false; candidates.len()];
        facts.for_each_definition(instructions, leaders, constants, written, native_calls, |pc, register, ty| {
            for (index, ((_, candidate, required), body)) in candidates.iter().enumerate() {
                if *candidate == register && body.contains(&pc) && !ty.is_subtype(*required) { rejected[index] = true; }
            }
        });
        if !rejected.contains(&true) { return Some(facts); }
        let mut index = 0;
        candidates.retain(|_| { index += 1; !rejected[index - 1] });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_call_retires_conditional_shapes_in_cfg_facts() {
        let program = crate::Engine::specialize(
            "function caller(f, value) { f(value); return value; }", "shape-call.js").unwrap();
        let function = program.function(1);
        let call = function.instructions().find(|instruction| instruction.op() == Op::Call).unwrap();
        assert!(!super::super::guarded_call_instruction(call));
        let null = Type::language(crate::ValueType::Null);
        for ty in [Type::direct_object(7), Type::direct_object(7).join(null),
            Type::direct_object(7).join(Type::NULLISH), Type::INT32, Type::ANY] {
            let mut types = vec![ty; usize::from(function.registers)];
            assert!(matches!(transfer(&mut types, call, &program.constants,
                &super::super::lane_region::writes, true), Flow::Next));
            for (register, actual) in types.into_iter().enumerate() {
                let expected = if register == usize::from(call.result_register()) { Type::ANY }
                    else { ty.without_object_shape() };
                assert_eq!(actual, expected, "{ty:?}, register {register}");
            }
        }
    }

    fn infer_inductive(body: &str, names: &[&str]) -> (Vec<Type>, Vec<Register>) {
        let program = crate::Engine::specialize(&format!("function loop(n,flag,seed){{{body}}}"), "inductive.js").unwrap();
        let function = program.function(1);
        let instructions: Vec<_> = function.instructions().collect();
        let leaders = super::super::lane_region::leaders(&instructions);
        let edges: Vec<_> = instructions.iter().enumerate().filter_map(|(pc, instruction)|
            matches!(instruction.op().control_flow_layout(), ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump)
                .then(|| instruction.jump_target() as usize).filter(|target| *target <= pc).map(|target| (target, pc))).collect();
        let header = edges.iter().map(|(header, _)| *header).min().unwrap();
        let back = edges.iter().filter(|(target, _)| *target == header).map(|(_, pc)| *pc).max().unwrap();
        // These fixtures initialize the tested homes from their third input.
        // Promoted locals need not retain debugger names in the bytecode.
        let homes: Vec<_> = instructions[..header].iter().filter(|instruction|
            instruction.op() == Op::Move && instruction.register_b() == 2)
            .map(|instruction| instruction.result_register()).take(names.len()).collect();
        assert_eq!(homes.len(), names.len(), "candidate homes {names:?}: {}", program.disassemble());
        let mut initial = vec![Type::ANY; usize::from(function.registers)];
        let mut candidates: CompoundBitSet = CompoundBitSet::with_capacity(initial.len());
        for register in &homes { candidates.insert(usize::from(*register)); }
        inductive_int32(&mut initial, candidates, header..=back, &instructions, &leaders, &program.constants,
            &super::super::lane_region::writes, &|_| None, false);
        (initial, homes)
    }

    #[test]
    fn inductive_integer_definitions_preserve_unchanged_arms_and_all_latches() {
        for body in [
            "let acc=seed,i=0;while(i<n){i=(i+1)|0;if(flag)acc=(acc+1)|0;}return acc;",
            "let acc=seed,i=0;while(i<n){i=(i+1)|0;if(flag)continue;acc=(acc+1)|0;}return acc;",
            "let acc=seed,i=0;while(i<n){i=(i+1)|0;if(flag)acc=(acc+1)|0;else acc=(acc-1)|0;}return acc;",
        ] {
            let (initial, homes) = infer_inductive(body, &["acc"]);
            assert_eq!(initial[usize::from(homes[0])], Type::INT32, "{body}");
        }
    }

    #[test]
    fn inductive_integer_definitions_reject_wide_and_unproved_writes() {
        for update in ["acc=0.5", "acc=2147483648", "acc='text'", "acc=1n", "acc=true", "acc=seed"] {
            let body = format!("let acc=seed,i=0;while(i<n){{i=(i+1)|0;if(flag)acc=(acc+1)|0;else {update};}}return acc;");
            let (initial, homes) = infer_inductive(&body, &["acc"]);
            assert_eq!(initial[usize::from(homes[0])], Type::ANY, "{update}");
        }
    }

    #[test]
    fn inductive_integer_definitions_remove_failed_dependencies() {
        let body = "let x=seed,y=seed,i=0;while(i<n){i=(i+1)|0;if(flag){x=0;y=0;}else{x=y;y=0.5;}}return x;";
        let (initial, homes) = infer_inductive(body, &["x", "y"]);
        assert!(homes.iter().all(|home| initial[usize::from(*home)] == Type::ANY));
        // A home without its own candidate proof cannot supply an I32 fact.
        let (initial, homes) = infer_inductive("let x=seed,y=seed,i=0;while(i<n){i=(i+1)|0;if(flag)x=0;else x=y;}return x;", &["x"]);
        assert_eq!(initial[usize::from(homes[0])], Type::ANY);
    }

    #[test]
    fn joins_widen_only_the_reached_block_and_converge_after_storage_growth() {
        let mut facts = Facts { offsets: vec![None; 4], types: Vec::new(), registers: 2, ..Facts::default() };
        assert!(facts.merge(2, &[Type::INT32, Type::direct_object(7)]));
        assert!(facts.merge(0, &[Type::NUMBER, Type::ANY]));
        assert!(facts.merge(2, &[Type::NUMBER, Type::direct_object(8)]));
        let widened = [Type::NUMBER, Type::DIRECT_OBJECT_FAMILY];
        assert_eq!(facts.at(2), Some(widened.as_slice()));
        assert_eq!(facts.at(0), Some([Type::NUMBER, Type::ANY].as_slice()));
        assert!(facts.at(1).is_none());
        assert!(!facts.merge(2, &[Type::INT32, Type::direct_object(7)]));
        assert!(!facts.merge(4, &[Type::ANY, Type::ANY]));
    }

    #[test]
    fn zero_register_blocks_keep_reachability_without_allocating_type_rows() {
        let mut facts = Facts { offsets: vec![None; 3], types: Vec::new(), registers: 0, ..Facts::default() };
        assert!(facts.merge(2, &[]));
        assert!(facts.merge(0, &[]));
        assert!(!facts.merge(2, &[]));
        assert_eq!(facts.at(0), Some([].as_slice()));
        assert!(facts.at(1).is_none());
        assert_eq!(facts.at(2), Some([].as_slice()));
        assert!(facts.types.is_empty());
    }
}
