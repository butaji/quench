//! Binding-time reduction of finite scalar control flow, shared by every native tier.
//! A loop with a statically true first test and false post-body test has no backedge.
//! Instructions keep their original unit PCs, origins and recovery homes.

use super::{facts, lane_unit::Unit};
use crate::{Value, bytecode::{Class, ControlFlowLayout, Op, Operand, OperandKind, ResidualProgram, WideInstruction}};
use crate::vm::operations::{NumberArithmetic, NumberBitwise, NumberF64Stage, NumberRelation, PartialOrderStage};
use cranelift_codegen::bitset::CompoundBitSet;

/// Small functions keep transient register facts on the compiler's stack.
const INLINE_CONSTANT_REGISTERS: usize = 16;

/// Flat constant rows. An unreached PC has no row; `None` in a row is dynamic.
struct Constants {
    offsets: Vec<Option<usize>>,
    values: Vec<Option<Value>>,
    registers: usize,
}

impl Constants {
    fn at(&self, pc: usize) -> Option<&[Option<Value>]> {
        let offset = self.offsets.get(pc).copied().flatten()?;
        Some(&self.values[offset..offset + self.registers])
    }

    fn merge(&mut self, pc: usize, incoming: &[Option<Value>]) -> bool {
        let Some(slot) = self.offsets.get_mut(pc) else { return false; };
        let Some(offset) = *slot else {
            *slot = Some(self.values.len());
            self.values.extend_from_slice(incoming);
            return true;
        };
        let mut changed = false;
        for (known, incoming) in self.values[offset..offset + self.registers].iter_mut().zip(incoming) {
            if known.is_some() && known.map(Value::to_bits) != incoming.map(Value::to_bits) {
                *known = None;
                changed = true;
            }
        }
        changed
    }
}

fn operand(program: &ResidualProgram, values: &[Option<Value>], operand: Operand) -> Option<Value> {
    match operand.kind()? {
        OperandKind::Constant => facts::immediate(program.constants.get(usize::from(operand.payload()))?),
        OperandKind::Register => values.get(usize::from(operand.payload())).copied().flatten(),
        _ => None,
    }
}

/// Static evaluation uses the interpreter's Number denotations. Unknown operands
/// remain dynamic; no guest conversion or effect is executed by the compiler.
fn scalar(program: &ResidualProgram, values: &mut [Option<Value>], instruction: WideInstruction) {
    if !scalar_body(instruction) {
        if instruction.op().loads_frame_local() && instruction.numeric_local_target().is_none() {
            // Reading a guest local defines its result without changing other
            // register homes. Its contents remain dynamic, including TDZ exits.
            for register in super::lane_region::writes(instruction) { values[usize::from(register)] = None; }
        } else if !instruction.op().is_nop() && !instruction.op().control_flow_layout().jumps() {
            values.fill(None);
        }
        return;
    }
    let value = match instruction.op().class() {
        Class::LoadConst => program.constants.get(instruction.constant_index()).and_then(facts::immediate),
        Class::Move => values.get(usize::from(instruction.register_b())).copied().flatten(),
        Class::IncDec if instruction.numeric_local_target().is_none() => {
            values.get(usize::from(instruction.register_b())).copied().flatten().and_then(Value::as_number)
                .zip(instruction.boolean_flag()).map(|(value, decrement)| Value::number(if decrement { value - 1.0 } else { value + 1.0 }))
        }
        _ if instruction.op().js_denotation() == crate::bytecode::JsDenotation::NumericBinary && instruction.numeric_local_target().is_none() => {
            let inputs = operand(program, values, instruction.operand_b()).and_then(Value::as_number)
                .zip(operand(program, values, instruction.operand_c()).and_then(Value::as_number));
            inputs.and_then(|(left, right)| {
                let operator = instruction.binary_operator();
                if let Some(operation) = NumberArithmetic::from_js_instruction(instruction.op(), operator) {
                    Some(Value::number(operation.apply(&mut NumberF64Stage, left, right)))
                } else if let Some(operation) = NumberBitwise::from_js_instruction(instruction.op(), operator) {
                    Some(crate::vm::operations::number_bitwise_result(operation,
                        crate::value::number_to_u32(left) as i32, crate::value::number_to_u32(right) as i32))
                } else {
                    NumberRelation::from_operator(operator).map(|operation|
                        Value::boolean(operation.apply(&mut PartialOrderStage, left, right)))
                }
            })
        }
        _ => None,
    };
    for register in super::lane_region::writes(instruction) {
        values[usize::from(register)] = if register == instruction.result_register() { value } else { None };
    }
}

fn scalar_body(instruction: WideInstruction) -> bool {
    instruction.result_flags() == 0 && instruction.numeric_local_target().is_none()
        && (matches!(instruction.op().class(), Class::LoadConst | Class::Move | Class::IncDec)
            || (instruction.op().js_denotation() == crate::bytecode::JsDenotation::NumericBinary
                && (NumberArithmetic::from_js_instruction(instruction.op(), instruction.binary_operator()).is_some()
                    || NumberBitwise::from_js_instruction(instruction.op(), instruction.binary_operator()).is_some()
                    || NumberRelation::from_operator(instruction.binary_operator()).is_some())))
}

fn test(program: &ResidualProgram, values: &[Option<Value>], instruction: WideInstruction) -> Option<bool> {
    let relation = NumberRelation::from_operator(instruction.binary_operator_field())?;
    let left = operand(program, values, instruction.operand_b())?.as_number()?;
    let right = operand(program, values, instruction.operand_c())?.as_number()?;
    Some(relation.apply(&mut PartialOrderStage, left, right))
}

/// Solve a conservative CFG with this latch redirected to its exit. Both arms
/// of every conditional remain reachable. The final first/post tests validate
/// the redirection against every incoming state, including enclosing backedges.
fn constants(program: &ResidualProgram, unit: &Unit, instructions: &[WideInstruction], entry: usize,
    latch: usize, exit: usize) -> Constants {
    let count = usize::from(unit.registers);
    let mut known = Constants { offsets: vec![None; instructions.len()], values: Vec::new(), registers: count };
    let mut pending = Vec::new();
    let mut queued: CompoundBitSet = CompoundBitSet::with_capacity(instructions.len());
    let mut leaders = super::lane_region::leaders(instructions);
    leaders.insert(entry);
    leaders.insert(instructions[latch].jump_target() as usize);
    leaders.insert(latch);
    let mut markers = vec![None; instructions.len()];
    for (frame, pc) in unit.markers.iter().copied().enumerate() {
        markers[pc] = Some(frame);
        if let Some(next) = unit.frames[frame].alternative { leaders.insert(next); }
    }
    let mut values = smallvec::SmallVec::<[Option<Value>; INLINE_CONSTANT_REGISTERS]>::new();
    values.resize(count, None);
    if known.merge(entry, &values) { pending.push(entry); queued.insert(entry); }
    while let Some(leader) = pending.pop() {
        queued.remove(leader);
        values.clear();
        values.extend_from_slice(known.at(leader).expect("pending constant row"));
        let mut pc = leader;
        loop {
            if let Some(frame) = markers[pc] {
                let frame = unit.frames[frame];
                if let Some(next) = frame.alternative
                    && known.merge(next, &values) && !queued.contains(next) {
                    pending.push(next);
                    queued.insert(next);
                }
                values[usize::from(frame.base)..usize::from(frame.base + frame.registers)].fill(Some(Value::UNDEFINED));
                for (destination, source) in frame.passed_registers(program.function(frame.target.function as usize)) {
                    values[usize::from(destination)] = values[usize::from(source)];
                }
            } else {
                scalar(program, &mut values, instructions[pc]);
            }
            let mut add = |target| {
                if known.merge(target, &values) && !queued.contains(target) { pending.push(target); queued.insert(target); }
            };
            if pc == latch { add(exit); break; }
            let instruction = instructions[pc];
            if instruction.returns_from_frame() { break; }
            match instruction.op().control_flow_layout() {
                ControlFlowLayout::Jump => { add(instruction.jump_target() as usize); break; }
                ControlFlowLayout::ConditionalJump => { add(instruction.jump_target() as usize); add(pc + 1); break; }
                ControlFlowLayout::Fallthrough => {
                    pc += 1;
                    if pc >= instructions.len() { break; }
                    if leaders.contains(pc) { add(pc); break; }
                }
                _ => break,
            }
        }
    }
    known
}

pub(super) fn specialize(program: &ResidualProgram, unit: &Unit, entry: usize) -> Option<Vec<WideInstruction>> {
    let mut residual: Option<Vec<WideInstruction>> = None;
    for (latch, instruction) in unit.instructions.iter().copied().enumerate() {
        if !instruction.op().is_jump() || instruction.result_flags() != 0 { continue; }
        let header = instruction.jump_target() as usize;
        if header >= latch { continue; }
        let instructions = residual.as_deref().unwrap_or(&unit.instructions);
        let condition = instructions[header];
        if !condition.op().is_jump_binary_false() || condition.result_flags() != 0 { continue; }
        let exit = condition.jump_target() as usize;
        if exit <= latch || !instructions[header + 1..latch].iter().copied().all(scalar_body)
            || unit.markers.iter().any(|pc| *pc > header && *pc < latch) { continue; }
        if instructions.iter().enumerate().any(|(pc, instruction)|
            !(header..=latch).contains(&pc)
                && instruction.op().control_flow_layout().jumps()
                && (header + 1..=latch).contains(&(instruction.jump_target() as usize))) { continue; }
        let known = constants(program, unit, instructions, entry, latch, exit);
        if known.at(header).and_then(|values| test(program, values, condition)) != Some(true)
            || known.at(latch).and_then(|values| test(program, values, condition)) != Some(false) { continue; }
        let residual = residual.get_or_insert_with(|| unit.instructions.clone());
        residual[header] = WideInstruction::new(Op::Nop, 0, 0, 0, 0);
        residual[latch] = WideInstruction::new(Op::Jump, 0, 0, 0, exit as u32);
    }
    residual
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::compiled::lane_unit::{self, CalleeBinding, CallTarget};

    fn unit(source: &str) -> (ResidualProgram, Unit) {
        let program = crate::Engine::specialize(source, "static-loop.js").unwrap();
        let caller = program.functions.iter().position(|body| body.header().name
            .is_some_and(|atom| &program.atoms[atom as usize] == "pipe")).unwrap();
        let callee = program.functions.iter().position(|body| body.header().name
            .is_some_and(|atom| &program.atoms[atom as usize] == "step")).unwrap();
        let body = program.function(caller);
        let unit = lane_unit::unit(&program, caller as u32, body, &|owner, pc|
            (owner == caller as u32 && body.instructions().nth(pc as usize).is_some_and(|op| op.op().is_call()))
                .then_some(CallTarget { function: callee as u32, callee: CalleeBinding::Function }).into_iter().collect(),
            &|id| program.function_get(id as usize)).unwrap();
        assert_eq!(unit.frames.len(), 1);
        (program, unit)
    }

    #[test]
    fn one_trip_inline_loop_erases_its_static_test_and_backedge() {
        let (program, unit) = unit("function pipe(x,n){function step(count,value){count=count|0;for(let i=0;i<count;i=(i+1)|0)value=(value^(value<<3))|0;return value;}for(let k=0;k<n;k=(k+1)|0)x=step(1,x);return x;}");
        let residual = specialize(&program, &unit, 0).expect("the constant argument fixes one trip");
        assert_eq!(residual.len(), unit.instructions.len(), "source PC mapping stays unchanged");
        let changed: Vec<_> = residual.iter().zip(&unit.instructions).enumerate()
            .filter(|(_, (after, before))| after.imm() != before.imm()).collect();
        assert_eq!(changed.len(), 2);
        assert_eq!(changed[0].1.0.op(), Op::Nop);
        let (pc, (after, before)) = changed[1];
        assert_eq!(after.op(), Op::Jump);
        assert!((before.jump_target() as usize) < pc && after.jump_target() as usize > pc);
        assert!(matches!(unit.origins[pc], lane_unit::Origin::Inlined { .. }));
    }

    #[test]
    fn changing_or_dynamic_trip_counts_keep_their_backedge() {
        for argument in ["2", "count"] {
            let source = format!("function pipe(x,n,count){{function step(count,value){{for(let i=0;i<count;i=(i+1)|0)value=(value+1)|0;return value;}}for(let k=0;k<n;k=(k+1)|0)x=step({argument},x);return x;}}");
            let (program, unit) = unit(&source);
            assert!(specialize(&program, &unit, 0).is_none(), "{argument} is not one static trip");
        }
        let (program, unit) = unit("function pipe(x,n){function step(count,value){for(let i=0;i<count;i=(i+1)|0)value=(value+1)|0;return value;}let count=1;for(let k=0;k<n;k=(k+1)|0){x=step(count,x);count=2;}return x;}");
        assert!(specialize(&program, &unit, 0).is_none(), "an enclosing backedge changes the argument");
    }

    #[test]
    fn static_number_boundaries_use_shared_semantics() {
        for (bound, one_trip) in [("0.5", true), ("1", true), ("1.5", false), ("-0", false),
            ("0/0", false), ("1/0", false), ("-1", false), ("2147483648", false), ("4294967297|0", true)] {
            let source = format!("function pipe(x,n){{function step(count,value){{for(let i=0;i<count;i=(i+1)|0)value=(value+1)|0;return value;}}for(let k=0;k<n;k=(k+1)|0)x=step({bound},x);return x;}}");
            let (program, unit) = unit(&source);
            assert_eq!(specialize(&program, &unit, 0).is_some(), one_trip, "bound {bound}\n{}", program.disassemble());
        }
    }

    #[test]
    fn polymorphic_call_results_join_before_proving_control() {
        let program = crate::Engine::specialize("function one(){return 1;}function two(){return 2;}function pipe(fn){let count=fn();let value=0;for(let i=0;i<count;i=(i+1)|0)value=(value+1)|0;return value;}", "static-alternatives.js").unwrap();
        let body = program.function(3);
        let unit = lane_unit::unit(&program, 3, body, &|owner, pc| {
            if owner == 3 && body.instructions().nth(pc as usize).is_some_and(|op| op.op().is_call()) {
                [1, 2].map(|function| CallTarget { function, callee: CalleeBinding::Function }).to_vec()
            } else { Vec::new() }
        }, &|id| program.function_get(id as usize)).unwrap();
        assert_eq!(unit.frames.len(), 2);
        assert_eq!(unit.frames[0].alternative, Some(unit.markers[1]));
        assert!(specialize(&program, &unit, 0).is_none(), "both admitted call results contribute to the bound");
    }
}
