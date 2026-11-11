//! What happens to a tier, and why a tier moved.

use super::{Build, Controller};

/// Why the code generator builds nothing for a function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::vm) enum Refusal {
    /// The function's shape is no tier's (`reason`), which another tier may still take.
    Shape(&'static str),
    /// Its code failed to generate: the same function fails the same way again.
    Codegen,
}

/// Why a transition happened, for the diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::vm) enum Why {
    /// Nothing changed: an entry or a leave was counted.
    Counted,
    /// The warm-up ended: the body is due.
    Warmed,
    /// The compile started.
    Started,
    /// A compile's code became the body.
    Published,
    /// A fact the compile took changed meanwhile.
    Stale,
    /// The code generator refused the body.
    Failed,
    /// Entering the body does not pay: the owner runs interpreted.
    Unprofitable,
    /// The body's leaves reached their budget: the next body drops what they showed.
    LeftTooOften,
    /// Speculation was given up: the next body is the generic one.
    Generalized,
    /// A lifecycle edge retired the body (a shape compaction, a dead callee).
    Invalidated,
    /// The code cache refused the body: the function warms again.
    Refused,
    /// An interpreted function is worth compiling again.
    Revived,
}

/// What happens to a tier.
pub(in crate::vm) enum Event<C: Controller> {
    /// The owner was entered, or took a backward jump; `favourable` where this entry is the kind
    /// its body would assume.
    Entry { favourable: bool },
    /// A run of the body left at `at`.
    GuardLeft { at: C::Leave },
    /// The code generator took the compile.
    Started(Build<C::Pending>),
    /// A compile's code was published.
    Compiled(C::Artifact),
    /// The code generator refused the body.
    Failed(Refusal),
    /// What happened carries nothing more.
    Note(Note),
}

/// An event of a tier that carries nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::vm) enum Note {
    /// A run of the body completed (the tests' stand-in for `Tier::complete`).
    #[cfg(test)]
    Completed,
    /// An entry the tier counts for its diagnostics only.
    Observed,
    /// The owner priced the warm-up as over: the body is due.
    Due,
    /// Speculation does not pay: the next body is generic.
    Generalize,
    /// The body's publication was retired from outside.
    Invalidated,
    /// The owner weighed the compile and put it off: the entry counts toward the warm-up, and the
    /// owner looks again when it asks (`Controller::ready`).
    Deferred,
    /// The compile's facts changed.
    Stale,
    /// The body's code failed to generate but a generic body may take the function: its entries
    /// price that one from the first.
    Reprice,
    /// The code cache refused the body.
    Refused,
}

impl<C: Controller> Event<C> {
    #[cfg(test)]
    pub(in crate::vm) const COMPLETED: Self = Self::Note(Note::Completed);
    pub(in crate::vm) const OBSERVED: Self = Self::Note(Note::Observed);
    pub(in crate::vm) const DUE: Self = Self::Note(Note::Due);
    pub(in crate::vm) const GENERALIZE: Self = Self::Note(Note::Generalize);
    pub(in crate::vm) const INVALIDATED: Self = Self::Note(Note::Invalidated);
    pub(in crate::vm) const DEFERRED: Self = Self::Note(Note::Deferred);
    pub(in crate::vm) const REPRICE: Self = Self::Note(Note::Reprice);
    pub(in crate::vm) const STALE: Self = Self::Note(Note::Stale);
    pub(in crate::vm) const REFUSED: Self = Self::Note(Note::Refused);
}
