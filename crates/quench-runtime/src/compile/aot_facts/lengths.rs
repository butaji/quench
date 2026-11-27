//! Length relations: what the solve knows of an index against an array's length. An index proven below
//! `a.length` proves the access `a[i]` in bounds, and a loop's test `i < a.length` proves it on the body's
//! edge, so a relation is created on an edge (`refine`), copied by a move (`define`), shifted by an index's
//! step, and retired by a write to its registers. A step that may change an array's length retires the
//! relations to every array but a string, whose length never changes (`advance`).
//!
//! The relations are one fact per register set, held beside the registers' words in each block's state: a
//! join intersects them (`Lengths::meet`), so a relation holds at a point only if every path proves it. The
//! set is finite (its relations name registers and the program's constants), so the solve settles without
//! widening. Element accesses are taken to run no guest code: an accessor defined on an index is not modelled,
//! which is the one assumption this domain makes beyond the words' own types.

use super::effects::{Ctx, Events, runs_guest_code};
use super::value::{Key, Origin, Word};
use crate::bytecode::{FieldLookup, Operand, OperandKind, Register, WideInstruction};
use crate::staged::{Callee, Comparison, Invocation, Operation};
use crate::typed_ir::{Elements, Length, Type};
use crate::vm::operations::{NumberArithmetic, NumberRelation};
use crate::ValueType;

/// A fact about the registers of a function, relating an index to the length of an array.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Relation {
    /// `length` holds the length of `array`, an array or a string (`holds_array`).
    Length { length: Register, array: Register },
    /// `sum` holds `index + offset`, where `index` holds a Number.
    Sum { sum: Register, index: Register, offset: i64 },
    /// `index + reach <= length(array)`: `index + d` is below the length of `array` for every `d < reach`. The strict
    /// `index + k < length` is `reach` `k + 1`, and the at-most `index + k <= length` is `reach` `k`.
    Within { index: Register, array: Register, reach: i64 },
}

impl Relation {
    /// Whether the relation names `register`, as its length, sum, index or array.
    fn names(self, register: Register) -> bool {
        match self {
            Self::Length { length, array } => length == register || array == register,
            Self::Sum { sum, index, .. } => sum == register || index == register,
            Self::Within { index, array, .. } => index == register || array == register,
        }
    }
}

/// The most relations a point holds. A relation past it is not recorded, which only loses facts.
const CAPACITY: usize = 16;

/// The relations a point holds, as a set: a join intersects them, and each instruction advances them (`advance`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Lengths {
    relations: Vec<Relation>,
}

impl Lengths {
    /// The relations the point holds.
    pub(super) fn relations(&self) -> &[Relation] {
        &self.relations
    }

    /// Records `relation`, unless the point holds it already, is at capacity, or proves nothing: an index whose reach
    /// is below one is below no length.
    fn insert(&mut self, relation: Relation) {
        let proves = !matches!(relation, Relation::Within { reach, .. } if reach < 1);
        if proves && self.relations.len() < CAPACITY && !self.relations.contains(&relation) {
            self.relations.push(relation);
        }
    }

    /// `self` met with `other` at a join: a relation the other point does not hold is lost. Whether one was.
    pub(super) fn meet(&mut self, other: &Lengths) -> bool {
        let before = self.relations.len();
        self.relations.retain(|relation| other.relations.contains(relation));
        self.relations.len() != before
    }

    /// Retires every relation that names `register`.
    fn kill(&mut self, register: Register) {
        self.relations.retain(|relation| !relation.names(register));
    }

    /// Shifts `register` by `by`: a bound on its index moves by the same step, so `reach` falls by `by`. Every relation
    /// that names `register` leaves the set, and the shifted bounds are returned for the caller to keep.
    fn rebase(&mut self, register: Register, by: i64) -> Vec<Relation> {
        let mut shifted = Vec::new();
        self.relations.retain(|relation| match *relation {
            Relation::Within { index, array, reach } if index == register => {
                shifted.push(Relation::Within { index, array, reach: reach.saturating_sub(by) });
                false
            }
            other => !other.names(register),
        });
        shifted
    }

    /// The relations a copy of `source` into `destination` carries: those of `source` as an index, a length or a sum, under
    /// `destination`. A relation naming `destination` as its array or index is not copied.
    fn copies(&self, destination: Register, source: Register) -> Vec<Relation> {
        self.relations.iter().filter_map(|relation| match *relation {
            Relation::Within { index, array, reach } if index == source && array != destination =>
                Some(Relation::Within { index: destination, array, reach }),
            Relation::Length { length, array } if length == source && array != destination =>
                Some(Relation::Length { length: destination, array }),
            Relation::Sum { sum, index, offset } if sum == source && index != destination =>
                Some(Relation::Sum { sum: destination, index, offset }),
            _ => None,
        }).collect()
    }

    /// Defines what `instruction` makes of its registers, where it copies one, steps an index in place, or sums an index
    /// and a constant. True where the registers' writes are taken here; false where the caller retires every register the
    /// instruction writes.
    fn define(&mut self, ctx: &Ctx<'_>, instruction: WideInstruction, words: &[Word]) -> bool {
        if instruction.result_flags() != 0 {
            return false;
        }
        if let Some(Operation::Move { destination, source }) = Operation::<u64, ()>::of(instruction) {
            let copies = self.copies(destination, source);
            // A move of `index + k` into the index itself steps the index by `k`: its bounds shift with it.
            let stepped = self.relations.iter().find_map(|relation| match *relation {
                Relation::Sum { sum, index, offset } if sum == source && index == destination => Some(offset),
                _ => None,
            });
            let kept = match stepped {
                Some(by) => self.rebase(destination, by),
                None => {
                    self.kill(destination);
                    Vec::new()
                }
            };
            for relation in kept.into_iter().chain(copies) {
                self.insert(relation);
            }
            return true;
        }
        // `i++` and `i--` in place: the index steps by one.
        if instruction.op() == crate::bytecode::Op::IncDec && instruction.result_register() == instruction.register_b() {
            let by = if instruction.boolean_flag() == Some(true) { -1 } else { 1 };
            for relation in self.rebase(instruction.result_register(), by) {
                self.insert(relation);
            }
            return true;
        }
        if let Some((destination, source, offset)) = add_of_constant(ctx, instruction) {
            if source == destination {
                for relation in self.rebase(destination, offset) {
                    self.insert(relation);
                }
            } else {
                self.kill(destination);
                // `destination = source + offset` where `source` is a Number, which the sum's relation needs.
                if words.get(usize::from(source)).is_some_and(|word| word.is_number()) {
                    self.insert(Relation::Sum { sum: destination, index: source, offset });
                }
            }
            return true;
        }
        false
    }

    /// Records the length a property read `result = base.length` defines, where `base` holds an array or a string (`holds_array`).
    fn read_length(&mut self, ctx: &Ctx<'_>, instruction: WideInstruction, words: &[Word]) {
        if instruction.result_flags() != 0 || !matches!(instruction.op().class(), crate::bytecode::Class::GetField) {
            return;
        }
        let Some(FieldLookup::Atom { atom, base, .. }) = instruction.field_lookup() else { return };
        let (length, Some(array)) = (instruction.result_register(), base.register_index()) else { return };
        if ctx.scope.length == Some(atom) && array != length && words.get(usize::from(array)).is_some_and(holds_array) {
            self.insert(Relation::Length { length, array });
        }
    }

    /// Advances the relations over an instruction that writes `words` as `events` report: a step that may resize an array
    /// retires every relation to a length but a string's, then the registers it writes are defined or retired.
    fn write(&mut self, ctx: &Ctx<'_>, instruction: WideInstruction, words: &[Word], events: &Events, resizes: bool, stored_below: bool) {
        // An element store past the length changes it; a store the relations prove within it does not.
        let resized = resizes || events.wild_elements
            || (!stored_below && events.writes.iter().any(|(key, word)| matches!(key, Key::Element(_)) && word.cell.elements.length != Length::Never));
        if resized {
            self.relations.retain(|relation| match *relation {
                Relation::Within { array, .. } | Relation::Length { array, .. } => words.get(usize::from(array)).is_some_and(is_string),
                Relation::Sum { .. } => true,
            });
        }
        if !self.define(ctx, instruction, words) {
            for register in instruction.written_registers() {
                self.kill(register);
            }
        }
        self.read_length(ctx, instruction, words);
    }
}

/// Whether `relations` prove `index + offset < length(array)`: a relation of `index` past `offset`, or of a register `index`
/// is the sum of, past the sum's own offset as well.
pub(super) fn proves_below(relations: &[Relation], index: Register, array: Register, offset: i64) -> bool {
    let within = |bound: Register, by: i64| relations.iter().any(|relation| matches!(*relation,
        Relation::Within { index: held, array: held_array, reach } if held == bound && held_array == array && reach > by));
    within(index, offset) || relations.iter().any(|relation| match *relation {
        Relation::Sum { sum, index: base, offset: by } if sum == index => within(base, by + offset),
        _ => false,
    })
}

/// The relations of every point of a function, by the pc each holds them before (`flow::Record`): a point that holds none
/// is absent.
#[derive(Default)]
pub(crate) struct Settled {
    points: Vec<(u32, Box<[Relation]>)>,
}

impl Settled {
    /// Records the relations the instruction at `pc` finds before it, where any hold.
    pub(super) fn push(&mut self, pc: u32, relations: &[Relation]) {
        if !relations.is_empty() {
            self.points.push((pc, relations.into()));
        }
    }

    /// Orders the points by pc, so the lookups binary-search them.
    pub(super) fn sort(&mut self) {
        self.points.sort_by_key(|(pc, _)| *pc);
    }

    /// Whether `index + offset < length(array)` holds before the instruction at `pc`.
    pub(crate) fn proves_below(&self, pc: u32, index: Register, array: Register, offset: i64) -> bool {
        self.points.binary_search_by_key(&pc, |(at, _)| *at)
            .is_ok_and(|found| proves_below(&self.points[found].1, index, array, offset))
    }
}

/// Advances `lengths` over `instruction`. `run` steps the register words over it (`effects::step`) and returns its events.
/// The relations are read from the words before the instruction (a call's callee, the bounds an element store meets) and
/// after it (the arrays' kinds, the registers it defines).
pub(super) fn advance(ctx: &Ctx<'_>, instruction: WideInstruction, lengths: &mut Lengths, words: &mut [Word],
    run: impl FnOnce(&mut [Word]) -> Events) -> Events {
    // An element access runs no guest code here (the module note), so only a call's or a construction's guest code resizes.
    let element = instruction.op() == crate::bytecode::Op::GetIndex
        || matches!(Operation::<u64, ()>::of(instruction), Some(Operation::ElementRead { .. } | Operation::ElementWrite { .. }));
    let mutator = Invocation::of(instruction, ctx.decode).is_some_and(|invocation| match invocation.callee {
        Callee::Method { atom, .. } => ctx.scope.is_array_mutator(atom),
        Callee::Register(register) => matches!(words[usize::from(register)].origin,
            Some(Origin::Member(atom) | Origin::MemberOn { atom, .. }) if ctx.scope.is_array_mutator(atom)),
        Callee::Known(_) => false,
    });
    let resizes = mutator || (!element && runs_guest_code(ctx, instruction, words));
    let stored_below = match Operation::<u64, ()>::of(instruction) {
        Some(Operation::ElementWrite { base, key, .. }) => proves_below(lengths.relations(), key, base, 0),
        _ => false,
    };
    let events = run(words);
    lengths.write(ctx, instruction, words, &events, resizes, stored_below);
    events
}

/// The relations the edge of the conditional jump `instruction` carries, from the state `lengths` and `words` the jump
/// leaves. `to_target` is whether the edge is the jump's target. Where the comparison holds on the edge, an index whose
/// value (or sum) is `left < right` (or `<=`, `>`, `>=`) of a register holding an array's length is below that length.
pub(super) fn refine(ctx: &Ctx<'_>, instruction: WideInstruction, to_target: bool, words: &[Word], lengths: &Lengths) -> Lengths {
    let mut refined = lengths.clone();
    let Some(Operation::CompareJump { comparison: Comparison::Relation { relation, .. }, left, right, negate }) = Operation::<u64, ()>::of(instruction)
        else { return refined };
    // The jump is taken where the comparison fails when `negate`, so the target's edge holds it false (`refine_ranges`).
    if to_target == negate {
        return refined;
    }
    // `index + slack <= length`: a strict comparison adds one to the index.
    let (small, big, slack): (Operand, Operand, i64) = match relation {
        NumberRelation::LessThan => (left, right, 1),
        NumberRelation::LessEqual => (left, right, 0),
        NumberRelation::GreaterThan => (right, left, 1),
        NumberRelation::GreaterEqual => (right, left, 0),
        _ => return refined,
    };
    let (Some(small), Some(big)) = (small.register_index(), big.register_index()) else { return refined };
    let Some(array) = lengths.relations.iter().find_map(|held| match *held {
        Relation::Length { length, array } if length == big => Some(array),
        _ => None,
    }) else { return refined };
    // The index is the sum the small operand holds, or the small operand itself where it is a Number.
    let indexed = lengths.relations.iter().find_map(|held| match *held {
        Relation::Sum { sum, index, offset } if sum == small => Some((index, offset)),
        _ => None,
    }).or_else(|| words.get(usize::from(small)).filter(|word| word.is_number()).map(|_| (small, 0)));
    let Some((index, offset)) = indexed else { return refined };
    refined.insert(Relation::Within { index, array, reach: offset + slack });
    refined
}

/// The integer an operand is, where it is a constant of the program that holds one integer (`Scope::constant_type`).
fn constant_of(ctx: &Ctx<'_>, operand: Operand) -> Option<i64> {
    if !matches!(operand.kind(), Some(OperandKind::Constant)) {
        return None;
    }
    let range = ctx.scope.constant_type(usize::from(operand.payload())).int_range()?;
    (range.min == range.max).then_some(range.min)
}

/// The register an addition of a constant defines (`destination`), with the register it adds the constant to and the
/// constant: `source + offset` or `offset + source`. A Number `source` makes a Number sum, which the relations need.
fn add_of_constant(ctx: &Ctx<'_>, instruction: WideInstruction) -> Option<(Register, Register, i64)> {
    if instruction.op() != crate::bytecode::Op::Binary || instruction.result_flags() != 0 {
        return None;
    }
    if !matches!(NumberArithmetic::from_js_instruction(instruction.op(), instruction.binary_operator()), Some(NumberArithmetic::Add)) {
        return None;
    }
    let (left, right) = (instruction.operand_b(), instruction.operand_c());
    let (source, offset) = match (left.register_index(), right.register_index()) {
        (Some(source), None) => (source, constant_of(ctx, right)?),
        (None, Some(source)) => (source, constant_of(ctx, left)?),
        _ => return None,
    };
    Some((instruction.result_register(), source, offset))
}

/// Whether `word` holds a string: its length never changes, so a relation to it survives every write to a length.
fn is_string(word: &Word) -> bool {
    word.cell.ty != Type::NEVER && word.cell.ty.is_subtype(Type::language(ValueType::String))
}

/// Whether `word` holds an array (an element class it names) or a string, whose length a relation may name.
fn holds_array(word: &Word) -> bool {
    word.cell.elements != Elements::NEVER || is_string(word)
}
