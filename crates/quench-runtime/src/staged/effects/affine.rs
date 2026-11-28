//! The element index of an operation as an affine form of the loop variables: its base, its coefficients and its indices.

use super::*;

/// What a linear form is linear in: the value a parameter holds on entry, or the value a register holds where the form was
/// asserted (a loop-invariant one, `lengths::ceilings`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Base {
    Parameter(u16),
    Register(u16),
}

/// A linear form with an integer constant: `offset + sum(coefficient * base)` over at most two bases. A term's coefficient is
/// nonzero and the terms are sorted by base, so equal forms are equal values.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Affine {
    pub offset: i64,
    pub terms: [Option<(Base, i64)>; 2],
}

impl Affine {
    /// Parameter `index`'s value on entry.
    pub const fn parameter(index: u16) -> Self {
        Self { offset: 0, terms: [Some((Base::Parameter(index), 1)), None] }
    }

    /// Register `index`'s value.
    pub const fn register(index: u16) -> Self {
        Self { offset: 0, terms: [Some((Base::Register(index), 1)), None] }
    }

    /// The constant `offset`.
    pub const fn constant(offset: i64) -> Self {
        Self { offset, terms: [None, None] }
    }

    /// This form plus `by`.
    pub const fn shifted(self, by: i64) -> Self {
        Self { offset: self.offset.saturating_add(by), ..self }
    }

    /// The sum of two forms, where the sum has at most two bases; None beyond.
    pub fn plus(self, other: Self) -> Option<Self> {
        let mut merged: Vec<(Base, i64)> = Vec::with_capacity(4);
        for (base, coefficient) in self.terms.iter().chain(other.terms.iter()).flatten().copied() {
            match merged.iter_mut().find(|(held, _)| *held == base) {
                Some((_, total)) => *total = total.saturating_add(coefficient),
                None => merged.push((base, coefficient)),
            }
        }
        merged.retain(|&(_, coefficient)| coefficient != 0);
        merged.sort();
        if merged.len() > 2 {
            return None;
        }
        let mut terms: [Option<(Base, i64)>; 2] = [None; 2];
        for (slot, term) in terms.iter_mut().zip(merged) {
            *slot = Some(term);
        }
        Some(Self { offset: self.offset.saturating_add(other.offset), terms })
    }

    /// This index as a caller sees it: each parameter `p` names the caller's parameter `parameter(p)` (the argument's
    /// own parameter), or nothing where the argument is no parameter of the caller. None where a parameter names nothing.
    pub(super) fn substitute(self, parameter: &dyn Fn(u16) -> Option<u16>) -> Option<Self> {
        self.terms.iter().flatten().try_fold(Self::constant(self.offset), |sum, &(base, coefficient)| {
            let Base::Parameter(index) = base else { return None };
            let param = parameter(index)?;
            sum.plus(Self { offset: 0, terms: [Some((Base::Parameter(param), coefficient)), None] })
        })
    }
}

/// Which indices of an array an element write touches: any index (`Whole`), or the indices `start .. start + len`
/// with both bounds affine over the function's parameters. A `Span` is a superset of what its write touches, so a
/// store the solve cannot bound is `Whole`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Indices {
    Whole,
    Span { start: Affine, len: Affine },
}

impl Indices {
    /// The indices either names: equal spans stay, any other pair is `Whole`.
    pub fn join(self, other: Self) -> Self {
        if self == other { self } else { Self::Whole }
    }

    /// The indices both name, where a bound is known: `Whole` gives way to the other, and two different spans, which
    /// have no common span, stay `Whole` (a sound superset of both).
    pub fn meet(self, other: Self) -> Self {
        match (self, other) {
            (Self::Whole, rest) | (rest, Self::Whole) => rest,
            _ => self.join(other),
        }
    }

    /// The indices as a caller sees them (`Affine::substitute`): `Whole` where a parameter names no caller parameter.
    pub fn substitute(self, parameter: &dyn Fn(u16) -> Option<u16>) -> Self {
        match self {
            Self::Whole => Self::Whole,
            Self::Span { start, len } => match (start.substitute(parameter), len.substitute(parameter)) {
                (Some(start), Some(len)) => Self::Span { start, len },
                _ => Self::Whole,
            },
        }
    }
}
