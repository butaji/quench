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
mod relations;
mod transfer;

use super::effects::{Ctx, Events};
use super::value::{Key, Word};
use crate::bytecode::{FieldLookup, Operand, Register, WideInstruction};
use crate::staged::{NumberOperation, Operation};
use crate::typed_ir::Length;
use relations::{holds_array, is_string};
use transfer::{Resize, moves_bits, shift_kind, step_of_constant, sum_of_registers};

pub(super) use relations::{Relation, proves_below};
pub(super) use transfer::{advance, refine};

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
            Relation::Sum { .. } | Relation::Less { .. } | Relation::Twice { .. } | Relation::AtLeast { .. }
            | Relation::Loaded { .. } | Relation::Equal { .. } => true,
        });
        if !self.define(ctx, instruction, words) {
            for register in instruction.written_registers() {
                self.kill(register);
            }
        }
        self.read_length(ctx, instruction, words);
    }

    /// Retires the field loads (`Relation::Loaded`) a step may change: a load of `key` from any base, at a write of the name
    /// (`Key::Member`, `MemberOn`, `MemberAny` of `key`), at a write through a key the analysis cannot name, a shape change, or
    /// a step that may run guest code (`calls_js`: a call or an accessor may write any field). `Equal` names registers, not
    /// fields, so a store leaves it.
    fn clobber_loads(&mut self, events: &Events, calls_js: bool) {
        let wild = calls_js || events.wild_member || events.wild_owner || events.shape.is_some();
        let names = |written: &Key, key: u32| matches!(*written, Key::Member(atom) | Key::MemberAny(atom) | Key::MemberOn { atom, .. } if atom == key);
        self.relations.retain(|relation| match *relation {
            Relation::Loaded { key, .. } => !wild && !events.writes.iter().any(|(written, _)| names(written, key)),
            _ => true,
        });
    }

    /// Records the field read `dest = base.key` (`read_length`'s field, any name) where it runs no guest code (`calls_js` false):
    /// `dest` holds `base`'s `key` (`Relation::Loaded`), and every other load of `key` from `base` that holds equals it (`Equal`).
    /// A read into `base` itself holds no load, since the base is gone once it is written.
    fn record_load(&mut self, instruction: WideInstruction, calls_js: bool) {
        if calls_js || instruction.result_flags() != 0 || !matches!(instruction.op().class(), crate::bytecode::Class::GetField) {
            return;
        }
        let Some(FieldLookup::Atom { atom, base, .. }) = instruction.field_lookup() else { return };
        let (dest, Some(base)) = (instruction.result_register(), base.register_index()) else { return };
        if dest == base {
            return;
        }
        let earlier: Vec<Register> = self.relations.iter().filter_map(|relation| match *relation {
            Relation::Loaded { dest: held, base: held_base, key } if held_base == base && key == atom => Some(held),
            _ => None,
        }).collect();
        self.insert(Relation::Loaded { dest, base, key: atom });
        for held in earlier {
            self.insert(Relation::equal(held, dest));
        }
    }
}
