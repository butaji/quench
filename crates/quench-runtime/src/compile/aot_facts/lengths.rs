//! Length relations: what the solve knows of an index against an array's length. An index proven below
//! `a.length` proves the access `a[i]` in bounds, and a loop's test `i < a.length` proves it on the body's
//! edge, so a relation is created on an edge (`refine`), copied by a move (`define`), shifted by an index's
//! step, and retired by a write to its registers. A step that may resize an array retires the relations to
//! every array but a string, whose length never changes (`advance`, `Resize`).
//!
//! Chains: an index below a register that is itself below the length is below it too (`Relation::Less`,
//! followed by `proves_below`); the midpoint `(lo + hi) >>> 1` of two indices, one below the other, is below
//! the larger (`Relation::Twice`, `halve`). Element accesses run guest code only through an index accessor the
//! world may define, or through the conversion of a key that is no Number or string (`element_runs_guest_code`).
//! A call resizes only the arrays of the element classes its resolved callees may write (`Types::call_writes`).
//!
//! The relations are one fact per register set, held beside the registers' words in each block's state: a
//! join intersects them (`Lengths::meet`), so a relation holds at a point only if every path proves it. The
//! set is finite (its relations name registers and the program's constants), so the solve settles without
//! widening.

use super::effects::{Ctx, Events, runs_guest_code};
use super::value::{Key, Origin, Word};
use super::FunctionFacts;
use crate::bytecode::{FieldLookup, Operand, OperandKind, Register, WideInstruction};
use crate::staged::{Callee, Comparison, Invocation, NumberOperation, Operation};
use crate::typed_ir::{Elements, Length, Type};
use crate::vm::operations::{NumberArithmetic, NumberBitwise, NumberRelation};
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
    /// `index < bound`, both Numbers: `index` is below whatever `bound` is below, one less (`below`).
    Less { index: Register, bound: Register },
    /// `sum < 2 * bound`, where `sum` holds the sum of two Numbers one of which is below the other: its half, a shift right by
    /// a count that moves a bit, is below the larger one (`Lengths::halve`).
    Twice { sum: Register, bound: Register },
    /// `array` holds elements `[0, index)`, all written: a fill loop's index has passed each element before it. Holds
    /// from `i = 0` (any array, the range is empty) until a write to `i` or `array` breaks it.
    Filled { array: Register, index: Register },
    /// `array` holds elements `[0, index]`, all written: the element at `index` was just stored over a `Filled` prefix.
    FilledThrough { array: Register, index: Register },
    /// `index >= bound`, both Numbers: the loop's test failed, so the index has reached the bound.
    AtLeast { index: Register, bound: Register },
    /// `array` holds elements `[0, length)`, all written: a fill loop ran to the length of `array` (`Length`).
    Complete { array: Register },
}

impl Relation {
    /// Whether the relation names `register`, as its length, sum, index, array or bound.
    fn names(self, register: Register) -> bool {
        match self {
            Self::Length { length, array } => length == register || array == register,
            Self::Sum { sum, index, .. } => sum == register || index == register,
            Self::Within { index, array, .. } => index == register || array == register,
            Self::Less { index, bound } => index == register || bound == register,
            Self::Twice { sum, bound } => sum == register || bound == register,
            Self::Filled { array, index } | Self::FilledThrough { array, index } => array == register || index == register,
            Self::AtLeast { index, bound } => index == register || bound == register,
            Self::Complete { array } => array == register,
        }
    }

    /// The relation after `register` moves by `by`, for a relation that names it: what holds of the register's new value.
    /// A bound it is below stays true only when it rises, and a bound above it only when it falls; `None` where a move breaks it.
    fn shifted(self, register: Register, by: i64) -> Option<Self> {
        match self {
            Self::Within { index, array, reach } if index == register => Some(Self::Within { index, array, reach: reach - by }),
            Self::Sum { sum, index, offset } if sum == register => Some(Self::Sum { sum, index, offset: offset + by }),
            Self::Less { index, .. } if index == register && by <= 0 => Some(self),
            Self::Less { bound, .. } if bound == register && by >= 0 => Some(self),
            Self::Twice { sum, .. } if sum == register && by <= 0 => Some(self),
            Self::Twice { bound, .. } if bound == register && by >= 0 => Some(self),
            // The index of `index >= bound` rises, and the bound falls, and the relation holds still.
            Self::AtLeast { index, .. } if index == register && by >= 0 => Some(self),
            Self::AtLeast { bound, .. } if bound == register && by <= 0 => Some(self),
            // A fill prefix through `index` steps by one into the prefix through the new index: `[0, index]` is `[0, index')`.
            Self::FilledThrough { array, index } if index == register && by == 1 => Some(Self::Filled { array, index }),
            _ => None,
        }
    }

    /// The relation with `to` for `from` in every place it names, as a copy of `from` into `to` makes it. A relation that
    /// names `to` already is not carried: a copy would name its destination twice.
    fn renamed(self, from: Register, to: Register) -> Option<Self> {
        if self.names(to) {
            return None;
        }
        let swap = |register: Register| if register == from { to } else { register };
        Some(match self {
            Self::Length { length, array } => Self::Length { length: swap(length), array: swap(array) },
            Self::Sum { sum, index, offset } => Self::Sum { sum: swap(sum), index: swap(index), offset },
            Self::Within { index, array, reach } => Self::Within { index: swap(index), array: swap(array), reach },
            Self::Less { index, bound } => Self::Less { index: swap(index), bound: swap(bound) },
            Self::Twice { sum, bound } => Self::Twice { sum: swap(sum), bound: swap(bound) },
            Self::Filled { array, index } => Self::Filled { array: swap(array), index: swap(index) },
            Self::FilledThrough { array, index } => Self::FilledThrough { array: swap(array), index: swap(index) },
            Self::AtLeast { index, bound } => Self::AtLeast { index: swap(index), bound: swap(bound) },
            Self::Complete { array } => Self::Complete { array: swap(array) },
        })
    }
}

/// The most relations a point holds. A relation past it is not recorded, which only loses facts.
const CAPACITY: usize = 32;

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

    /// Records `relation`, unless the point holds it already, is at capacity, or proves nothing: an index whose reach is
    /// below zero is below no length (a reach of zero is at most the length, which still proves nothing of an access).
    fn insert(&mut self, relation: Relation) {
        let proves = !matches!(relation, Relation::Within { reach, .. } if reach < 0);
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

    /// Records that `index` holds 0: every array the registers hold has an empty prefix `[0, 0)` written (`Filled`).
    fn zeroed(&mut self, words: &[Word], index: Register) {
        for (array, word) in words.iter().enumerate() {
            if array != usize::from(index) && holds_array(word) {
                self.insert(Relation::Filled { array: array as Register, index });
            }
        }
    }

    /// Shifts `register` by `by`: the relations that name it move with it (`Relation::shifted`), and the ones that survive
    /// are returned for the caller to keep.
    fn rebase(&mut self, register: Register, by: i64) -> Vec<Relation> {
        let mut shifted = Vec::new();
        self.relations.retain(|relation| {
            if !relation.names(register) {
                return true;
            }
            shifted.extend(relation.shifted(register, by));
            false
        });
        shifted
    }

    /// The relations a copy of `source` into `destination` carries: every one naming `source`, with `destination` for it
    /// (`Relation::renamed`). A copy of an array carries the lengths and bounds of the array, so both registers hold it.
    fn copies(&self, destination: Register, source: Register) -> Vec<Relation> {
        self.relations.iter().filter(|relation| relation.names(source)).filter_map(|relation| relation.renamed(source, destination)).collect()
    }

    /// Defines what `instruction` makes of its registers, where it copies one, steps an index by a constant, sums two
    /// registers, or shifts a sum. True where the registers' writes are taken here; false where the caller retires every
    /// register the instruction writes.
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
        if let Some((destination, source, delta)) = step_of_constant(ctx, instruction) {
            if source == destination {
                for relation in self.rebase(destination, delta) {
                    self.insert(relation);
                }
            } else {
                self.kill(destination);
                // `destination = source + delta` where `source` is a Number, which the sum's relation needs.
                if words.get(usize::from(source)).is_some_and(|word| word.is_number()) {
                    self.insert(Relation::Sum { sum: destination, index: source, offset: delta });
                }
            }
            return true;
        }
        if let Some((destination, first, second)) = sum_of_registers(instruction) {
            // `destination = first + second` where one is below the other: the sum is below twice the larger. The bound is
            // read before the kill, so a sum written into one of its operands keeps it.
            let bound = [(first, second), (second, first)]
                .into_iter()
                .find(|&(small, big)| self.relations.contains(&Relation::Less { index: small, bound: big }))
                .map(|(_, big)| big)
                .filter(|&big| big != destination);
            self.kill(destination);
            if let Some(bound) = bound {
                self.insert(Relation::Twice { sum: destination, bound });
            }
            return true;
        }
        if let Some(Operation::Number(NumberOperation::Bitwise { destination, left, right, operation })) = Operation::<u64, ()>::of(instruction)
            && let Some(unsigned) = shift_kind(operation)
        {
            self.halve(ctx, words, destination, [left, right], unsigned);
            return true;
        }
        false
    }

    /// `destination = source >> count` (`>>>` where `unsigned`) by a constant count that moves a bit: where `source` is below
    /// twice a bound (`Twice`), the result is below that bound (`Less`). `words` are the facts after the shift, which hold
    /// the source as it was unless the shift overwrites it. An unsigned shift needs a non-negative source, as a negative
    /// one shifts to a large value.
    fn halve(&mut self, ctx: &Ctx<'_>, words: &[Word], destination: Register, [source, count]: [Operand; 2], unsigned: bool) {
        let held = source.register_index();
        let bound = held.and_then(|held| self.relations.iter().find_map(|relation| match *relation {
            Relation::Twice { sum, bound } if sum == held => Some(bound),
            _ => None,
        }));
        let non_negative = held.is_some_and(|held| {
            held != destination && words.get(usize::from(held)).and_then(|word| word.cell.ty.int_range()).is_some_and(|range| range.min >= 0)
        });
        self.kill(destination);
        if let Some(bound) = bound.filter(|&bound| bound != destination && moves_bits(ctx, count) && (!unsigned || non_negative)) {
            self.insert(Relation::Less { index: destination, bound });
        }
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
    /// retires the relations to it (but a string's), then the registers it writes are defined or retired.
    fn write(&mut self, ctx: &Ctx<'_>, instruction: WideInstruction, words: &[Word], events: &Events, resize: Resize<'_, '_>, stored_below: bool) {
        // An element store past the length changes it; a store the relations prove within it does not, and a wild one may
        // change any array's.
        let every = events.wild_elements
            || (!stored_below && events.writes.iter().any(|(key, word)| matches!(key, Key::Element(_)) && word.cell.elements.length != Length::Never));
        self.relations.retain(|relation| match *relation {
            Relation::Within { array, .. } | Relation::Length { array, .. } | Relation::Filled { array, .. }
            | Relation::FilledThrough { array, .. } | Relation::Complete { array } => {
                words.get(usize::from(array)).is_some_and(|word| is_string(word) || !(every || resize.touches(word)))
            }
            Relation::Sum { .. } | Relation::Less { .. } | Relation::Twice { .. } | Relation::AtLeast { .. } => true,
        });
        if !self.define(ctx, instruction, words) {
            for register in instruction.written_registers() {
                self.kill(register);
            }
        }
        self.read_length(ctx, instruction, words);
    }
}

/// What a step may resize: no array (`Nothing`), every array (`Every`: a mutator, or guest code the solve does not follow), or the
/// arrays of the element classes the call at a point may write (`Calls`, `Types::call_writes`).
#[derive(Clone, Copy)]
enum Resize<'a, 'b> {
    Nothing,
    Every,
    /// A call: it resizes the arrays of the classes its settled effects may write (`FunctionFacts::may_write_elements_of`).
    Calls { facts: &'a FunctionFacts<'b>, pc: u32 },
}

impl Resize<'_, '_> {
    /// Whether the step may resize the array `word` holds.
    fn touches(self, word: &Word) -> bool {
        match self {
            Self::Nothing => false,
            Self::Every => true,
            Self::Calls { facts, pc } => facts.may_write_elements_of(pc, word.elements_key()),
        }
    }
}

/// What the step at `pc` may resize, read from the facts before it (`words`): a mutator resizes its receiver's array; an element
/// access resizes through what `element_runs_guest_code` finds; a call resizes what its settled effects may write; any other
/// step that runs guest code resizes every array.
fn resize_of<'a, 'b>(ctx: &Ctx<'_>, facts: &'a FunctionFacts<'b>, pc: usize, instruction: WideInstruction, words: &[Word]) -> Resize<'a, 'b> {
    let mutator = Invocation::of(instruction, ctx.decode).is_some_and(|invocation| match invocation.callee {
        Callee::Method { atom, .. } => ctx.scope.is_array_mutator(atom),
        Callee::Register(register) => matches!(words[usize::from(register)].origin,
            Some(Origin::Member(atom) | Origin::MemberOn { atom, .. }) if ctx.scope.is_array_mutator(atom)),
        Callee::Known(_) => false,
    });
    if mutator {
        Resize::Every
    } else if element_op(instruction) {
        if element_runs_guest_code(facts, instruction, words) { Resize::Every } else { Resize::Nothing }
    } else if Invocation::of(instruction, ctx.decode).is_some() {
        Resize::Calls { facts, pc: pc as u32 }
    } else if runs_guest_code(ctx, instruction, words) {
        Resize::Every
    } else {
        Resize::Nothing
    }
}

/// Whether `instruction` is an element access: a `GetIndex`, or an element read or write.
pub(super) fn element_op(instruction: WideInstruction) -> bool {
    instruction.op() == crate::bytecode::Op::GetIndex
        || matches!(Operation::<u64, ()>::of(instruction), Some(Operation::ElementRead { .. } | Operation::ElementWrite { .. }))
}

/// Whether the element access `instruction` runs guest code on `words`, the facts before it: through an index accessor, which the
/// world may define (`Scope::index_accessors_absent`), or through the conversion of a key that is no Number or string.
pub(super) fn element_runs_guest_code(facts: &FunctionFacts<'_>, instruction: WideInstruction, words: &[Word]) -> bool {
    let plain_key = element_key(instruction).is_some_and(|key| words.get(usize::from(key)).is_some_and(|word| word.is_number() || is_string(word)));
    !(facts.index_accessors_absent() && plain_key)
}

/// The register an element access reads its key from: a `GetIndex`'s third operand, or an element operation's key.
fn element_key(instruction: WideInstruction) -> Option<Register> {
    match Operation::<u64, ()>::of(instruction) {
        Some(Operation::ElementRead { key, .. } | Operation::ElementWrite { key, .. }) => Some(key),
        _ => (instruction.op() == crate::bytecode::Op::GetIndex).then(|| instruction.register_c()),
    }
}

/// Whether `relations` prove `index + offset < length(array)`.
pub(super) fn proves_below(relations: &[Relation], index: Register, array: Register, offset: i64) -> bool {
    below(relations, index, array, offset, relations.len())
}

/// Whether `relations` prove `index + offset < length(array)` through at most `depth` links: a `Within` past `offset`, a length
/// (the index is the length, so it is below it one short of it), a sum's base past its own offset, or a `Less` link to a bound
/// below the length one less. `depth` cuts off a cycle of links, which no consistent set holds.
fn below(relations: &[Relation], index: Register, array: Register, offset: i64, depth: usize) -> bool {
    relations.iter().any(|relation| match *relation {
        Relation::Within { index: held, array: held_array, reach } => held == index && held_array == array && reach > offset,
        Relation::Length { length, array: held_array } => length == index && held_array == array && offset < 0,
        Relation::Sum { sum, index: base, offset: by } => sum == index && depth > 0 && below(relations, base, array, by + offset, depth - 1),
        Relation::Less { index: held, bound } => held == index && depth > 0 && below(relations, bound, array, offset - 1, depth - 1),
        Relation::Twice { .. } | Relation::Filled { .. } | Relation::FilledThrough { .. } | Relation::AtLeast { .. } | Relation::Complete { .. } => false,
    })
}

/// Advances `lengths` over the instruction at `pc`. `run` steps the register words over it (`effects::step`) and returns its events.
/// The relations are read from the words before the instruction (a call's callee, the bounds an element store meets, a resize's
/// keys and receivers) and after it (the arrays' kinds, the registers it defines).
pub(super) fn advance(ctx: &Ctx<'_>, facts: &FunctionFacts<'_>, pc: usize, instruction: WideInstruction, lengths: &mut Lengths,
    words: &mut [Word], run: impl FnOnce(&mut [Word]) -> Events) -> Events {
    let resize = resize_of(ctx, facts, pc, instruction, words);
    let stored_below = match Operation::<u64, ()>::of(instruction) {
        Some(Operation::ElementWrite { base, key, .. }) => proves_below(lengths.relations(), key, base, 0),
        _ => false,
    };
    // An element store over a fill prefix extends it through the stored index (`FilledThrough`).
    let filled_store = match Operation::<u64, ()>::of(instruction) {
        Some(Operation::ElementWrite { base, key, .. }) if lengths.relations.contains(&Relation::Filled { array: base, index: key }) => Some((base, key)),
        _ => None,
    };
    let events = run(words);
    lengths.write(ctx, instruction, words, &events, resize, stored_below);
    if let Some((array, index)) = filled_store {
        lengths.insert(Relation::FilledThrough { array, index });
    }
    // A write that leaves an index at 0 has an empty prefix in every array (`Filled`, `[0, 0)`).
    for register in instruction.written_registers() {
        if words.get(usize::from(register)).is_some_and(|word| word.cell.ty.int_range().is_some_and(|range| range.min == 0 && range.max == 0)) {
            lengths.zeroed(words, register);
        }
    }
    if let Some(length) = allocated_length(ctx, pc, instruction, words) {
        lengths.insert(length);
    }
    events
}

/// The Length relation a `new Array(n)` at `pc` gives its array: `n` holds the array's length. Read from the words after the call;
/// the callee must be the Array constructor, its one argument a Number, and the result the allocation at `pc`.
fn allocated_length(ctx: &Ctx<'_>, pc: usize, instruction: WideInstruction, words: &[Word]) -> Option<Relation> {
    let invocation = Invocation::of(instruction, ctx.decode)?;
    let Callee::Register(callee) = invocation.callee else { return None };
    if !ctx.scope.is_array_constructor(words.get(usize::from(callee))?.origin) { return None; }
    let arguments: Vec<Register> = invocation.arguments.registers(ctx.records).collect();
    let [length] = arguments.as_slice() else { return None };
    let array = instruction.result_register();
    let allocated = words.get(usize::from(array))?.origin == Some(Origin::Alloc { function: ctx.id, pc: pc as u32 });
    (allocated && words.get(usize::from(*length))?.is_number()).then_some(Relation::Length { length: *length, array })
}

/// The relations the edge of the conditional jump `instruction` carries, from the state `lengths` and `words` the jump
/// leaves. `to_target` is whether the edge is the jump's target. Where the comparison holds on the edge, an index whose
/// value (or sum) is `left < right` (or `<=`, `>`, `>=`) of a register holding a bound on an array's length is below that
/// length: by the bound's own relation (`bound_of`), and as a `Less` link to the bound (`proves_below` follows it).
pub(super) fn refine(ctx: &Ctx<'_>, instruction: WideInstruction, to_target: bool, words: &[Word], lengths: &Lengths) -> Lengths {
    let mut refined = lengths.clone();
    let Some(Operation::CompareJump { comparison: Comparison::Relation { relation, .. }, left, right, negate }) = Operation::<u64, ()>::of(instruction)
        else { return refined };
    // The comparison holds on this edge when the jump's edge is not the one `negate` takes it on. A failing `<` proves
    // `left >= right`, and a holding `>=` does too: the loop's exit edge of `index < bound` reaches the bound.
    let holds = to_target != negate;
    if let (Some(index), Some(bound)) = (left.register_index(), right.register_index()) {
        let at_least = match relation {
            NumberRelation::LessThan => !holds,
            NumberRelation::GreaterEqual => holds,
            _ => false,
        };
        if at_least && words.get(usize::from(index)).is_some_and(|word| word.is_number()) && words.get(usize::from(bound)).is_some_and(|word| word.is_number()) {
            refined.insert(Relation::AtLeast { index, bound });
            for array in complete_arrays(&refined.relations, index, bound) {
                refined.insert(Relation::Complete { array });
            }
        }
    }
    if !holds {
        return refined;
    }
    // `index + slack <= bound`: a strict comparison adds one to the index.
    let (small, big, slack): (Operand, Operand, i64) = match relation {
        NumberRelation::LessThan => (left, right, 1),
        NumberRelation::LessEqual => (left, right, 0),
        NumberRelation::GreaterThan => (right, left, 1),
        NumberRelation::GreaterEqual => (right, left, 0),
        _ => return refined,
    };
    let (Some(small), Some(big)) = (small.register_index(), big.register_index()) else { return refined };
    // The index is the sum the small operand holds, or the small operand itself where it is a Number.
    let indexed = lengths.relations.iter().find_map(|held| match *held {
        Relation::Sum { sum, index, offset } if sum == small => Some((index, offset)),
        _ => None,
    }).or_else(|| words.get(usize::from(small)).filter(|word| word.is_number()).map(|_| (small, 0)));
    let Some((index, offset)) = indexed else { return refined };
    if let Some((array, reach)) = bound_of(&lengths.relations, big) {
        refined.insert(Relation::Within { index, array, reach: offset + slack + reach });
    }
    // `index < bound` between Numbers, where the bound is named by a relation: `proves_below` follows it through the bound's own.
    let named = lengths.relations.iter().any(|held| held.names(big));
    if slack == 1 && offset == 0 && named && words.get(usize::from(big)).is_some_and(|word| word.is_number()) {
        refined.insert(Relation::Less { index, bound: big });
    }
    refined
}

/// The arrays whose whole prefix `[0, bound)` is filled through `index` (`Filled { index }`) and whose length is `bound`: a fill
/// loop that ran `index` to `bound` with the array's length as the bound has written every element.
fn complete_arrays(relations: &[Relation], index: Register, bound: Register) -> Vec<Register> {
    relations.iter().filter_map(|relation| match *relation {
        Relation::Filled { array, index: filled } if filled == index && relations.contains(&Relation::Length { length: bound, array }) => Some(array),
        _ => None,
    }).collect()
}

/// The array a bound is at most the length of, and how far short of it the bound is: a length register holds the length (no
/// shortfall), and a register with a `Within` (`bound + reach <= length`) is at most `reach` short of it. The smallest shortfall is kept.
fn bound_of(relations: &[Relation], bound: Register) -> Option<(Register, i64)> {
    relations.iter().filter_map(|relation| match *relation {
        Relation::Length { length, array } if length == bound => Some((array, 0)),
        Relation::Within { index, array, reach } if index == bound => Some((array, reach)),
        _ => None,
    }).min_by_key(|&(_, reach)| reach)
}

/// The integer an operand is, where it is a constant of the program that holds one integer (`Scope::constant_type`).
fn constant_of(ctx: &Ctx<'_>, operand: Operand) -> Option<i64> {
    if !matches!(operand.kind(), Some(OperandKind::Constant)) {
        return None;
    }
    let range = ctx.scope.constant_type(usize::from(operand.payload())).int_range()?;
    (range.min == range.max).then_some(range.min)
}

/// Whether `instruction` is a Number binary operation of `operation` with no flags.
fn is_number_binary(instruction: WideInstruction, operation: NumberArithmetic) -> bool {
    instruction.op() == crate::bytecode::Op::Binary
        && instruction.result_flags() == 0
        && NumberArithmetic::from_js_instruction(instruction.op(), instruction.binary_operator()) == Some(operation)
}

/// The step a constant addition or subtraction makes of a register, as `(destination, source, delta)` where
/// `destination = source + delta`: `source + k`, `k + source` or `source - k`. A `k - source` steps no register by a constant.
fn step_of_constant(ctx: &Ctx<'_>, instruction: WideInstruction) -> Option<(Register, Register, i64)> {
    let (left, right) = (instruction.operand_b(), instruction.operand_c());
    let (source, delta) = if is_number_binary(instruction, NumberArithmetic::Add) {
        match (left.register_index(), right.register_index()) {
            (Some(source), None) => (source, constant_of(ctx, right)?),
            (None, Some(source)) => (source, constant_of(ctx, left)?),
            _ => return None,
        }
    } else if is_number_binary(instruction, NumberArithmetic::Subtract) {
        (left.register_index()?, constant_of(ctx, right)?.checked_neg()?)
    } else {
        return None;
    };
    Some((instruction.result_register(), source, delta))
}

/// The registers of `destination = first + second`, a Number addition of two registers.
fn sum_of_registers(instruction: WideInstruction) -> Option<(Register, Register, Register)> {
    if !is_number_binary(instruction, NumberArithmetic::Add) {
        return None;
    }
    Some((instruction.result_register(), instruction.operand_b().register_index()?, instruction.operand_c().register_index()?))
}

/// Whether a constant shift count moves a bit: its low bits (`WORD32_SHIFT_MASK`) are not all zero.
fn moves_bits(ctx: &Ctx<'_>, count: Operand) -> bool {
    constant_of(ctx, count).is_some_and(|count| ((count as u32) & crate::value::WORD32_SHIFT_MASK) != 0)
}

/// Whether a shift is zero-filling (`>>>`, `true`) or sign-propagating (`>>`, `false`); `None` for any other operation.
fn shift_kind(operation: NumberBitwise) -> Option<bool> {
    match operation {
        NumberBitwise::ShiftRight => Some(false),
        NumberBitwise::ShiftRightUnsigned => Some(true),
        _ => None,
    }
}

/// Whether `word` holds a string: its length never changes, so a relation to it survives every write to a length.
fn is_string(word: &Word) -> bool {
    word.cell.ty != Type::NEVER && word.cell.ty.is_subtype(Type::language(ValueType::String))
}

/// Whether `word` holds an array (an element class it names) or a string, whose length a relation may name.
fn holds_array(word: &Word) -> bool {
    word.cell.elements != Elements::NEVER || is_string(word)
}
