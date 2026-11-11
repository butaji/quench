//! A JavaScript function's tier: the `Tier` it owns and what the numeric and general-dispatch
//! paths decide of it. The numeric profile looks at prespecified entry counts, the general
//! baseline prices its compile against the entries or the loop work it would save; both are the
//! owner's decisions, sent to the lifecycle as events (`Due`, `Generalize`).

use super::{JsEntryStep, general_dispatch_work_due, general_promotion_due, numeric_promotion_decision};
use crate::vm::compiled::CompiledJsFunction;
use crate::vm::compiled::tier_policy::{TIERS, Tiers};
use crate::vm::tier_state::{Controller, Event, Past, Refusal, Tier, Verdict, Why};
use std::rc::Rc;

/// The function tiers' controller. Its speculative body is the numeric artifact, whose one
/// assumption is its entry profile; its generic body is the general-dispatch baseline.
pub(in crate::vm) struct JsController;

/// The entries a numeric artifact's guards rejected.
#[derive(Default)]
pub(in crate::vm) struct GuardMisses(u8);

impl Controller for JsController {
    type Pending = ();
    type Artifact = Rc<CompiledJsFunction>;
    type Feedback = ();
    type Stats = GuardMisses;
    type Leave = ();

    /// Counting never ends a warm-up: the owner looks at the profile at its prespecified entry
    /// counts (`JsTier::numeric_entry`) or prices the baseline (`JsTier::general_entry`).
    fn warmup(_: &Tiers, _: &Past<()>) -> u32 {
        u32::MAX
    }

    /// A numeric artifact that repeatedly rejects entry values is useful only for its matching
    /// profile: past `specialized_guard_misses` the baseline replaces it.
    fn judge(policy: &Tiers, stats: &mut GuardMisses, _: &Past<()>, _: ()) -> Verdict<()> {
        stats.0 = stats.0.saturating_add(1);
        if stats.0 < policy.js_function.specialized_guard_misses { Verdict::Keep } else { Verdict::Generalize }
    }

    fn carried(stats: &GuardMisses) -> u32 {
        u32::from(stats.0)
    }

    fn revives(policy: &Tiers, entries: u32) -> bool {
        entries >= u32::from(policy.js_function.general_promotion_entries)
    }
}

pub(in crate::vm) type JsTier = Tier<JsController>;

/// What an entry of the general-dispatch path found.
#[derive(Clone, Copy, Default)]
pub(super) struct GeneralEntry {
    /// The baseline is due to compile or run at this entry.
    pub(super) due: bool,
    /// The entry promoted the function's tier (a diagnostic event).
    pub(super) promoted: bool,
}

/// What a numeric artifact's demotion to the baseline left to do.
pub(super) struct Demotion {
    /// Whether the baseline is due at once.
    pub(super) compile_due: bool,
    /// The mapped bytes of the artifact it replaced, which the code cache gets back.
    pub(super) released_bytes: usize,
}

impl JsTier {
    /// Counts an entry the numeric path observes, and says what it does past counting. A
    /// profile in its numeric warm-up looks at the prespecified counts; one promoted waits for
    /// a numeric entry to compile. `full` is a code cache with no room: nothing is promoted
    /// until some is released.
    pub(super) fn numeric_entry(&mut self, numeric: bool, first: bool, general_candidate: bool, full: bool) -> JsEntryStep {
        match self {
            Self::Warming { past, .. } if !past.is_spent() => self.look(numeric, first, general_candidate, full),
            Self::Queued { past } if !past.is_spent() && numeric && !full => JsEntryStep::Compile,
            _ => JsEntryStep::Count,
        }
    }

    fn look(&mut self, numeric: bool, first: bool, general_candidate: bool, full: bool) -> JsEntryStep {
        self.apply(Event::Entry { favourable: numeric }, &TIERS);
        let Self::Warming { entries, favourable, .. } = &*self else { unreachable!("an entry of a warming tier warms it") };
        let promotion = numeric_promotion_decision(*favourable, *entries);
        if full {
            return JsEntryStep::Count;
        }
        if first || (promotion == Some(true) && numeric) {
            self.apply(Event::DUE, &TIERS);
            return JsEntryStep::PromoteAndCompile;
        }
        match promotion {
            Some(true) => self.promote(Event::DUE, JsEntryStep::Promote),
            Some(false) if general_candidate => self.promote(Event::GENERALIZE, JsEntryStep::Promote),
            // The last look found no numeric profile and no general body is wanted: the
            // function is closed for good, so later entries are no longer counted (as the
            // `Unsupported` state ended its counting before the tier lifecycle).
            Some(false) if *entries >= u32::from(u8::MAX) => self.promote(Event::Failed(Refusal::Codegen), JsEntryStep::Count),
            _ => JsEntryStep::Count,
        }
    }

    fn promote(&mut self, event: Event<JsController>, step: JsEntryStep) -> JsEntryStep {
        self.apply(event, &TIERS);
        step
    }

    /// Counts an entry the general-dispatch path observes: a hot function is promoted to price
    /// the baseline once it has run `general_promotion_entries` times, a shape the numeric
    /// tier refused likewise; one pricing the baseline is due once its entries pay for it and
    /// the code cache (`open`) has room for it.
    pub(super) fn general_entry(&mut self, priced: bool, instructions: usize, open: bool) -> GeneralEntry {
        match self {
            Self::Warming { past, .. } if !past.is_spent() => self.hot_general_entry(),
            Self::Warming { .. } => self.pricing_general_entry(priced, instructions, open),
            Self::Queued { past } => GeneralEntry { due: past.is_spent(), promoted: false },
            Self::GenericInterpreted { .. } => GeneralEntry { due: false, promoted: self.apply(Event::Entry { favourable: false }, &TIERS) == Why::Revived },
            Self::Cold | Self::Compiling { .. } | Self::Speculative { .. } | Self::Generic { .. } => GeneralEntry::default(),
        }
    }

    fn hot_general_entry(&mut self) -> GeneralEntry {
        self.apply(Event::Entry { favourable: false }, &TIERS);
        let Self::Warming { entries, .. } = &*self else { unreachable!("an entry of a warming tier warms it") };
        let promoted = *entries >= u32::from(TIERS.js_function.general_promotion_entries);
        if promoted {
            self.apply(Event::GENERALIZE, &TIERS);
        }
        GeneralEntry { due: false, promoted }
    }

    fn pricing_general_entry(&mut self, priced: bool, instructions: usize, open: bool) -> GeneralEntry {
        self.apply(Event::Entry { favourable: false }, &TIERS);
        let due = open && (!priced || self.pays(instructions));
        if due {
            self.apply(Event::DUE, &TIERS);
        }
        GeneralEntry { due, promoted: false }
    }

    /// Whether the entries a warming tier counted pay for the baseline of a body of
    /// `instructions`, each refused compile doubling what they must be.
    fn pays(&self, instructions: usize) -> bool {
        let Self::Warming { entries, past, .. } = self else { return false };
        general_promotion_due(entries.checked_shr(u32::from(past.refused)).unwrap_or(0), instructions)
    }

    /// Whether the loop work observed (`accumulated`) pays for the baseline of a body of
    /// `instructions`: a function still warming, or refused by the numeric tier, is then due.
    pub(super) fn paid_backedges(&mut self, accumulated: u64, instructions: usize, priced: bool) -> bool {
        let instructions = u64::try_from(instructions.max(1)).unwrap_or(u64::MAX);
        if !self.awaits_general_work() || !general_dispatch_work_due(accumulated, instructions, priced) {
            return false;
        }
        self.apply(Event::GENERALIZE, &TIERS);
        self.apply(Event::DUE, &TIERS);
        true
    }

    /// Counts an entry of a function the numeric tier refused, for the diagnostics only: the
    /// entries so far and the reason it was refused.
    pub(super) fn observed_rejection(&mut self) -> Option<(u64, &'static str)> {
        self.apply(Event::OBSERVED, &TIERS);
        match self {
            Self::GenericInterpreted { entries, refusal: Refusal::Shape(reason) } => Some((u64::from(*entries), reason)),
            _ => None,
        }
    }

    /// Whether loop work may still promote this function.
    pub(super) fn awaits_general_work(&self) -> bool {
        matches!(self, Self::Warming { .. } | Self::GenericInterpreted { refusal: Refusal::Shape(_), .. })
    }

    /// Counts an entry a numeric artifact's guards rejected: at `specialized_guard_misses` the
    /// baseline replaces it, due at once where its entries so far pay for it.
    pub(super) fn guard_miss(&mut self, priced: bool, instructions: usize) -> Option<Demotion> {
        let Self::Speculative { artifact, .. } = &*self else { return None };
        let released_bytes = artifact.mapped_bytes();
        if self.apply(Event::GuardLeft { at: () }, &TIERS) != Why::Generalized {
            return None;
        }
        let compile_due = !priced || self.pays(instructions);
        if compile_due {
            self.apply(Event::DUE, &TIERS);
        }
        Some(Demotion { compile_due, released_bytes })
    }

    /// Keeps `compiled` as the function's body: the baseline is the generic one.
    pub(super) fn install(&mut self, compiled: Rc<CompiledJsFunction>) {
        if compiled.is_general_dispatch() {
            self.apply(Event::GENERALIZE, &TIERS);
        }
        self.apply(Event::Compiled(compiled), &TIERS);
    }

    /// Records that no body was installed: the code cache refused one that compiled (a
    /// transient refusal, the function warms again, longer), the numeric tier refused the
    /// function's code but the baseline may take it, or the code generator failed it for good.
    pub(super) fn not_installed(&mut self, compiled: bool, general_fallback: bool) {
        let event = match (compiled, general_fallback) {
            (true, _) => Event::REFUSED,
            (false, true) => Event::REPRICE,
            (false, false) => Event::Failed(Refusal::Codegen),
        };
        self.apply(event, &TIERS);
    }

    /// The function's native body, numeric or baseline.
    pub(in crate::vm) fn native(&self) -> Option<&Rc<CompiledJsFunction>> {
        self.artifact()
    }

    /// The baseline the general tier may run for the function: `Err` where another tier owns
    /// it (a numeric body, or no tier at all), `None` where the general tier may compile one.
    pub(in crate::vm) fn general_body(&self) -> Result<Option<&Rc<CompiledJsFunction>>, ()> {
        match self.baseline() {
            Some(body) => Ok(Some(body)),
            None if self.native().is_some() || self.is_unsupported() => Err(()),
            None => Ok(None),
        }
    }

    /// The function's baseline body, which a VM frame's general loop runs.
    pub(in crate::vm) fn baseline(&self) -> Option<&Rc<CompiledJsFunction>> {
        match self {
            Self::Generic { artifact, .. } => Some(artifact),
            _ => None,
        }
    }

    /// Whether the baseline is due: its entries paid, and the next entry compiles it.
    pub(in crate::vm) fn baseline_due(&self) -> bool {
        matches!(self, Self::Queued { past } if past.is_spent())
    }

    /// Whether the general tier holds the function: pricing its baseline, due, or compiled.
    pub(in crate::vm) fn holds_for_general(&self) -> bool {
        self.baseline().is_some() || self.prices_baseline()
    }

    /// Whether the function awaits the baseline: pricing it or due, or refused by the numeric
    /// tier, which the baseline may take.
    pub(in crate::vm) fn awaits_baseline(&self) -> bool {
        self.prices_baseline() || self.is_rejected()
    }

    fn prices_baseline(&self) -> bool {
        matches!(self, Self::Warming { past, .. } | Self::Queued { past } if past.is_spent())
    }

    /// Whether the numeric tier takes no entry of the function: the baseline or the interpreter
    /// owns it.
    pub(in crate::vm) fn closed_to_numeric(&self) -> bool {
        self.prices_baseline() || self.is_refused()
    }

    /// Whether the code generator failed the function: no tier takes it again.
    pub(in crate::vm) fn is_unsupported(&self) -> bool {
        matches!(self, Self::GenericInterpreted { refusal: Refusal::Codegen, .. })
    }

    /// Whether the code generator builds nothing for the function.
    pub(in crate::vm) fn is_refused(&self) -> bool {
        matches!(self, Self::GenericInterpreted { .. })
    }

    /// Whether the numeric tier refused the function's shape, which the general tier may take.
    pub(in crate::vm) fn is_rejected(&self) -> bool {
        matches!(self, Self::GenericInterpreted { refusal: Refusal::Shape(_), .. })
    }

    /// The name of its state, for the diagnostics report.
    pub(in crate::vm) fn label(&self) -> &'static str {
        match self {
            Self::Cold | Self::Warming { .. } if !self.holds_for_general() => "hot",
            Self::Cold | Self::Warming { .. } => "general-promotion-pending",
            Self::Queued { .. } if self.baseline_due() => "general-compile-due",
            Self::Queued { .. } | Self::Compiling { .. } => "promotion-pending",
            Self::GenericInterpreted { .. } => "unsupported",
            Self::Speculative { .. } | Self::Generic { .. } => "compiled",
        }
    }
}
