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

mod build;
mod event;

pub(super) use build::{Build, Looked, Polled};
pub(super) use event::{Event, Note, Refusal, Why};

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

/// Why a tier whose entries keep being declined runs interpreted.
const DECLINED: &str = "entries-declined";

/// The speculative bodies built so far, and what they learned.
pub(super) struct Past<F> {
    pub(super) bodies: u8,
    /// The compiles the code cache refused: each doubles the next warm-up the owner asks for.
    pub(super) refused: u8,
    /// The entries in a row the owner declined to run for want of a body that fits them.
    pub(super) misses: u32,
    pub(super) feedback: F,
}

impl<F: Default> Default for Past<F> {
    fn default() -> Self {
        Self { bodies: 0, refused: 0, misses: 0, feedback: F::default() }
    }
}

impl<F> Past<F> {
    /// Whether the next body is the generic one: the speculative bodies are spent.
    pub(super) fn is_spent(&self) -> bool {
        self.bodies >= SPECULATIVE_BODIES
    }

    /// The history after a body that is dropped, planned from `feedback`.
    fn after(&self, feedback: F) -> Self {
        Self { bodies: self.bodies, refused: self.refused, misses: self.misses, feedback }
    }

    /// The history whose next body is the generic one.
    fn spent(feedback: F) -> Self {
        Self { bodies: SPECULATIVE_BODIES, refused: 0, misses: 0, feedback }
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
    /// No body pays for its entries: the owner runs interpreted from now on.
    Interpret(Refusal),
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
    /// The permanent body that assumes nothing; `past` is the spent history that led to it.
    Generic { artifact: C::Artifact, past: Past<C::Feedback> },
    /// Run by the interpreter: the code generator refused every body, or the function is no
    /// tier's. `entries` counts what it ran since, toward `Controller::revives`.
    GenericInterpreted { entries: u32, refusal: Refusal },
}


impl<C: Controller> Default for Tier<C> {
    fn default() -> Self {
        Self::Cold
    }
}

impl<C: Controller> Tier<C> {
    /// The published code, speculative or generic.
    pub(super) fn artifact(&self) -> Option<&C::Artifact> {
        match self {
            Self::Speculative { artifact, .. } | Self::Generic { artifact, .. } => Some(artifact),
            _ => None,
        }
    }

    /// The state's name, for the diagnostics.
    pub(super) fn stage(&self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::Warming { .. } => "warming",
            Self::Queued { .. } => "queued",
            Self::Compiling { .. } => "compiling",
            Self::Speculative { .. } => "speculative",
            Self::Generic { .. } => "generic",
            Self::GenericInterpreted { .. } => "interpreted",
        }
    }

    /// Whether the owner runs interpreted: no body fits it.
    pub(super) fn is_interpreted(&self) -> bool {
        matches!(self, Self::GenericInterpreted { .. })
    }

    /// Whether code compiles on the code generator's thread.
    pub(super) fn is_compiling(&self) -> bool {
        matches!(self, Self::Compiling { .. })
    }

    /// Counts an entry that leaves the warm-up unfinished: the one check a hot path makes per
    /// entry, a discriminant and an increment. `false` where the entry is the transition's.
    #[inline(always)]
    pub(super) fn count(&mut self, favourable: bool, policy: &Tiers) -> bool {
        let Self::Warming { entries, favourable: kind, past } = self else { return self.count_first(favourable, policy) };
        if C::ready(policy, *entries + 1, *kind + u32::from(favourable), past) {
            return false;
        }
        *entries += 1;
        *kind += u32::from(favourable);
        true
    }

    /// `count` of a cold tier: the first entry starts its warm-up unless it ends it.
    #[cold]
    fn count_first(&mut self, favourable: bool, policy: &Tiers) -> bool {
        let past = Past::default();
        if !matches!(self, Self::Cold) || C::ready(policy, 1, u32::from(favourable), &past) {
            return false;
        }
        *self = Self::Warming { entries: 1, favourable: u32::from(favourable), past };
        true
    }

    /// Takes `event`: the successor of this state, and why.
    pub(super) fn transition(self, event: Event<C>, policy: &Tiers) -> (Self, Why) {
        match event {
            Event::Entry { favourable } => self.entered(favourable, policy),
            Event::GuardLeft { at } => self.left(at, policy),
            Event::Started(build) => self.started(build),
            Event::Compiled(artifact) => self.published(artifact),
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
            Self::Warming { past, .. } | Self::Queued { past } | Self::Compiling { past, .. } | Self::Speculative { past, .. } | Self::Generic { past, .. } => past,
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

    fn left(mut self, at: C::Leave, policy: &Tiers) -> (Self, Why) {
        let verdict = self.judged(at, policy);
        self.settle(verdict)
    }

    /// What the controller's judgement of a leave at `at` decides, counted into the body's
    /// stats. Only a body can be judged; the generic one only for `Interpret`.
    fn judged(&mut self, at: C::Leave, policy: &Tiers) -> Verdict<C::Feedback> {
        match self {
            Self::Speculative { past, stats, .. } => C::judge(policy, stats, past, at),
            _ => Verdict::Keep,
        }
    }

    /// The state a `verdict` on this one leaves.
    fn settle(self, verdict: Verdict<C::Feedback>) -> (Self, Why) {
        match (verdict, self) {
            (Verdict::Respecialize(feedback), Self::Speculative { past, stats, .. }) =>
                Self::rewarm(C::carried(&stats), past.after(feedback), Why::LeftTooOften),
            (Verdict::Generalize, Self::Speculative { past, stats, .. }) =>
                Self::rewarm(C::carried(&stats), Past::spent(C::learned(&stats, &past.feedback)), Why::Generalized),
            (Verdict::Interpret(refusal), Self::Speculative { .. }) =>
                (Self::GenericInterpreted { entries: 0, refusal }, Why::Unprofitable),
            (_, other) => (other, Why::Counted),
        }
    }

    /// `transition` of a run's leave at `at`, in place: the judgement counts into the body's
    /// stats without moving the state, which moves only for a verdict that changes it.
    #[inline(always)]
    pub(super) fn leave(&mut self, at: C::Leave, policy: &Tiers) -> Why {
        match self.judged(at, policy) {
            Verdict::Keep => Why::Counted,
            verdict => {
                let (next, why) = std::mem::take(self).settle(verdict);
                *self = next;
                why
            }
        }
    }

    /// An entry the owner declined to run for want of a body that fits it; at `limit` in a row
    /// no body is worth entering and the owner runs interpreted.
    #[cold]
    pub(super) fn miss(&mut self, limit: u32) -> Why {
        let Some(past) = self.past_mut() else { return Why::Counted };
        past.misses = past.misses.saturating_add(1);
        if past.misses < limit {
            return Why::Counted;
        }
        *self = Self::GenericInterpreted { entries: 0, refusal: Refusal::Shape(DECLINED) };
        Why::Unprofitable
    }

    /// An entry the owner ran: the declined ones before it are forgiven.
    #[inline(always)]
    pub(super) fn served(&mut self) {
        if let Some(past) = self.past_mut() && past.misses != 0 {
            past.misses = 0;
        }
    }

    fn past_mut(&mut self) -> Option<&mut Past<C::Feedback>> {
        match self {
            Self::Warming { past, .. } | Self::Queued { past } | Self::Compiling { past, .. } | Self::Speculative { past, .. } | Self::Generic { past, .. } => Some(past),
            Self::Cold | Self::GenericInterpreted { .. } => None,
        }
    }

    /// The history a body that is due to compile is planned from.
    pub(super) fn past(&self) -> Option<&Past<C::Feedback>> {
        match self {
            Self::Warming { past, .. } | Self::Queued { past } => Some(past),
            _ => None,
        }
    }

    /// One entry of the owner while its code compiles: counts down to the next look, then looks
    /// with `look` at the handle, which waits `interval` entries again where the code is not
    /// ready. Whether the code is still compiling afterwards is `is_compiling`.
    pub(super) fn poll_build(&mut self, interval: u16, look: impl FnOnce(Box<C::Pending>) -> Polled<C::Pending, C::Artifact>) -> Why {
        if let Self::Compiling { build, .. } = self && build.waiting() {
            return Why::Counted;
        }
        let Self::Compiling { build, past } = std::mem::take(self) else { return Why::Counted };
        let (next, why) = match build.poll(interval, look) {
            Looked::Pending(build) => (Self::Compiling { build, past }, Why::Counted),
            Looked::Ready(artifact) => Self::Queued { past }.published(artifact),
            Looked::Stale => Self::Queued { past }.dropped(Why::Stale, Past::wasted),
            Looked::Failed => Self::Queued { past }.refused_by_host(Refusal::Codegen),
        };
        *self = next;
        why
    }

    fn started(self, build: Build<C::Pending>) -> (Self, Why) {
        match self.warming_past() {
            Some(past) => (Self::Compiling { build, past }, Why::Started),
            None => unreachable!("a compile starts only from a state before one: the owner checked"),
        }
    }

    fn published(self, artifact: C::Artifact) -> (Self, Why) {
        let past = self.into_past();
        if past.is_spent() {
            return (Self::Generic { artifact, past }, Why::Published);
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
            Self::Generic { past, .. } => Self::rewarm(0, past.after(C::Feedback::default()), Why::Invalidated),
            compiling @ Self::Compiling { .. } => compiling.dropped(Why::Stale, Past::wasted),
            other => (other, Why::Counted),
        }
    }
}

#[cfg(test)]
mod tests;
