//! What the tier machine learns from a specialized body's runs, and the levels and promotions it
//! draws from that: completions, native credits, held and failed plans, the compile level.
use super::super::*;
use super::{LaneBody, LaneBodyHistory, PERCENT};

impl LaneView {
    /// Counts a run of this function's body that a completed native caller's run contains.
    pub(super) fn note_native_completions(&self, runs: u32) {
        // SAFETY: as `note_native_leave`; no run of this body is inspecting it.
        let body = unsafe { &mut *self.tier_lane.get() };
        if let LaneBody::Specialized { native_completions, .. } = body {
            native_completions.set(native_completions.get().saturating_add(runs));
            self.promote_if_repaid(body);
        }
    }

    /// Books a body that left before it held as a failure of its plan's sites (one failed
    /// site at least; which is unknown), once.
    pub(super) fn note_failed(&self) {
        let history = self.history.get();
        if history.held_noted { return; }
        self.history.set(LaneBodyHistory { held_noted: true, ..history });
        crate::vm::compiled::note_site_outcome(u32::from(history.planned_sites), false);
    }

    /// Books a body that ran its first credit window without leaving as a success of every site
    /// its plan relied on (`site_outcome`), once.
    pub(super) fn note_held(&self, completions: u64, leaves: u64) {
        let history = self.history.get();
        if history.held_noted || leaves * PERCENT >= completions { return; }
        self.history.set(LaneBodyHistory { held_noted: true, ..history });
        crate::vm::compiled::note_site_outcome(u32::from(history.planned_sites), true);
    }

    /// A Quick body whose evidence (`body_completion_evidence`) shows it has run long enough
    /// without leaving repays an Optimized compile: its next compile is one.
    pub(super) fn promote_if_repaid(&self, body: &mut LaneBody) {
        let LaneBody::Specialized { promote_at, recompiles, stuck, native_completions, native_leaves, completions, exits, .. } = body else { return };
        let (done, left) = (u64::from(native_completions.get()) + u64::from(completions.get()), u64::from(native_leaves.get()) + u64::from(exits.get()));
        let (promote_at, recompiles, stuck) = (*promote_at, *recompiles, *stuck);
        if promote_at == 0 || done < u64::from(promote_at) || left * PERCENT >= done { return; }
        self.history.set(LaneBodyHistory { promoted: true, ..self.history.get() });
        self.retire_body(body, LaneBody::Unspecialized { entries: 0, recompiles, stuck });
    }

    /// What the runs of this body have shown the tier machine: its completions, its leaves,
    /// and whether the completions are exact (`false`: natively called runs count as many as
    /// their caller's completed runs, a bound). `None` unless it is specialized.
    pub(in crate::vm) fn body_completion_evidence(&self) -> Option<(u64, u64, bool)> {
        // SAFETY: as `note_native_leave`.
        match unsafe { &*self.tier_lane.get() } {
            LaneBody::Specialized { native_completions, native_leaves, completions, exits, .. } => Some((
                u64::from(native_completions.get()) + u64::from(completions.get()),
                u64::from(native_leaves.get()) + u64::from(exits.get()),
                false,
            )),
            _ => None,
        }
    }

    /// The diagnostics line of what `body_completion_evidence` holds.
    pub(in crate::vm) fn report_completion_evidence(&self, program: usize, function: u32) {
        if let Some((completions, leaves, _)) = self.body_completion_evidence() {
            eprintln!("QUENCH_LANE_EVIDENCE {{\"program\":{program},\"function\":{function},\"completions\":{completions},\"leaves\":{leaves}}}");
        }
    }

    /// The level its next compile runs at: the mode's where it fixes one. The default mode starts
    /// every body Optimized: Quick first, promoted on completions, measured slower than that on
    /// deltablue, crypto, regexp and fib (`ab-quick-first-promote-vs-tip`), until the evidence that
    /// promotes a natively called body exists (`LaneBodyHistory::promoted`).
    pub(in crate::vm) fn compile_level(&self, sites: usize, entries: u32) -> crate::vm::jit_backend::CompileLevel {
        use crate::vm::jit_backend::CompileLevel;
        let history = self.history.get();
        let level = if CompileLevel::run_default() == CompileLevel::Quick { CompileLevel::Quick }
            else if !crate::jit_mode::tier_allowed(crate::jit_mode::Tier::Quick) || history.promoted { CompileLevel::Optimized }
            else if TIERS.priors.selective_quick { crate::vm::compiled::level_for_plan(sites, entries) } else { CompileLevel::Optimized };
        self.history.set(LaneBodyHistory { quick: level == CompileLevel::Quick, planned_sites: u16::try_from(sites).unwrap_or(u16::MAX), ..history });
        level
    }
}
