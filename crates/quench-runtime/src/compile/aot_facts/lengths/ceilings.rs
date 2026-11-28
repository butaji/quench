//! Index ranges: what a Number register's value is, as an interval whose ends are the min (floor) or max (ceiling) of up to
//! `MAX_TERMS` linear forms (`Affine`) over the values of registers. A register with no entry holds its own value (`Interval::exact`),
//! which bounds it on both sides, so a retired entry is always sound. The intervals ride the length relations' state (`Lengths`):
//! a join takes the union of the forms (a widening past `MAX_TERMS` is unbounded), a write retires the entries that name the register,
//! and a comparison edge tightens the register's ends (`Lengths::refine_interval`).
//!
//! Rounding: a constant bound is rounded outward (a floor down, a ceiling up), so every form is an integer offset over registers. A
//! truncation (`x | 0`) is the floor's min and the ceiling's max with 0 (`Interval::truncated`): truncation is monotone, and it maps NaN to 0.
//!
//! The lane reads an index range once on the entering edge of a loop, at which every register a form names is unwritten in the loop
//! (`index_ranges`); the analysis never checks it in execution.

use super::super::blocks::Blocks;
use super::super::effects::Ctx;
use super::super::value::Word;
use super::transfer::{is_number_binary, step_of_constant, sum_of_registers};
use super::{CAPACITY, Lengths};
use crate::bytecode::{ControlEdge, Function, Op, Operand, OperandKind, Register, WideInstruction};
use crate::staged::effects::{Affine, Base};
use crate::staged::{NumberOperation, Operation};
use crate::vm::operations::{NumberArithmetic, NumberBitwise, NumberRelation};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

/// The most forms an end of an interval holds. An end past it is unbounded, which is what makes the join's fixpoint finite.
pub(super) const MAX_TERMS: usize = 4;

/// An end of an interval: the min (a floor) or the max (a ceiling) of its forms.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Bound {
    terms: [Option<Affine>; MAX_TERMS],
}

impl Bound {
    /// The end of one form.
    fn of(form: Affine) -> Self {
        let mut terms = [None; MAX_TERMS];
        terms[0] = Some(form);
        Self { terms }
    }

    /// The forms this end takes the min or max of.
    pub fn forms(&self) -> impl Iterator<Item = Affine> + '_ {
        self.terms.iter().flatten().copied()
    }

    /// The end that takes the forms of both ends; None where that has more than `MAX_TERMS` forms.
    fn union(self, other: Self) -> Option<Self> {
        let mut terms = self.terms;
        for form in other.forms() {
            if terms.contains(&Some(form)) {
                continue;
            }
            *terms.iter_mut().find(|slot| slot.is_none())? = Some(form);
        }
        Some(Self { terms })
    }

    /// The end of every sum of a form of `self` and a form of `other`; None where a sum has two bases past `Affine::plus`, or
    /// the sums have more than `MAX_TERMS` distinct forms.
    fn plus(self, other: Self) -> Option<Self> {
        let mut sum: Option<Self> = None;
        for a in self.forms() {
            for b in other.forms() {
                let form = Self::of(a.plus(b)?);
                sum = Some(match sum {
                    Some(bound) => bound.union(form)?,
                    None => form,
                });
            }
        }
        sum
    }

    /// The end shifted by the integer `by`.
    fn shifted(self, by: i64) -> Self {
        Self { terms: self.terms.map(|slot| slot.map(|form| form.shifted(by))) }
    }

    /// This end with each form over a register of `parameters` (`parameter_registers`) rewritten over that parameter's entry value.
    fn rebased(self, parameters: &[(Register, u16)]) -> Option<Self> {
        let mut bound: Option<Self> = None;
        for form in self.forms() {
            let mut mapped = Affine::constant(form.offset);
            for &(base, coefficient) in form.terms.iter().flatten() {
                let base = match base {
                    Base::Register(register) => parameters.iter().find(|&&(held, _)| held == register).map_or(base, |&(_, index)| Base::Parameter(index)),
                    Base::Parameter(_) => base,
                };
                mapped = mapped.plus(Affine { offset: 0, terms: [Some((base, coefficient)), None] })?;
            }
            bound = Some(match bound {
                Some(held) => held.union(Self::of(mapped))?,
                None => Self::of(mapped),
            });
        }
        bound
    }

    /// Whether a form of this end is the value of `register`.
    fn names(&self, register: Register) -> bool {
        self.forms().any(|form| form.terms.iter().flatten().any(|&(base, _)| base == Base::Register(register)))
    }
}

/// What a Number register's value is: where it is not NaN, a value in `[floor, ceiling]` (an end `None` is unbounded). `nan` is
/// whether the value may be NaN (a comparison's fall-through edge keeps it; a comparison that holds excludes it).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Interval {
    pub(super) floor: Option<Bound>,
    pub(super) ceiling: Option<Bound>,
    pub(super) nan: bool,
    /// Whether the value is an integer wherever it is not NaN: a truncation, a Number constant with no fraction, an Int-typed
    /// register (`typed_interval`), or a sum or shift of integral values.
    pub(super) integral: bool,
}

impl Interval {
    /// The interval of a register holding its own value: it bounds itself, and it may be NaN.
    fn exact(register: Register) -> Self {
        let own = Some(Bound::of(Affine::register(register)));
        Self { floor: own, ceiling: own, nan: true, integral: false }
    }

    /// Whether a form of either end is the value of `register`.
    fn names(&self, register: Register) -> bool {
        [self.floor, self.ceiling].iter().flatten().any(|bound| bound.names(register))
    }

    /// The interval of a value that is either one of two (their ends joined; NaN where either may be).
    fn join(self, other: Self) -> Self {
        Self { floor: side_union(self.floor, other.floor), ceiling: side_union(self.ceiling, other.ceiling), nan: self.nan || other.nan, integral: self.integral && other.integral }
    }

    /// The interval shifted by the integer `by`.
    fn shifted(self, by: i64) -> Self {
        Self { floor: self.floor.map(|bound| bound.shifted(by)), ceiling: self.ceiling.map(|bound| bound.shifted(by)), nan: self.nan, integral: self.integral }
    }

    /// The interval of `x | 0`: trunc is monotone, so the floor is at least the min of trunc's floor and 0, and the ceiling at most
    /// the max of its ceiling and 0 (trunc of a value in `[floor, ceiling]` lies in `[min(floor, 0), max(ceiling, 0)]`). NaN maps to 0.
    fn truncated(self) -> Self {
        let zero = Bound::of(Affine::constant(0));
        Self {
            floor: self.floor.and_then(|bound| bound.union(zero)),
            ceiling: self.ceiling.and_then(|bound| bound.union(zero)),
            nan: false,
            integral: true,
        }
    }
}

/// The union of two ends; unbounded where either is.
fn side_union(a: Option<Bound>, b: Option<Bound>) -> Option<Bound> {
    a.zip(b).and_then(|(a, b)| a.union(b))
}

/// The greatest integer at most `value`, as a form: a floor, rounded outward. None where `value` is no finite number.
fn round_down(value: f64) -> Option<Affine> {
    value.is_finite().then(|| Affine::constant(value.floor() as i64))
}

/// The least integer at least `value`, as a form: a ceiling, rounded outward. None where `value` is no finite number.
fn round_up(value: f64) -> Option<Affine> {
    value.is_finite().then(|| Affine::constant(value.ceil() as i64))
}

/// The value of the operand where it is a Number constant of the program.
fn constant_value(ctx: &Ctx<'_>, operand: Operand) -> Option<f64> {
    if matches!(operand.kind(), Some(OperandKind::Constant)) {
        ctx.scope.constant_number(usize::from(operand.payload()))
    } else {
        None
    }
}

/// The relation with its operands swapped: `b < x` is `x > b`.
fn mirrored(relation: NumberRelation) -> Option<NumberRelation> {
    Some(match relation {
        NumberRelation::LessThan => NumberRelation::GreaterThan,
        NumberRelation::LessEqual => NumberRelation::GreaterEqual,
        NumberRelation::GreaterThan => NumberRelation::LessThan,
        NumberRelation::GreaterEqual => NumberRelation::LessEqual,
        _ => return None,
    })
}

impl Lengths {
    /// The interval `register` holds here with its integrality from its type at this point (`words`): an Int-typed register is integral.
    pub(in crate::compile::aot_facts) fn typed_interval(&self, words: &[Word], register: Register) -> Interval {
        let mut interval = self.interval(register);
        interval.integral |= words.get(usize::from(register)).is_some_and(|word| word.cell.ty.int_range().is_some());
        interval
    }

    /// The interval `register` holds at this point: its entry, or its own value.
    pub(in crate::compile::aot_facts) fn interval(&self, register: Register) -> Interval {
        self.bounds.iter().find(|(held, _)| *held == register).map_or_else(|| Interval::exact(register), |(_, interval)| *interval)
    }

    /// Records `interval` for `register`, replacing its entry; the register's own value (the default) is not recorded.
    pub(super) fn set_interval(&mut self, register: Register, interval: Interval) {
        self.bounds.retain(|(held, _)| *held != register);
        if interval != Interval::exact(register) && self.bounds.len() < CAPACITY {
            self.bounds.push((register, interval));
        }
    }

    /// Retires the entries a write of `register` breaks: its own, and every entry that names its value. Retiring is sound, since
    /// a register without an entry holds its own value.
    pub(super) fn retire_bounds(&mut self, register: Register) {
        self.bounds.retain(|(held, interval)| *held != register && !interval.names(register));
    }

    /// The interval of `source` as an operand of a result written to `destination`: its entry, or its own value where the entry names
    /// `destination` (the value `destination` held before the write).
    fn operand_interval(&self, words: &[Word], source: Register, destination: Register) -> Interval {
        let interval = self.typed_interval(words, source);
        if interval.names(destination) { Interval { integral: interval.integral, ..Interval::exact(source) } } else { interval }
    }

    /// The interval an instruction gives its destination, from the state before it: a Number constant (`LoadConst`), a copy of a Number,
    /// a Number shifted by a constant, a sum of two Numbers, or `x | 0` of a Number. None where it gives none (the destination then holds
    /// its own value). `words` are the facts after the instruction, which agree with the state on its operands (they are not its destination).
    pub(super) fn result_interval(&self, ctx: &Ctx<'_>, instruction: WideInstruction, words: &[Word]) -> Option<(Register, Interval)> {
        if instruction.result_flags() != 0 {
            return None;
        }
        let number = |register: Register| words.get(usize::from(register)).is_some_and(|word| word.is_number());
        if instruction.op() == Op::LoadConst {
            let value = ctx.scope.constant_number(instruction.constant_index())?;
            if value.is_nan() {
                return None;
            }
            let interval = Interval {
                floor: round_down(value).map(Bound::of),
                ceiling: round_up(value).map(Bound::of),
                nan: false,
                integral: value.fract() == 0.0,
            };
            return Some((instruction.result_register(), interval));
        }
        if let Some(Operation::Move { destination, source }) = Operation::<u64, ()>::of(instruction) {
            return number(source).then(|| (destination, self.operand_interval(words, source, destination)));
        }
        if let Some((destination, source, delta)) = step_of_constant(ctx, instruction) {
            return (source != destination && number(source)).then(|| (destination, self.operand_interval(words, source, destination).shifted(delta)));
        }
        // A product or difference of two Numbers is integral where both operands are: a double product or difference of integers is
        // an integer or infinite, never fractional. It is NaN where an operand may be (`Infinity * 0`), and its bounds are its own value.
        if [NumberArithmetic::Multiply, NumberArithmetic::Subtract].into_iter().any(|operator| is_number_binary(instruction, operator)) {
            let destination = instruction.result_register();
            // A Number constant is integral where it has no fraction, and never NaN unless it is NaN.
            let operand = |operand: Operand| match operand.register_index() {
                Some(register) => (register != destination && number(register)).then(|| self.typed_interval(words, register)),
                None => constant_value(ctx, operand).map(|value| Interval {
                    nan: value.is_nan(),
                    integral: value.fract() == 0.0,
                    ..Interval::exact(destination)
                }),
            };
            let (a, b) = (operand(instruction.operand_b())?, operand(instruction.operand_c())?);
            return Some((destination, Interval { nan: a.nan || b.nan, integral: a.integral && b.integral, ..Interval::exact(destination) }));
        }
        if let Some((destination, first, second)) = sum_of_registers(instruction) {
            if first == destination || second == destination || !number(first) || !number(second) {
                return None;
            }
            let (a, b) = (self.operand_interval(words, first, destination), self.operand_interval(words, second, destination));
            let floor = a.floor.zip(b.floor).and_then(|(a, b)| a.plus(b));
            let ceiling = a.ceiling.zip(b.ceiling).and_then(|(a, b)| a.plus(b));
            return Some((destination, Interval { floor, ceiling, nan: a.nan || b.nan, integral: a.integral && b.integral }));
        }
        if let Some(Operation::Number(NumberOperation::Bitwise { destination, left, right, operation })) = Operation::<u64, ()>::of(instruction) {
            let truncated = if operation != NumberBitwise::Or {
                None
            } else if constant_value(ctx, right) == Some(0.0) {
                Some(left)
            } else if constant_value(ctx, left) == Some(0.0) {
                Some(right)
            } else {
                None
            };
            if let Some(source) = truncated {
                return source.register_index().filter(|&source| source != destination && number(source))
                    .map(|source| (destination, self.operand_interval(words, source, destination).truncated()));
            }
            // Any other Number bitwise operation yields an integer, never NaN, whose bounds are unknown.
            let numeric = [left, right].into_iter().all(|operand| operand.register_index().is_none_or(number));
            return numeric.then_some((destination, Interval { floor: None, ceiling: None, nan: false, integral: true }));
        }
        None
    }

    /// Refines the intervals on an edge where `left relation right` holds (`holds`) or fails. A Number register compared with a Number
    /// register or a Number constant takes the other side as one end: where the comparison holds, the register is not NaN and sits
    /// on the bound's side; where it fails, it is on the other side or NaN. An end already tighter than the bound is kept.
    pub(super) fn refine_interval(&mut self, ctx: &Ctx<'_>, left: Operand, right: Operand, relation: NumberRelation, holds: bool, words: &[Word]) {
        let number = |register: Register| words.get(usize::from(register)).is_some_and(|word| word.is_number());
        let (register, other, relation) = if let Some(register) = left.register_index() {
            (register, right, relation)
        } else if let (Some(register), Some(relation)) = (right.register_index(), mirrored(relation)) {
            (register, left, relation)
        } else {
            return;
        };
        // Where the comparison holds, the register is at most the bound (`LessThan`, `LessEqual`) or at least it.
        let ceiling_side = match relation {
            NumberRelation::LessThan | NumberRelation::LessEqual => holds,
            NumberRelation::GreaterThan | NumberRelation::GreaterEqual => !holds,
            _ => return,
        };
        if !number(register) {
            return;
        }
        let own = Bound::of(Affine::register(register));
        let end = |upper: bool| -> Option<Bound> {
            match other.register_index() {
                Some(bound) => (bound != register && number(bound)).then(|| Bound::of(Affine::register(bound))),
                None => constant_value(ctx, other)
                    .and_then(|value| if upper { round_up(value) } else { round_down(value) })
                    .map(Bound::of),
            }
        };
        // An end the entry holds earlier (not its own value) is kept: either bound is sound, and two are not one form.
        let tighten = |old: Option<Bound>, candidate: Option<Bound>| match old {
            Some(held) if held != own => Some(held),
            _ => candidate.or(old),
        };
        let mut interval = self.interval(register);
        if ceiling_side {
            interval.ceiling = tighten(interval.ceiling, end(true));
        } else {
            interval.floor = tighten(interval.floor, end(false));
        }
        if holds {
            interval.nan = false;
        }
        self.set_interval(register, interval);
    }

    /// Joins the intervals of `self` and `other` at a point (`Lengths::meet`), by `Interval::join` over the registers either holds
    /// an entry for. Returns whether an entry changed.
    pub(super) fn meet_bounds(&mut self, other: &Lengths) -> bool {
        let sorted = |bounds: &[(Register, Interval)]| {
            let mut bounds = bounds.to_vec();
            bounds.sort_by_key(|&(register, _)| register);
            bounds
        };
        let before = sorted(self.bounds.as_slice());
        let mut registers: Vec<Register> = self.bounds.iter().chain(&other.bounds).map(|&(register, _)| register).collect();
        registers.sort_unstable();
        registers.dedup();
        let joined: Vec<(Register, Interval)> = registers.into_iter()
            .map(|register| (register, self.interval(register).join(other.interval(register))))
            .collect();
        self.bounds.clear();
        for (register, interval) in joined {
            self.set_interval(register, interval);
        }
        sorted(self.bounds.as_slice()) != before
    }
}

/// A loop of a function: its header's pc, the pc ranges of its body's blocks, and the registers its body writes.
pub(super) struct Loop {
    header: u32,
    ranges: Vec<Range<usize>>,
    written: BTreeSet<Register>,
}

/// The loops of `function`. A backedge is a normal edge to a block no later than its source; its loop's body is the blocks that reach
/// the source without passing the header. Backedges sharing a header are one loop, so its body covers all of them.
pub(super) fn loops_of(function: &Function, blocks: &Blocks) -> Vec<Loop> {
    let mut preds: Vec<Vec<usize>> = vec![Vec::new(); blocks.ranges.len()];
    for (block, targets) in blocks.targets.iter().enumerate() {
        for &target in targets {
            preds[target].push(block);
        }
    }
    let mut sources: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (block, (targets, kinds)) in blocks.targets.iter().zip(&blocks.kinds).enumerate() {
        for (&target, kind) in targets.iter().zip(kinds) {
            if matches!(kind, ControlEdge::Normal) && target <= block {
                sources.entry(target).or_default().push(block);
            }
        }
    }
    sources.into_iter().map(|(header, backedges)| {
        let mut body = BTreeSet::from([header]);
        let mut pending = backedges;
        while let Some(block) = pending.pop() {
            if body.insert(block) {
                pending.extend(preds[block].iter().copied());
            }
        }
        let ranges: Vec<Range<usize>> = body.iter().map(|&block| blocks.ranges[block].clone()).collect();
        let written = ranges.iter().flat_map(|range| range.clone())
            .flat_map(|pc| function.instruction_at(pc).written_registers())
            .collect();
        Loop { header: blocks.ranges[header].start as u32, ranges, written }
    }).collect()
}

/// The value of an index as its floor and ceiling, both forms over registers that no write in the loop at `header` (the pc of its
/// header) breaks: the lane evaluates them once on that loop's entering edge and compares them with 0 and the array's length. The
/// floor and ceiling bound the index where it is not NaN, so `nan` says whether the lane must also take the NaN case.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IndexRange {
    pub header: u32,
    pub floor: Bound,
    pub ceiling: Bound,
    pub nan: bool,
    /// The index is an integer wherever it is not NaN (`Interval::integral`). Inside the loop version whose entering edge checked the
    /// range (0 <= floor, ceiling < length, below 2^31), the index is an int32 with no representation guard (known-types law).
    pub integral: bool,
}

/// The index ranges of the reads a settled solve recorded (`intervals`, by pc), each paired with the outermost loop containing it in
/// which its forms are unwritten. A read in no such loop has none.
pub(super) fn index_ranges(intervals: &BTreeMap<u32, Interval>, loops: &[Loop], parameters: &[(Register, u16)]) -> BTreeMap<u32, IndexRange> {
    intervals.iter().filter_map(|(&pc, interval)| index_range(pc, interval, loops, parameters).map(|range| (pc, range))).collect()
}

/// The parameters (`index`, and the register that holds it) no instruction of `function` writes, so each holds its value on entry at
/// every point: a form over such a register is a form over `Base::Parameter(index)`. A function that reads its `arguments` object may
/// write its parameters through it, so it has none.
pub(super) fn parameter_registers(function: &Function) -> Vec<(Register, u16)> {
    let reads_arguments = (0..function.code.len()).any(|pc| function.instruction_at(pc).op().reads_arguments());
    if reads_arguments {
        return Vec::new();
    }
    let written: BTreeSet<Register> = (0..function.code.len()).flat_map(|pc| function.instruction_at(pc).written_registers()).collect();
    (0..function.params).filter_map(|index| {
        function.promoted_register(index).filter(|register| !written.contains(register)).map(|register| (register, index))
    }).collect()
}

/// The index range of the read at `pc` (`index_ranges`). Loops are in header order, so the first invariant one containing `pc` is the
/// outermost. A form over a parameter (its entry value) is not an invariant of a loop here, so such a read has none.
fn index_range(pc: u32, interval: &Interval, loops: &[Loop], parameters: &[(Register, u16)]) -> Option<IndexRange> {
    let (floor, ceiling) = (interval.floor?.rebased(parameters)?, interval.ceiling?.rebased(parameters)?);
    let mut bases = Vec::new();
    for form in floor.forms().chain(ceiling.forms()) {
        for &(base, _) in form.terms.iter().flatten() {
            if let Base::Register(register) = base {
                bases.push(register);
            }
        }
    }
    let inside = |lp: &&Loop| lp.ranges.iter().any(|range| range.contains(&(pc as usize)));
    let lp = loops.iter().filter(inside).find(|lp| bases.iter().all(|base| !lp.written.contains(base)))?;
    Some(IndexRange { header: lp.header, floor, ceiling, nan: interval.nan, integral: interval.integral })
}
