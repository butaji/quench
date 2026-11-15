//! Counted loops with straight-line int32 bodies, from the bytecode alone: what a tier may keep
//! as int32 words for the whole loop once one check of its entry facts has passed. A plan names
//! the loop's counter and limit, the units of its body (an int32 sum, a truncation island, a
//! packed field access), the registers that must enter as int32s and the ones the body defines
//! first; it holds no lane record and no handler, so every tier reads the same plan. The
//! interpreter's proved regions are its one user so far (`vm::dispatch_fast::region_state`).

use super::loop_ints::exact_factor;
use crate::bytecode::{Class, FieldBase, Register, StoreReceiver, WideInstruction};
use crate::compile::liveness::{register_dead, RegisterMasks};
use crate::vm::operations::NumberRelation;
use crate::Value;
use oxc_ast::ast::BinaryOperator as Operator;

/// The records from a counted loop's update to its exit: the back edge, then the exit itself.
pub(crate) const EXIT_AFTER_UPDATE: usize = 2;
/// The records of an int32 sum and of a truncation island: the sum and its truncation, and the
/// product before them.
const SUM: usize = 2;
const SCALED_SUM: usize = 3;

/// The counted loops a plan takes: the operator of the test and the direction the update steps.
/// Strict orderings only, whose step cannot overflow where the test holds
/// (`loop_ints::step_is_exact`, which the tests check for each).
pub(crate) const COUNTED: [(Operator, NumberRelation, bool); 2] = [
    (Operator::LessThan, NumberRelation::LessThan, false),
    (Operator::GreaterThan, NumberRelation::GreaterThan, true),
];

/// A loop that steps an int32 counter in place under a test against a limit register it does not
/// change, and whose test leaves right after the back edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CountedLoop {
    pub(crate) header: usize,
    pub(crate) update: usize,
    pub(crate) counter: Register,
    pub(crate) limit: Register,
    pub(crate) decrement: bool,
    pub(crate) relation: NumberRelation,
}

/// What a body unit does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UnitKind {
    /// `(left ± right) | 0` of two int32 registers: two records.
    Sum { subtract: bool },
    /// `(addend ± factor * k) | 0` (`staged::numeric::scaled_sum`): three records.
    ScaledSum { subtract: bool, scaled_first: bool },
    /// `GetField` of a register receiver's packed integer.
    Get,
    /// `SetField` of an int32 register to a register receiver's packed integer.
    Set,
}

/// The field a `Get` or `Set` unit reaches: its receiver register, its field cache site and, for
/// a store, the field's name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FieldAccess {
    pub(crate) receiver: Register,
    pub(crate) site: u16,
    pub(crate) store: Option<u32>,
}

/// One unit of a loop's body: how many instructions it spans, the int32 registers it reads, the
/// int32 register it writes, and the field it reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Unit {
    pub(crate) length: usize,
    pub(crate) kind: UnitKind,
    pub(crate) reads: [Option<Register>; 2],
    pub(crate) write: Option<Register>,
    pub(crate) field: Option<FieldAccess>,
}

/// A unit and where it starts, in instructions from the loop's header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Planned {
    pub(crate) at: usize,
    pub(crate) unit: Unit,
}

/// A counted loop whose body is units, and the entry facts a tier proves once per entry.
#[derive(Debug)]
pub(crate) struct LoopPlan {
    pub(crate) counted: CountedLoop,
    pub(crate) units: Box<[Planned]>,
    /// The registers that must hold int32s at entry: read or carried by the body.
    pub(crate) must: Box<[Register]>,
    /// The registers the body defines before reading and that are dead at the header: an entry
    /// may give them any int32.
    pub(crate) fresh: Box<[Register]>,
}

/// The instruction's result register, where it writes one plainly (no numeric local target).
fn plain_result(instruction: WideInstruction) -> Option<Register> {
    (instruction.result_flags() == 0).then(|| instruction.result_register())
}

/// Whether `instruction` is the binary `operator`.
fn binary_of(instruction: WideInstruction, operator: Operator) -> bool {
    instruction.op().class() == Class::Binary && instruction.binary_operator() == operator as u32
}

/// The int32 `operand` names in `constants`, where it is a constant operand.
fn int_constant(operand: crate::bytecode::Operand, constants: &[Value]) -> Option<i32> {
    (operand.kind() == Some(crate::bytecode::OperandKind::Constant))
        .then(|| constants.get(usize::from(operand.payload()))?.as_int())?
}

/// The counted loop whose update is the instruction at `update`.
pub(crate) fn counted_loop(code: &[WideInstruction], update: usize) -> Option<CountedLoop> {
    let (step, jump) = (*code.get(update)?, *code.get(update + 1)?);
    if step.op().class() != Class::IncDec || jump.op().class() != Class::Jump {
        return None;
    }
    let counter = plain_result(step).filter(|&counter| counter == step.register_b())?;
    let header = jump.jump_target() as usize;
    let test = *code.get(header).filter(|_| header < update)?;
    if test.op().class() != Class::JumpBinaryFalse {
        return None;
    }
    let decrement = step.imm() != 0;
    let (_, relation, _) = COUNTED.into_iter()
        .find(|&(operator, _, direction)| test.binary_operator_field() == operator as u32 && direction == decrement)?;
    let limit = test.operand_c().register_index().filter(|&limit| limit != counter)?;
    let exits = test.jump_target() as usize == update + EXIT_AFTER_UPDATE;
    (exits && test.operand_b().register_index() == Some(counter))
        .then_some(CountedLoop { header, update, counter, limit, decrement, relation })
}

/// `left ± right` of two registers: its register and the sum.
struct RegisterSum {
    subtract: bool,
    left: Register,
    right: Register,
    sum: Register,
}

fn register_sum(instruction: WideInstruction) -> Option<RegisterSum> {
    let subtract = if binary_of(instruction, Operator::Subtraction) { true } else if binary_of(instruction, Operator::Addition) { false } else { return None };
    Some(RegisterSum {
        subtract,
        left: instruction.operand_b().register_index()?,
        right: instruction.operand_c().register_index()?,
        sum: plain_result(instruction)?,
    })
}

/// A register sum and its truncation `sum | 0` right after it, with the truncation's register.
struct TruncatedSum {
    sum: RegisterSum,
    result: Register,
}

fn truncated_sum(code: &[WideInstruction], constants: &[Value], at: usize) -> Option<TruncatedSum> {
    let sum = register_sum(*code.get(at)?)?;
    let truncation = *code.get(at + SUM - 1)?;
    let source = truncation.operand_b().register_index();
    let zero = int_constant(truncation.operand_c(), constants) == Some(0);
    (binary_of(truncation, Operator::BitwiseOR) && zero && source == Some(sum.sum))
        .then(|| plain_result(truncation).map(|result| TruncatedSum { sum, result }))?
}

/// Whether `register` is dead once a unit ends, at `after`: overwritten by the unit's `result`,
/// or not live there.
fn dies(live: Option<&RegisterMasks>, after: usize, register: Register, result: Register) -> bool {
    register == result
        || live.and_then(|masks| masks.get(after)).is_some_and(|set| register_dead(set, usize::from(register)))
}

/// A fused sum: `(left ± right) | 0` whose sum register dies in it.
fn sum_unit(code: &[WideInstruction], constants: &[Value], live: Option<&RegisterMasks>, at: usize) -> Option<Unit> {
    let TruncatedSum { sum, result } = truncated_sum(code, constants, at)?;
    dies(live, at + SUM, sum.sum, result).then_some(Unit {
        length: SUM,
        kind: UnitKind::Sum { subtract: sum.subtract },
        reads: [Some(sum.left), Some(sum.right)],
        write: Some(result),
        field: None,
    })
}

/// A truncation island: `factor * k` into a register that a fused sum takes as one operand,
/// both dying in it, with `k` a factor whose product and sum are exact (`exact_factor`).
fn scaled_sum_unit(code: &[WideInstruction], constants: &[Value], live: Option<&RegisterMasks>, at: usize) -> Option<Unit> {
    let scale = *code.get(at)?;
    int_constant(scale.operand_c(), constants).filter(|&constant| exact_factor(constant))?;
    let (scaled, factor) = (plain_result(scale)?, scale.operand_b().register_index()?);
    if !binary_of(scale, Operator::Multiplication) { return None; }
    let TruncatedSum { sum, result } = truncated_sum(code, constants, at + 1)?;
    let scaled_first = sum.left == scaled;
    let addend = if scaled_first { sum.right } else { sum.left };
    let single = (scaled_first || sum.right == scaled) && addend != scaled;
    let dead = dies(live, at + SCALED_SUM, scaled, result) && dies(live, at + SCALED_SUM, sum.sum, result);
    (single && dead).then_some(Unit {
        length: SCALED_SUM,
        kind: UnitKind::ScaledSum { subtract: sum.subtract, scaled_first },
        reads: [Some(factor), Some(addend)],
        write: Some(result),
        field: None,
    })
}

/// `GetField` or `SetField` of a register receiver, as a unit of one instruction.
fn field_unit(instruction: WideInstruction) -> Option<Unit> {
    let receiver = FieldBase(instruction.b()).register_index()?;
    let access = |store| FieldAccess { receiver, site: instruction.c(), store };
    if instruction.op().class() == Class::GetField {
        return plain_result(instruction).map(|write| Unit {
            length: 1, kind: UnitKind::Get, reads: [None, None], write: Some(write), field: Some(access(None)),
        });
    }
    let register_store = instruction.op().field_store().is_some_and(|store| store.receiver == StoreReceiver::Register);
    register_store.then(|| Unit {
        length: 1, kind: UnitKind::Set, reads: [Some(instruction.a()), None], write: None, field: Some(access(Some(instruction.imm()))),
    })
}

/// The unit that starts at `at`, where the instruction is the start of one.
fn unit(code: &[WideInstruction], constants: &[Value], live: Option<&RegisterMasks>, at: usize) -> Option<Unit> {
    scaled_sum_unit(code, constants, live, at)
        .or_else(|| sum_unit(code, constants, live, at))
        .or_else(|| field_unit(*code.get(at)?))
}

/// The units of `counted`'s body, which hold the whole of it.
fn body(code: &[WideInstruction], constants: &[Value], live: Option<&RegisterMasks>, counted: &CountedLoop) -> Option<Vec<Planned>> {
    let (mut units, mut at) = (Vec::new(), counted.header + 1);
    while at < counted.update {
        let unit = unit(code, constants, live, at)?;
        units.push(Planned { at: at - counted.header, unit });
        at += unit.length;
    }
    (at == counted.update).then_some(units)
}

/// Splits what `units` and the loop touch into the registers that must enter as int32s and the
/// ones the body defines first (dead at the header), each once.
fn entry_registers(live: Option<&RegisterMasks>, counted: &CountedLoop, units: &[Planned]) -> (Vec<Register>, Vec<Register>) {
    let dead_at_header = |register: &Register| {
        live.and_then(|masks| masks.get(counted.header)).is_some_and(|set| register_dead(set, usize::from(*register)))
    };
    let writes = units.iter().filter_map(|planned| planned.unit.write);
    let (mut fresh, carried): (Vec<Register>, Vec<Register>) = writes.partition(dead_at_header);
    let reads = units.iter().flat_map(|planned| planned.unit.reads).flatten();
    let mut must: Vec<Register> = [counted.counter, counted.limit].into_iter().chain(reads).chain(carried)
        .filter(|register| !fresh.contains(register)).collect();
    must.sort_unstable();
    must.dedup();
    fresh.sort_unstable();
    fresh.dedup();
    (must, fresh)
}

/// The plan of the counted loop `counted`, where its body is units that write neither the
/// counter, the limit nor a receiver, and no jump enters it but through its header.
fn plan(
    code: &[WideInstruction], constants: &[Value], live: Option<&RegisterMasks>, targets: &dyn Fn(usize) -> bool, counted: CountedLoop,
) -> Option<LoopPlan> {
    if (counted.header + 1..=counted.update + 1).any(targets) {
        return None;
    }
    let units = body(code, constants, live, &counted)?;
    let writes: Vec<Register> = units.iter().filter_map(|planned| planned.unit.write).collect();
    let receivers = units.iter().filter_map(|planned| planned.unit.field).map(|field| field.receiver);
    let forbidden = receivers.chain([counted.counter, counted.limit]).any(|register| writes.contains(&register));
    let (must, fresh) = entry_registers(live, &counted, &units);
    (!forbidden).then(|| LoopPlan { counted, units: units.into(), must: must.into(), fresh: fresh.into() })
}

/// The plan of each counted loop of `code` whose body is units; `targets` says which
/// instructions a jump reaches, and `entered_past` which loop headers a record before them
/// (fused with the test) can run the loop's body from without reaching them.
pub(crate) fn plan_loops(
    code: &[WideInstruction], constants: &[Value], live: Option<&RegisterMasks>,
    targets: &dyn Fn(usize) -> bool, entered_past: &dyn Fn(usize) -> bool,
) -> Vec<LoopPlan> {
    (0..code.len()).filter_map(|update| counted_loop(code, update))
        .filter(|counted| !entered_past(counted.header))
        .filter_map(|counted| plan(code, constants, live, targets, counted))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typed_ir::loop_ints::step_is_exact;

    /// The table is indexed by direction (`lane_int_loop`) and holds only exact steps.
    #[test]
    fn counted_loops_are_indexed_by_direction_and_step_exactly() {
        for (index, (_, relation, decrement)) in COUNTED.into_iter().enumerate() {
            assert_eq!(usize::from(decrement), index);
            assert!(step_is_exact(relation, decrement));
        }
    }
}
