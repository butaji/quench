//! Binding-time reduction of finite scalar control flow, shared by every native tier.
//! A loop with a statically true first test and false post-body test has no backedge.
//! Instructions keep their original unit PCs, origins and recovery homes.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::{facts, lane_unit::Unit};
use quench_runtime::{Value, bytecode::{ControlFlowLayout, Operand, OperandKind, ResidualProgram, WideInstruction}};
use quench_runtime::staged::{BinaryClass, Operation};
use quench_runtime::vm::operations::{NumberF64Stage, NumberRelation, PartialOrderStage};

/// Small functions keep transient register facts on the compiler's stack.
const INLINE_CONSTANT_REGISTERS: usize = 16;

/// One constant row: `None` is dynamic.
type Row = smallvec::SmallVec<[Option<Value>; INLINE_CONSTANT_REGISTERS]>;

/// Constant rows at block heads. An unreached head has no row.
struct Constants {
    heads: Vec<usize>,
    rows: Vec<Option<Row>>,
}

impl Constants {
    fn at(&self, pc: usize) -> Option<&[Option<Value>]> {
        let block = self.heads.binary_search(&pc).ok()?;
        self.rows[block].as_deref()
    }
}

/// `constants` as a forward problem of `dataflow::solve`. Blocks start at the region's leaders
/// and at every admission marker, so a marker's alternative edge leaves from the head of its
/// own block carrying the block's input, the row before the frame's entry rewrites it.
struct ConstantFlow<'a> {
    program: &'a ResidualProgram,
    unit: &'a Unit,
    instructions: &'a [WideInstruction],
    registers: usize,
    entry: Option<usize>,
    /// Each block's first instruction, ascending, and one past its last.
    heads: Vec<usize>,
    ends: Vec<usize>,
    /// Each block's successors at `successor_starts[block]..[block + 1]`.
    successors: Vec<usize>,
    successor_starts: Vec<usize>,
    /// The frame a block's head admits, when its head is a marker, and that frame's
    /// alternative block with whether the block also reaches it by its own control.
    frames: Vec<Option<(usize, Option<(usize, bool)>)>>,
    /// The input of the block `transfer` last ran, which a marker's alternative edge carries.
    input: Row,
}

impl quench_runtime::dataflow::Dataflow for ConstantFlow<'_> {
    type State = Row;

    fn blocks(&self) -> usize {
        self.heads.len()
    }

    fn edges(&self, block: usize) -> &[usize] {
        &self.successors[self.successor_starts[block]..self.successor_starts[block + 1]]
    }

    fn entry_state(&self, block: usize) -> Option<Row> {
        (self.entry == Some(block)).then(|| {
            let mut row = Row::new();
            row.resize(self.registers, None);
            row
        })
    }

    fn join(&self, into: &mut Row, incoming: &Row) -> bool {
        let mut changed = false;
        for (known, incoming) in into.iter_mut().zip(incoming) {
            if known.is_some() && known.map(Value::to_bits) != incoming.map(Value::to_bits) {
                *known = None;
                changed = true;
            }
        }
        changed
    }

    fn transfer(&mut self, block: usize, state: &Row) -> Row {
        let mut values = state.clone();
        let (head, end) = (self.heads[block], self.ends[block]);
        let mut from = head;
        if let Some((frame, alternative)) = self.frames[block] {
            if alternative.is_some() { self.input.clone_from(state); }
            let frame = self.unit.frames[frame];
            values[usize::from(frame.base)..usize::from(frame.base + frame.registers)].fill(Some(Value::UNDEFINED));
            for (destination, source) in frame.passed_registers(self.program.function(frame.target.function as usize)) {
                values[usize::from(destination)] = values[usize::from(source)];
            }
            from = head + 1;
        }
        for pc in from..end {
            scalar(self.program, &mut values, self.instructions[pc]);
        }
        values
    }

    fn transfer_edge(&self, block: usize, target: usize, out: &Row) -> Row {
        match self.frames[block] {
            Some((_, Some((alternative, also_reached)))) if alternative == target => {
                let mut row = self.input.clone();
                if also_reached { quench_runtime::dataflow::Dataflow::join(self, &mut row, out); }
                row
            }
            _ => out.clone(),
        }
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
    if !scalar_body(program, instruction) {
        if matches!(Operation::<u64, ()>::of(instruction), Some(Operation::LoadLocalPlain { .. } | Operation::LoadLocalChecked { .. }))
            && instruction.numeric_local_target().is_none() {
            // Reading a guest local defines its result without changing other
            // register homes. Its contents remain dynamic, including TDZ exits.
            for register in super::lane_region::writes(instruction) { values[usize::from(register)] = None; }
        } else if !Operation::<u64, ()>::is_nop(instruction) && !instruction.op().control_flow_layout().jumps() {
            values.fill(None);
        }
        return;
    }
    let decoded = super::decoded(instruction);
    let value = match () {
        _ if quench_runtime::staged::Operation::<(), ()>::decode_constant_load(instruction).is_some() =>
            program.constants.get(instruction.constant_index()).and_then(facts::immediate),
        _ if matches!(decoded, Some(quench_runtime::staged::Operation::Move { .. })) => values.get(usize::from(instruction.register_b())).copied().flatten(),
        _ if matches!(decoded, Some(quench_runtime::staged::Operation::Number(quench_runtime::staged::NumberOperation::IncDec { .. }))) => {
            values.get(usize::from(instruction.register_b())).copied().flatten().and_then(Value::as_number)
                .zip(instruction.boolean_flag()).map(|(value, decrement)| Value::number(if decrement { value - 1.0 } else { value + 1.0 }))
        }
        _ if super::meaning_compiled::is_numeric_binary(instruction) && instruction.numeric_local_target().is_none() => {
            let inputs = operand(program, values, instruction.operand_b()).and_then(Value::as_number)
                .zip(operand(program, values, instruction.operand_c()).and_then(Value::as_number));
            inputs.and_then(|(left, right)| {
                match BinaryClass::of_operator(instruction.binary_operator()) {
                    BinaryClass::Arithmetic(operation) => Some(Value::number(operation.apply(&mut NumberF64Stage, left, right))),
                    BinaryClass::Bitwise(operation) => Some(quench_runtime::vm::operations::number_bitwise_result(operation,
                        quench_runtime::value::number_to_i32(left), quench_runtime::value::number_to_i32(right))),
                    BinaryClass::Relation(operation) => Some(Value::boolean(operation.apply(&mut PartialOrderStage, left, right))),
                    BinaryClass::Remainder | BinaryClass::Other => None,
                }
            })
        }
        _ => None,
    };
    for register in super::lane_region::writes(instruction) {
        values[usize::from(register)] = if register == instruction.result_register() { value } else { None };
    }
}

fn scalar_body(_program: &ResidualProgram, instruction: WideInstruction) -> bool {
    instruction.result_flags() == 0 && instruction.numeric_local_target().is_none()
        && (quench_runtime::staged::Operation::<(), ()>::decode_constant_load(instruction).is_some()
            || matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Move { .. }))
            || matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Number(quench_runtime::staged::NumberOperation::IncDec { .. })))
            || (super::meaning_compiled::is_numeric_binary(instruction)
                && !matches!(BinaryClass::of_operator(instruction.binary_operator()), BinaryClass::Remainder | BinaryClass::Other)))
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
    let length = instructions.len();
    let mut leaders = super::lane_region::leaders(instructions);
    leaders.insert(entry);
    leaders.insert(instructions[latch].jump_target() as usize);
    leaders.insert(latch);
    for frame in &unit.frames {
        if let Some(next) = frame.alternative { leaders.insert(next); }
    }
    for pc in unit.markers.iter().copied() { leaders.insert(pc); }
    let heads: Vec<usize> = leaders.iter().filter(|&pc| pc < length).collect();
    let block_of = |pc: usize| heads.binary_search(&pc).ok();
    let mut markers = vec![None; heads.len()];
    for (frame, pc) in unit.markers.iter().copied().enumerate() {
        if let Some(block) = block_of(pc) { markers[block] = Some(frame); }
    }
    let (mut ends, mut successors, mut successor_starts, mut frames) =
        (Vec::with_capacity(heads.len()), Vec::new(), vec![0], Vec::with_capacity(heads.len()));
    for (block, &head) in heads.iter().enumerate() {
        let mut targets = smallvec::SmallVec::<[usize; 2]>::new();
        let mut pc = head;
        loop {
            if pc == latch { targets.push(exit); break; }
            let instruction = instructions[pc];
            if instruction.returns_from_frame() { break; }
            match instruction.op().control_flow_layout() {
                ControlFlowLayout::Jump => { targets.push(instruction.jump_target() as usize); break; }
                ControlFlowLayout::ConditionalJump => { targets.push(instruction.jump_target() as usize); targets.push(pc + 1); break; }
                ControlFlowLayout::Fallthrough => {
                    if pc + 1 >= length { break; }
                    if block_of(pc + 1).is_some() { targets.push(pc + 1); break; }
                    pc += 1;
                }
                _ => break,
            }
        }
        ends.push(pc + 1);
        let first = successors.len();
        for target in targets.into_iter().filter_map(block_of) {
            if !successors[first..].contains(&target) { successors.push(target); }
        }
        frames.push(markers[block].map(|frame| {
            let alternative = unit.frames[frame].alternative.and_then(block_of).map(|alternative| {
                let also_reached = successors[first..].contains(&alternative);
                if !also_reached { successors.push(alternative); }
                (alternative, also_reached)
            });
            (frame, alternative)
        }));
        successor_starts.push(successors.len());
    }
    let mut flow = ConstantFlow {
        program, unit, instructions, registers: usize::from(unit.registers), entry: block_of(entry),
        heads: Vec::new(), ends, successors, successor_starts, frames, input: Row::new(),
    };
    flow.heads = heads;
    let rows = quench_runtime::dataflow::solve(&mut flow);
    Constants { heads: flow.heads, rows }
}

pub(crate) fn specialize(program: &ResidualProgram, unit: &Unit, entry: usize) -> Option<Vec<WideInstruction>> {
    let mut residual: Option<Vec<WideInstruction>> = None;
    for (latch, instruction) in unit.instructions.iter().copied().enumerate() {
        if !matches!(Operation::<u64, ()>::of(instruction), Some(Operation::Jump)) || instruction.result_flags() != 0 { continue; }
        let header = instruction.jump_target() as usize;
        if header >= latch { continue; }
        let instructions = residual.as_deref().unwrap_or(&unit.instructions);
        let condition = instructions[header];
        if !matches!(Operation::<u64, ()>::of(condition), Some(Operation::CompareJump { .. })) || condition.result_flags() != 0 { continue; }
        let exit = condition.jump_target() as usize;
        if exit <= latch || !instructions[header + 1..latch].iter().copied().all(|instruction| scalar_body(program, instruction))
            || unit.markers.iter().any(|pc| *pc > header && *pc < latch) { continue; }
        if instructions.iter().enumerate().any(|(pc, instruction)|
            !(header..=latch).contains(&pc)
                && instruction.op().control_flow_layout().jumps()
                && (header + 1..=latch).contains(&(instruction.jump_target() as usize))) { continue; }
        let known = constants(program, unit, instructions, entry, latch, exit);
        if known.at(header).and_then(|values| test(program, values, condition)) != Some(true)
            || known.at(latch).and_then(|values| test(program, values, condition)) != Some(false) { continue; }
        let residual = residual.get_or_insert_with(|| unit.instructions.clone());
        residual[header] = super::meaning_compiled::nop_record();
        residual[latch] = super::meaning_compiled::jump_record(exit as u32);
    }
    residual
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiled::lane_unit::{self, CalleeBinding, CallTarget};

    fn unit(source: &str) -> (ResidualProgram, Unit) {
        let program = quench_runtime::Engine::specialize(source, "static-loop.js").unwrap();
        let caller = program.functions.iter().position(|body| body.header().name
            .is_some_and(|atom| &program.atoms[atom as usize] == "pipe")).unwrap();
        let callee = program.functions.iter().position(|body| body.header().name
            .is_some_and(|atom| &program.atoms[atom as usize] == "step")).unwrap();
        let body = program.function(caller);
        let unit = lane_unit::unit(&program, caller as u32, body, &|owner, pc|
            (owner == caller as u32 && body.instructions().nth(pc as usize).is_some_and(|op| Operation::<u64, ()>::is_plain_call(op)))
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
        assert!(Operation::<u64, ()>::is_nop(*changed[0].1.0));
        let (pc, (after, before)) = changed[1];
        assert!(matches!(Operation::<u64, ()>::of(*after), Some(Operation::Jump)));
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
        let program = quench_runtime::Engine::specialize("function one(){return 1;}function two(){return 2;}function pipe(fn){let count=fn();let value=0;for(let i=0;i<count;i=(i+1)|0)value=(value+1)|0;return value;}", "static-alternatives.js").unwrap();
        let body = program.function(3);
        let unit = lane_unit::unit(&program, 3, body, &|owner, pc| {
            if owner == 3 && body.instructions().nth(pc as usize).is_some_and(|op| Operation::<u64, ()>::is_plain_call(op)) {
                [1, 2].map(|function| CallTarget { function, callee: CalleeBinding::Function }).to_vec()
            } else { Vec::new() }
        }, &|id| program.function_get(id as usize)).unwrap();
        assert_eq!(unit.frames.len(), 2);
        assert_eq!(unit.frames[0].alternative, Some(unit.markers[1]));
        assert!(specialize(&program, &unit, 0).is_none(), "both admitted call results contribute to the bound");
    }
}
