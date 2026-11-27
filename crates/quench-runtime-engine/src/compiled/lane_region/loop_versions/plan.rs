//! The expression an element access's index is made of, and the loop whose entry can prove it
//! (`Lowering::resolve`): a counter below a bound, scaled and added to a base, all of them
//! invariant but the counter.

#[allow(unused_imports)] use quench_runtime::staged::{OperationOperationExt};
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::super::*;
use super::{Hoist, Induction, Reach, Term, Version};
use super::super::super::facts::{IndexProof, Point};
use super::super::super::operations::NumberArithmetic;
use super::super::int_ranges::IntRanges;

/// The unit's instructions, the loops of it the region lowers whole, and the operations hoisted out of them.
#[derive(Clone, Copy)]
pub(crate) struct Nest<'a> {
    pub(crate) instructions: &'a [WideInstruction],
    pub(crate) loops: &'a [(usize, usize)],
    pub(crate) hoists: &'a [Hoist],
}

impl Nest<'_> {
    /// The loops that contain `pc`, outermost first.
    pub(crate) fn around(&self, pc: usize) -> Vec<(usize, usize)> {
        let mut around: Vec<(usize, usize)> = self.loops.iter().copied().filter(|&(header, back)| (header..=back).contains(&pc)).collect();
        around.sort_unstable();
        around
    }

    /// Whether no instruction of the loop writes any of `registers`.
    pub(crate) fn unwritten(&self, (header, back): (usize, usize), registers: &[Register]) -> bool {
        self.instructions[header..=back].iter().all(|instruction| !instruction.written_registers().any(|written| registers.contains(&written)))
    }
}

/// Whether the instruction at `at` jumps to `at` or before it: a backward edge of its loop.
fn jumps_back(instruction: WideInstruction, at: usize) -> bool {
    instruction.op().immediate_role() == quench_runtime::bytecode::ImmediateRole::JumpTarget && (instruction.imm() as usize) <= at
}

/// An element access whose index reads as `base + counter * scale`, before the loop that proves it is chosen.
pub(crate) struct Shape {
    pc: usize,
    array: Register,
    /// The addition that forms the index, where the index is a sum.
    add: Option<usize>,
    counter: Register,
    /// The counter's bound, where the index is a counter below one (`None` for a ranged index).
    bound: Option<Register>,
    /// The counter's least value.
    low: i64,
    /// The highest index of a ranged index (`None` for a counter below a bound).
    ceiling: Option<i64>,
    base: Option<Operand>,
    /// The scale and the PC of the product it multiplies the counter in.
    scale: Option<(Operand, usize)>,
    /// The in-place step the index register takes before the access (`Step`), for an index that is no sum.
    step: Option<Step>,
}

/// The largest constant an in-place step adds: `|by|` times an int32 trip count stays exact in `i64`.
const STEP_LIMIT: i64 = 1 << 16;

/// The step an index register takes in its loop: the register, the constant it adds, and whether
/// the access reads a copy of the register made before the step (`copied`) or the register after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Step {
    pub(crate) register: Register,
    pub(crate) by: i64,
    pub(crate) copied: bool,
}

/// An access proved by the entry of a loop: that proof, the addition it bounds the sum of, and the
/// product it makes an induction of.
pub(crate) struct Resolved {
    pub(crate) version: Version,
    pub(crate) add: Option<usize>,
    pub(crate) induction: Option<Induction>,
}

/// The counter an operand of an index's sum counts: its bound and the scale it is multiplied by.
struct Counted {
    operand: Register,
    counter: Register,
    bound: Register,
    scale: Option<(Operand, usize)>,
}

/// The array and index registers of an element access.
fn access_registers(access: WideInstruction) -> Option<(Register, Register)> {
    if matches!(crate::compiled::performed(access), Some(quench_runtime::staged::Operation::ElementRead { .. })) { Some((access.operand_b().register_index()?, access.operand_c().register_index()?)) }
    else if matches!(crate::compiled::performed(access), Some(quench_runtime::staged::Operation::ElementWrite { .. })) { Some((access.register_b(), access.register_c())) } else { None }
}

/// The Number arithmetic `instruction` is, where it is a plain binary one.
pub(crate) fn arithmetic_of(instruction: WideInstruction) -> Option<NumberArithmetic> {
    (quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::NumericBinary) && !instruction.returns_from_frame()
        && instruction.result_flags() == 0)
        .then(|| NumberArithmetic::from_js_instruction(instruction.op(), instruction.binary_operator()))
        .flatten()
}

impl Lowering<'_, '_> {
    /// The expression the index of the access at `pc` is made of, where `proof` leaves it to its loop's
    /// entry (`IndexProof::Versioned`) or it is a sum of a counter below a bound (`counted`).
    pub(crate) fn shape(&self, instructions: &[WideInstruction], pc: usize, point: &Point, proof: IndexProof) -> Option<Shape> {
        let (array, index) = access_registers(instructions[pc])?;
        match proof {
            IndexProof::Proven => None,
            IndexProof::Versioned { bound, .. } => Some(Shape { pc, array, add: None, counter: index, bound: Some(bound),
                low: point.int_range(index)?.min, ceiling: None, base: None, scale: None, step: None }),
            IndexProof::Ranged { max, .. } => Some(Shape { pc, array, add: None, counter: index, bound: None,
                low: 0, ceiling: Some(max), base: None, scale: None, step: None }),
            IndexProof::Check => self.sum_shape(instructions, pc, point, (array, index)).or_else(|| self.step_shape(instructions, pc, (array, index))),
        }
    }

    /// The shape of the access at `pc` whose index register is stepped in its block before the access, or copied
    /// before a step of the copied register (`Step`): its innermost loop's entry proves its reach.
    fn step_shape(&self, instructions: &[WideInstruction], pc: usize, (array, index): (Register, Register)) -> Option<Shape> {
        let step = self.step_of(instructions, pc, index)?;
        Some(Shape { pc, array, add: None, counter: index, bound: None, low: 0, ceiling: None, base: None, scale: None, step: Some(step) })
    }

    /// The shape of the access at `pc` whose index is the sum, just before it in its block, of a base
    /// and a counter or a product of one.
    fn sum_shape(&self, instructions: &[WideInstruction], pc: usize, point: &Point, (array, index): (Register, Register)) -> Option<Shape> {
        let add = self.local_definition(instructions, pc, index)?;
        let sum = instructions[add];
        if arithmetic_of(sum)? != NumberArithmetic::Add { return None; }
        let [left, right] = [sum.operand_b(), sum.operand_c()];
        let (counted, base) = [(left, right), (right, left)].into_iter()
            .find_map(|(operand, base)| self.counted(instructions, add, operand).map(|counted| (counted, base)))?;
        let from = counted.scale.map_or(add, |(_, multiply)| multiply);
        let (base_register, scale_register) = (base.register_index(), counted.scale.and_then(|(scale, _)| scale.register_index()));
        let low = point.int_range(counted.counter)?.min;
        let sound = low >= 0 && counted.counter != index && counted.bound != array && Some(counted.bound) != base_register
            && [base_register, scale_register].into_iter().flatten().all(|register| register != counted.counter && register != counted.operand)
            && !instructions[from + 1..pc].iter().any(|between| between.written_registers().any(|written| written == counted.counter));
        sound.then_some(Shape { pc, array, add: Some(add), counter: counted.counter, bound: Some(counted.bound), low, ceiling: None,
            base: Some(base), scale: counted.scale, step: None })
    }

    /// The counter the sum's `operand` counts: the operand itself below a bound, or a product of one and a scale. A counter's
    /// bound is the solve's at the sum (`Lowering::bound_below`).
    fn counted(&self, instructions: &[WideInstruction], add: usize, operand: Operand) -> Option<Counted> {
        let register = operand.register_index()?;
        if let Some(bound) = self.bound_below(add, register) {
            return Some(Counted { operand: register, counter: register, bound, scale: None });
        }
        let multiply = self.local_definition(instructions, add, register)?;
        let product = instructions[multiply];
        if arithmetic_of(product)? != NumberArithmetic::Multiply { return None; }
        let [left, right] = [product.operand_b(), product.operand_c()];
        [(left, right), (right, left)].into_iter().find_map(|(counted, scale)| {
            let counter = counted.register_index()?;
            Some(Counted { operand: register, counter, bound: self.bound_below(add, counter)?, scale: Some((scale, multiply)) })
        })
    }

    /// The PC of the latest write of `register` before `pc`, within `pc`'s block.
    pub(crate) fn local_definition(&self, instructions: &[WideInstruction], pc: usize, register: Register) -> Option<usize> {
        let mut at = pc;
        while at > 0 && self.blocks[at].is_none() {
            at -= 1;
            if instructions[at].written_registers().any(|written| written == register) { return Some(at); }
        }
        None
    }

    /// The proof of `shape` by the entry of the outermost loop it can be made in, where its array's
    /// view is hoisted a register's.
    pub(crate) fn resolve(&self, (nest, ranges): (Nest<'_>, &IntRanges), shape: &Shape) -> Option<Resolved> {
        let array = self.loop_array(shape.pc, Some(shape.array))?;
        if let Some(step) = shape.step {
            let version = self.resolve_step(nest, array.header, shape.pc, step)?;
            return Some(Resolved { version, add: None, induction: None });
        }
        let base = match (shape.base, shape.add) { (Some(operand), Some(add)) => Some(self.term(nest, add, operand, true)?), _ => None };
        let scale = match shape.scale { Some((operand, multiply)) => Some(self.term(nest, multiply, operand, false)?), None => None };
        let header = self.checking_loop(nest, shape, array, [base, scale])?;
        let induction = shape.scale.zip(scale)
            .and_then(|((_, multiply), scale)| self.induction(nest, ranges, shape.counter, (multiply, scale)));
        let reach = match (shape.ceiling, shape.bound) {
            (Some(ceiling), _) => Reach::Ranged { ceiling },
            (None, Some(bound)) => Reach::Counted { bound, low: shape.low, base, scale },
            (None, None) => return None,
        };
        Some(Resolved { version: Version { pc: shape.pc, header, reach }, add: shape.add, induction })
    }

    /// The term `operand` read at `at` is: a constant, the word of the hoisted operation that last wrote its
    /// register (where `hoisted` admits one), or the register's own.
    fn term(&self, nest: Nest<'_>, at: usize, operand: Operand, hoisted: bool) -> Option<Term> {
        if let Some(value) = self.int32_constant(operand) { return Some(Term::Constant(value)); }
        let register = operand.register_index()?;
        let definition = self.local_definition(nest.instructions, at, register)
            .filter(|definition| nest.hoists.iter().any(|hoist| hoist.pc == *definition));
        match definition {
            Some(pc) => hoisted.then_some(Term::Hoisted(pc)),
            None => Some(Term::Register(register)),
        }
    }

    /// The header of the outermost loop around `shape`'s access, within its array's loop and the loops
    /// that hoisted its terms, that writes no register it reads and holds each of them an int32. Its
    /// entry compares the reach of the access with the array's length (`Lowering::version_loops`); whether
    /// the body may change that length is settled over all versions (`IntRanges::settled_versions`).
    fn checking_loop(&self, nest: Nest<'_>, shape: &Shape, array: LoopArray, terms: [Option<Term>; 2]) -> Option<usize> {
        let terms = terms.into_iter().flatten();
        let registers: Vec<Register> = shape.bound.into_iter().chain(terms.clone().filter_map(Term::register)).collect();
        let floor = terms.filter_map(Term::hoisted).filter_map(|pc| nest.hoists.iter().find(|hoist| hoist.pc == pc))
            .map(|hoist| hoist.header).fold(array.header, usize::max);
        nest.around(shape.pc).into_iter().find(|&span| span.0 >= floor
            && nest.unwritten(span, &registers) && registers.iter().all(|register| self.int32_at(span.0, *register)))
            .map(|(header, _)| header)
    }

    /// The induction of the product at `multiply` of `counter` and `scale`, where `counter` only steps by
    /// one in the innermost loop that writes it and the product is an index word no hoist already holds.
    fn induction(&self, nest: Nest<'_>, ranges: &IntRanges, counter: Register, (multiply, scale): (usize, Term)) -> Option<Induction> {
        let indexed = ranges.index_range(multiply).is_some() && !nest.hoists.iter().any(|hoist| hoist.pc == multiply)
            && self.demand_from(multiply + 1, usize::from(nest.instructions[multiply].result_register())).index_only();
        let span = nest.around(multiply).into_iter().rev().find(|&span| !nest.unwritten(span, &[counter]))?;
        let steps = nest.instructions[span.0..=span.1].iter().filter(|instruction| instruction.written_registers().any(|written| written == counter))
            .all(|instruction| self.steps_by_one(*instruction, counter));
        (indexed && steps && self.int32_at(span.0, counter) && scale.register().is_none_or(|register| nest.unwritten(span, &[register])))
            .then_some(Induction { multiply, counter, scale, span })
    }

    /// Whether `instruction` is `counter++`.
    pub(crate) fn steps_by_one(&self, instruction: WideInstruction, counter: Register) -> bool {
        use quench_runtime::staged::{DecodeContext, NumberOperation, Operation};
        instruction.result_flags() == 0
            && matches!(Operation::<ir::Value, EmitProof>::decode(instruction, DecodeContext::of(self.program)),
                Some(Operation::Number(NumberOperation::IncDec { destination, source, decrement: false, .. }))
                    if destination == counter && source == counter)
    }

    /// The step the index of the access at `pc` is, where its block steps the index itself before the access (`++index`),
    /// or the index is a copy (`Move`) of a register the block steps between the copy and the access (`index = i; i++`).
    pub(crate) fn step_of(&self, instructions: &[WideInstruction], pc: usize, index: Register) -> Option<Step> {
        let definition = self.local_definition(instructions, pc, index)?;
        let defined = instructions[definition];
        if let Some(by) = self.step_by(defined, index) {
            return Some(Step { register: index, by, copied: false });
        }
        if !matches!(crate::compiled::decoded(defined), Some(quench_runtime::staged::Operation::Move { .. })) { return None; }
        let register = defined.register_b();
        let stepped = (definition + 1..pc).find(|&at| instructions[at].written_registers().any(|written| written == register))?;
        let by = self.step_by(instructions[stepped], register)?;
        (register != index).then_some(Step { register, by, copied: true })
    }

    /// The constant `instruction` adds to `register` when it steps it: `register++`, `register--`, `register += c` or
    /// `register -= c` for an int32 constant `c` within `STEP_LIMIT`. None for any other instruction.
    fn step_by(&self, instruction: WideInstruction, register: Register) -> Option<i64> {
        use quench_runtime::staged::{DecodeContext, NumberOperation, Operation};
        if instruction.result_flags() != 0 { return None; }
        if let Some(Operation::Number(NumberOperation::IncDec { destination, source, decrement, .. })) =
            Operation::<ir::Value, EmitProof>::decode(instruction, DecodeContext::of(self.program)) {
            return (destination == register && source == register).then_some(if decrement { -1 } else { 1 });
        }
        let sign = match arithmetic_of(instruction)? {
            NumberArithmetic::Add => 1,
            NumberArithmetic::Subtract => -1,
            _ => return None,
        };
        if instruction.result_register() != register { return None; }
        let (left, right) = (instruction.operand_b(), instruction.operand_c());
        let by = match (left.register_index(), right.register_index()) {
            (Some(held), None) if held == register => i64::from(self.int32_constant(right)?),
            (None, Some(held)) if held == register && sign == 1 => i64::from(self.int32_constant(left)?),
            _ => return None,
        };
        (by.abs() <= STEP_LIMIT).then_some(sign * by)
    }

    /// The version that proves the reach of the stepped access at `pc` by the entry of its innermost loop `(header, back)`,
    /// which must start at or after `floor` (the header of the loop that hoists the access's array). The loop's counter is
    /// the one register the body steps once, at its latch (`steps_by_one`), below a bound the body never writes that the
    /// access sees (`bound_below`); the counter, the bound and the stepped register hold int32s on the entering edge.
    pub(crate) fn resolve_step(&self, nest: Nest<'_>, floor: usize, pc: usize, step: Step) -> Option<Version> {
        let (header, back) = nest.around(pc).last().copied()?;
        if header < floor { return None; }
        let body = &nest.instructions[header..=back];
        let writes = |register: Register| body.iter().filter(|instruction| instruction.written_registers().any(|written| written == register)).count();
        // A backward edge before the latch is a nested loop or a `continue` to the header: either runs the step more than once per trip.
        if writes(step.register) != 1 || (header..back).any(|at| jumps_back(nest.instructions[at], at)) { return None; }
        let counter = (header..=back).find_map(|at| {
            let instruction = nest.instructions[at];
            let counter = instruction.result_register();
            // Stepped once per trip: the step is the counter's only write, and no leader lies between it and the back edge.
            (self.steps_by_one(instruction, counter) && writes(counter) == 1 && (at + 1..=back).all(|later| self.blocks[later].is_none()))
                .then_some(counter)
        })?;
        let bound = self.bound_below(pc, counter)?;
        if bound == counter || bound == step.register || counter == step.register || writes(bound) != 0 { return None; }
        if ![counter, bound, step.register].into_iter().all(|register| self.int32_at(header, register)) { return None; }
        let reach = Reach::Stepped { counter, bound, start: step.register, by: step.by, copied: step.copied };
        Some(Version { pc, header, reach })
    }

    /// Whether the register holds an int32 on entry to the block led by `header`.
    pub(crate) fn int32_at(&self, header: usize, register: Register) -> bool {
        self.facts.at(header).and_then(|types| types.get(usize::from(register))).is_some_and(|ty| ty.is_subtype(Type::INT32))
    }

    /// The int32 `operand` is, where it is a constant one.
    pub(crate) fn int32_constant(&self, operand: Operand) -> Option<i32> {
        let value = super::super::super::facts::constant_number(&self.program.constants, operand)?;
        (value == f64::from(value as i32)).then_some(value as i32)
    }
}
