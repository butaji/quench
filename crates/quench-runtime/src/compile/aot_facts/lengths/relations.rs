//! The relations a point holds, as data: what each names, how a move, a step or a write changes it (`Relation::shifted`,
//! `renamed`), and the queries the relations answer (`proves_below`, `bound_of`, `complete_arrays`). The point's set is
//! `super::Lengths`; the transfer over an instruction is `super::transfer`.

use super::super::value::Word;
use crate::bytecode::Register;
use crate::typed_ir::{Elements, Type};
use crate::ValueType;

/// A fact about the registers of a function, relating an index to the length of an array.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::compile::aot_facts) enum Relation {
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
    pub(super) fn names(self, register: Register) -> bool {
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
    pub(super) fn shifted(self, register: Register, by: i64) -> Option<Self> {
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
    pub(super) fn renamed(self, from: Register, to: Register) -> Option<Self> {
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


/// Whether `relations` prove `index + offset < length(array)`.
pub(in crate::compile::aot_facts) fn proves_below(relations: &[Relation], index: Register, array: Register, offset: i64) -> bool {
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


/// The arrays whose whole prefix `[0, bound)` is filled through `index` (`Filled { index }`) and whose length is `bound`: a fill
/// loop that ran `index` to `bound` with the array's length as the bound has written every element.
pub(super) fn complete_arrays(relations: &[Relation], index: Register, bound: Register) -> Vec<Register> {
    relations.iter().filter_map(|relation| match *relation {
        Relation::Filled { array, index: filled } if filled == index && relations.contains(&Relation::Length { length: bound, array }) => Some(array),
        _ => None,
    }).collect()
}

/// The array a bound is at most the length of, and how far short of it the bound is: a length register holds the length (no
/// shortfall), and a register with a `Within` (`bound + reach <= length`) is at most `reach` short of it. The smallest shortfall is kept.
pub(super) fn bound_of(relations: &[Relation], bound: Register) -> Option<(Register, i64)> {
    relations.iter().filter_map(|relation| match *relation {
        Relation::Length { length, array } if length == bound => Some((array, 0)),
        Relation::Within { index, array, reach } if index == bound => Some((array, reach)),
        _ => None,
    }).min_by_key(|&(_, reach)| reach)
}



/// Whether `word` holds a string: its length never changes, so a relation to it survives every write to a length.
pub(super) fn is_string(word: &Word) -> bool {
    word.cell.ty != Type::NEVER && word.cell.ty.is_subtype(Type::language(ValueType::String))
}

/// Whether `word` holds an array (an element class it names) or a string, whose length a relation may name.
pub(super) fn holds_array(word: &Word) -> bool {
    word.cell.elements != Elements::NEVER || is_string(word)
}
