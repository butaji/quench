//! The lifecycle every tier controller walks, written once.
//!
//! A tier's code (a function's native body, a loop site's region, a Wasm function's artifact)
//! is `Cold`, `Warming` toward its first compile, `Queued` for it, `Compiling` on the code
//! generator's thread, a `Speculative` body that assumes what the runs so far showed, or the
//! one permanent `Generic` body that assumes nothing. A body whose assumptions fail too often
//! is replaced by the next one planned without them; after `SPECULATIVE_BODIES` such bodies
//! the next is generic. A function the code generator refuses to build runs `GenericInterpreted`.
//! No state is a dead end that a fact can no longer leave except `Generic`, which is the end by
//! design.
//!
//! Every change of state is `Tier::transition`, which consumes the state and returns its
//! successor with the reason (`Why`) for the diagnostics. The controllers (`JsTier`, `WasmTier`,
//! `LoopSite`, `LaneView`) own a `Tier` and hold no flag or counter about it beside it; what
//! they decide from it is a `match`. The thresholds are `tier_policy`'s.

use super::compiled::tier_policy::{Tiers, SPECULATIVE_BODIES};

/// What one controller plugs into the lifecycle: the code it builds, what its bodies learn,
/// and the rows of the policy table it reads.
pub(super) trait Controller {
    /// The code generator's handle on a compile under way.
    type Pending;
    /// The published code a compile came to.
    type Artifact;
    /// What the bodies so far learned, which the next body plans around (the places a lane body
    /// left at, the operations a loop widened).
    type Feedback: Clone + Default;
    /// How a speculative body's runs went, toward `judge`.
    type Stats: Default;
    /// Where a run of a speculative body left.
    type Leave: Copy;

    /// The entries before a body of this history compiles.
    fn warmup(policy: &Tiers, past: &Past<Self::Feedback>) -> u32;

    /// Whether `entries` (`favourable` of them the kind the body assumes) end the warm-up.
    fn ready(policy: &Tiers, entries: u32, _favourable: u32, past: &Past<Self::Feedback>) -> bool {
        entries >= Self::warmup(policy, past)
    }

    /// What a leave of the body at `at` does to it.
    fn judge(policy: &Tiers, stats: &mut Self::Stats, past: &Past<Self::Feedback>, at: Self::Leave) -> Verdict<Self::Feedback>;

    /// A run of the body completed.
    fn completed(_stats: &mut Self::Stats) {}

    /// The feedback a body that is dropped leaves its successor: the bodies' before it, and
    /// where its own runs left.
    fn learned(_stats: &Self::Stats, feedback: &Self::Feedback) -> Self::Feedback {
        feedback.clone()
    }

    /// The entries a dropped body hands its successor's warm-up: its runs that already paid.
    fn carried(_stats: &Self::Stats) -> u32 {
        0
    }

    /// Whether a function that ran `entries` times interpreted is worth compiling again.
    fn revives(_policy: &Tiers, _entries: u32) -> bool {
        false
    }
}

/// The speculative bodies built so far, and what they learned.
pub(super) struct Past<F> {
    pub(super) bodies: u8,
    /// The compiles the code cache refused: each doubles the next warm-up the owner asks for.
    pub(super) refused: u8,
    pub(super) feedback: F,
}

impl<F: Default> Default for Past<F> {
    fn default() -> Self {
        Self { bodies: 0, refused: 0, feedback: F::default() }
    }
}

impl<F> Past<F> {
    /// Whether the next body is the generic one: the speculative bodies are spent.
    pub(super) fn is_spent(&self) -> bool {
        self.bodies >= SPECULATIVE_BODIES
    }

    /// The history after a body that is dropped, planned from `feedback`.
    fn after(&self, feedback: F) -> Self {
        Self { bodies: self.bodies, refused: self.refused, feedback }
    }

    /// The history whose next body is the generic one.
    fn spent(feedback: F) -> Self {
        Self { bodies: SPECULATIVE_BODIES, refused: 0, feedback }
    }

    /// The history after a compile that came to nothing (a stale one): it counts as a body.
    fn wasted(self) -> Self {
        Self { bodies: self.bodies.saturating_add(1).min(SPECULATIVE_BODIES), ..self }
    }

    /// The history after the code cache refused a compile.
    fn refusal(self) -> Self {
        Self { refused: self.refused.saturating_add(1), ..self }
    }
}

/// What a controller's judgement of a leave decides.
pub(super) enum Verdict<F> {
    /// The body stays; the leave is counted.
    Keep,
    /// Compile the next body from this feedback, which drops what the leaves showed.
    Respecialize(F),
    /// Speculation does not pay for this function: the next body is the generic one.
    Generalize,
}

/// A compile under way: the code generator's handle and the entries to let pass before the next
/// look at it. The one polled handle of every tier.
pub(super) struct Build<P> {
    pending: Box<P>,
    wait: u16,
}

/// What a look at a compile's handle came to.
pub(super) enum Polled<P, A> {
    Ready(A),
    /// A fact the compile took changed while it ran.
    Stale,
    /// The code generator refused to build the code.
    Failed,
    Pending(Box<P>),
}

/// What polling a `Build` came to.
pub(super) enum Looked<P, A> {
    Ready(A),
    Stale,
    Failed,
    Pending(Build<P>),
}

impl<P> Build<P> {
    /// A compile to look at after `wait` entries.
    pub(super) fn new(pending: Box<P>, wait: u16) -> Self {
        Self { pending, wait }
    }

    pub(super) fn pending(&self) -> &P {
        &self.pending
    }

    /// One entry of the owner: counts down to the next look, which `look` makes at the handle
    /// and which waits `interval` entries again where the code is not ready.
    pub(super) fn poll<A>(self, interval: u16, look: impl FnOnce(Box<P>) -> Polled<P, A>) -> Looked<P, A> {
        if self.wait > 0 {
            return Looked::Pending(Self { wait: self.wait - 1, ..self });
        }
        match look(self.pending) {
            Polled::Ready(artifact) => Looked::Ready(artifact),
            Polled::Stale => Looked::Stale,
            Polled::Failed => Looked::Failed,
            Polled::Pending(pending) => Looked::Pending(Self { pending, wait: interval }),
        }
    }
}

/// The lifecycle of one tier's code.
pub(super) enum Tier<C: Controller> {
    /// Never entered.
    Cold,
    /// Counting entries toward the first compile, `favourable` of them the kind its body would
    /// assume.
    Warming { entries: u32, favourable: u32, past: Past<C::Feedback> },
    /// Due to compile at the owner's next chance.
    Queued { past: Past<C::Feedback> },
    Compiling { build: Build<C::Pending>, past: Past<C::Feedback> },
    /// A body that assumes what the runs so far showed, the `past.bodies`th of them.
    Speculative { artifact: C::Artifact, past: Past<C::Feedback>, stats: C::Stats },
    /// The permanent body that assumes nothing.
    Generic { artifact: C::Artifact },
    /// Run by the interpreter: the code generator refused every body, or the function is no
    /// tier's. `entries` counts what it ran since, toward `Controller::revives`.
    GenericInterpreted { entries: u32, refusal: Refusal },
}

/// Why the code generator builds nothing for a function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Refusal {
    /// The function's shape is no tier's (`reason`), which another tier may still take.
    Shape(&'static str),
    /// Its code failed to generate: the same function fails the same way again.
    Codegen,
}

/// Why a transition happened, for the diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Why {
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
pub(super) enum Event<C: Controller> {
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
pub(super) enum Note {
    /// A run of the body completed.
    Completed,
    /// An entry the tier counts for its diagnostics only.
    Observed,
    /// The owner priced the warm-up as over: the body is due.
    Due,
    /// Speculation does not pay: the next body is generic.
    Generalize,
    /// The body's publication was retired from outside.
    Invalidated,
    /// The compile's facts changed.
    Stale,
    /// The code cache refused the body.
    Refused,
}

impl<C: Controller> Event<C> {
    pub(super) const COMPLETED: Self = Self::Note(Note::Completed);
    pub(super) const OBSERVED: Self = Self::Note(Note::Observed);
    pub(super) const DUE: Self = Self::Note(Note::Due);
    pub(super) const GENERALIZE: Self = Self::Note(Note::Generalize);
    pub(super) const INVALIDATED: Self = Self::Note(Note::Invalidated);
    pub(super) const STALE: Self = Self::Note(Note::Stale);
    pub(super) const REFUSED: Self = Self::Note(Note::Refused);
}

impl<C: Controller> Tier<C> {
    /// The published code, speculative or generic.
    pub(super) fn artifact(&self) -> Option<&C::Artifact> {
        match self {
            Self::Speculative { artifact, .. } | Self::Generic { artifact } => Some(artifact),
            _ => None,
        }
    }

    /// Whether code compiles on the code generator's thread.
    pub(super) fn is_compiling(&self) -> bool {
        matches!(self, Self::Compiling { .. })
    }

    /// Counts an entry that leaves the warm-up unfinished: the one check a hot path makes per
    /// entry, a discriminant and an increment. `false` where the entry is the transition's.
    #[inline(always)]
    pub(super) fn count(&mut self, favourable: bool, policy: &Tiers) -> bool {
        let Self::Warming { entries, favourable: kind, past } = self else { return false };
        if C::ready(policy, *entries + 1, *kind + u32::from(favourable), past) {
            return false;
        }
        *entries += 1;
        *kind += u32::from(favourable);
        true
    }

    /// Takes `event`: the successor of this state, and why.
    pub(super) fn transition(self, event: Event<C>, policy: &Tiers) -> (Self, Why) {
        match event {
            Event::Entry { favourable } => self.entered(favourable, policy),
            Event::GuardLeft { at } => self.left(at, policy),
            Event::Started(build) => self.started(build),
            Event::Compiled(artifact) => self.compiled(artifact),
            Event::Failed(refusal) => self.refused_by_host(refusal),
            Event::Note(note) => self.noted(note),
        }
    }

    fn noted(self, note: Note) -> (Self, Why) {
        match note {
            Note::Completed => self.completed(),
            Note::Observed => self.observed(),
            Note::Due => self.due(),
            Note::Generalize => self.generalized(),
            Note::Invalidated => self.invalidated(),
            Note::Stale => self.dropped(Why::Stale, Past::wasted),
            Note::Refused => self.dropped(Why::Refused, Past::refusal),
        }
    }

    /// `transition` in place. The state is moved out and back, so a panic between would drop it
    /// twice: it aborts instead.
    pub(super) fn apply(&mut self, event: Event<C>, policy: &Tiers) -> Why {
        struct AbortOnUnwind;
        impl Drop for AbortOnUnwind {
            fn drop(&mut self) {
                std::process::abort();
            }
        }
        let guard = AbortOnUnwind;
        // SAFETY: `self` is valid; it is read once and written once, and nothing between can
        // unwind without the guard aborting first.
        let (next, why) = unsafe { std::ptr::read(self) }.transition(event, policy);
        unsafe { std::ptr::write(self, next) };
        std::mem::forget(guard);
        why
    }

    /// The history this state carries, whatever it is: a body a new one replaces keeps its.
    fn into_past(self) -> Past<C::Feedback> {
        match self {
            Self::Cold | Self::GenericInterpreted { .. } => Past::default(),
            Self::Generic { .. } => Past::spent(C::Feedback::default()),
            Self::Warming { past, .. } | Self::Queued { past } | Self::Compiling { past, .. } | Self::Speculative { past, .. } => past,
        }
    }

    /// The warm-up's history: the one this state carries, before any body compiled.
    fn warming_past(self) -> Option<Past<C::Feedback>> {
        match self {
            Self::Cold => Some(Past::default()),
            Self::Warming { past, .. } | Self::Queued { past } => Some(past),
            _ => None,
        }
    }

    /// The warm-up after `entries`: due once it is over.
    fn warmed(entries: u32, favourable: u32, past: Past<C::Feedback>, policy: &Tiers) -> (Self, Why) {
        if C::ready(policy, entries, favourable, &past) {
            (Self::Queued { past }, Why::Warmed)
        } else {
            (Self::Warming { entries, favourable, past }, Why::Counted)
        }
    }

    fn entered(self, favourable: bool, policy: &Tiers) -> (Self, Why) {
        let more = u32::from(favourable);
        match self {
            Self::Cold => Self::warmed(1, more, Past::default(), policy),
            Self::Warming { entries, favourable: kind, past } => Self::warmed(entries.saturating_add(1), kind.saturating_add(more), past, policy),
            Self::GenericInterpreted { entries, refusal } => Self::interpreted(entries.saturating_add(1), refusal, policy),
            other => (other, Why::Counted),
        }
    }

    fn observed(self) -> (Self, Why) {
        match self {
            Self::GenericInterpreted { entries, refusal } => (Self::GenericInterpreted { entries: entries.saturating_add(1), refusal }, Why::Counted),
            other => (other, Why::Counted),
        }
    }

    /// An interpreted function counted `entries`: a shape no tier took may be taken by the
    /// generic one once enough of them ran.
    fn interpreted(entries: u32, refusal: Refusal, policy: &Tiers) -> (Self, Why) {
        if matches!(refusal, Refusal::Shape(_)) && C::revives(policy, entries) {
            return (Self::Warming { entries, favourable: 0, past: Past::spent(C::Feedback::default()) }, Why::Revived);
        }
        (Self::GenericInterpreted { entries, refusal }, Why::Counted)
    }

    fn completed(mut self) -> (Self, Why) {
        if let Self::Speculative { stats, .. } = &mut self {
            C::completed(stats);
        }
        (self, Why::Counted)
    }

    /// The warm-up that follows a dropped body: its successor's, carrying what the body earned.
    fn rewarm(entries: u32, past: Past<C::Feedback>, why: Why) -> (Self, Why) {
        (Self::Warming { entries, favourable: 0, past }, why)
    }

    fn left(self, at: C::Leave, policy: &Tiers) -> (Self, Why) {
        let Self::Speculative { artifact, past, mut stats } = self else { return (self, Why::Counted) };
        let entries = C::carried(&stats);
        match C::judge(policy, &mut stats, &past, at) {
            Verdict::Keep => (Self::Speculative { artifact, past, stats }, Why::Counted),
            Verdict::Respecialize(feedback) => Self::rewarm(entries, past.after(feedback), Why::LeftTooOften),
            Verdict::Generalize => Self::rewarm(entries, Past::spent(C::learned(&stats, &past.feedback)), Why::Generalized),
        }
    }

    fn started(self, build: Build<C::Pending>) -> (Self, Why) {
        match self.warming_past() {
            Some(past) => (Self::Compiling { build, past }, Why::Started),
            None => unreachable!("a compile starts only from a state before one: the owner checked"),
        }
    }

    fn compiled(self, artifact: C::Artifact) -> (Self, Why) {
        let past = self.into_past();
        if past.is_spent() {
            return (Self::Generic { artifact }, Why::Published);
        }
        let past = Past { bodies: past.bodies + 1, ..past };
        (Self::Speculative { artifact, past, stats: C::Stats::default() }, Why::Published)
    }

    /// A compile or a body that came to nothing: the function warms again from `past`.
    fn dropped(self, why: Why, past: impl FnOnce(Past<C::Feedback>) -> Past<C::Feedback>) -> (Self, Why) {
        match self {
            Self::Compiling { past: kept, .. } | Self::Queued { past: kept } | Self::Warming { past: kept, .. } =>
                Self::rewarm(0, past(kept), why),
            Self::Cold => Self::rewarm(0, past(Past::default()), why),
            other => (other, Why::Counted),
        }
    }

    fn refused_by_host(self, refusal: Refusal) -> (Self, Why) {
        match self {
            Self::Compiling { .. } | Self::Queued { .. } | Self::Warming { .. } | Self::Cold =>
                (Self::GenericInterpreted { entries: 0, refusal }, Why::Failed),
            other => (other, Why::Counted),
        }
    }

    /// The owner priced the warm-up as over.
    fn due(self) -> (Self, Why) {
        match self {
            Self::Cold => (Self::Queued { past: Past::default() }, Why::Warmed),
            Self::Warming { past, .. } => (Self::Queued { past }, Why::Warmed),
            other => (other, Why::Counted),
        }
    }

    fn generalized(self) -> (Self, Why) {
        match self {
            Self::Cold => Self::rewarm(0, Past::spent(C::Feedback::default()), Why::Generalized),
            Self::Warming { entries, favourable, past } => (Self::Warming { entries, favourable, past: Past::spent(past.feedback) }, Why::Generalized),
            Self::Queued { past } => (Self::Queued { past: Past::spent(past.feedback) }, Why::Generalized),
            Self::Speculative { past, stats, .. } => Self::rewarm(C::carried(&stats), Past::spent(C::learned(&stats, &past.feedback)), Why::Generalized),
            Self::GenericInterpreted { entries, refusal: Refusal::Shape(_) } => Self::rewarm(entries, Past::spent(C::Feedback::default()), Why::Revived),
            other => (other, Why::Counted),
        }
    }

    fn invalidated(self) -> (Self, Why) {
        match self {
            Self::Speculative { past, stats, .. } => Self::rewarm(0, past.after(C::learned(&stats, &past.feedback)), Why::Invalidated),
            Self::Generic { .. } => Self::rewarm(0, Past::spent(C::Feedback::default()), Why::Invalidated),
            compiling @ Self::Compiling { .. } => compiling.dropped(Why::Stale, Past::wasted),
            other => (other, Why::Counted),
        }
    }
}

#[cfg(test)]
mod tests;
