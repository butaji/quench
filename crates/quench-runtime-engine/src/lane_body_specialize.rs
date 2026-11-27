//! A lane body's specialization at its function's entries and loops (`specialize_lane_body`):
//! the attempt a view makes at its body, and the state each attempt leaves (`body_after`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// Whether the mode compiles function bodies at all (Cranelift's tiers or `--b-jit`'s stencils): `--no-jit` compiles none.
fn mode_compiles_bodies() -> bool {
    crate::compiled::admission::specialize(crate::compiled::admission::UnitRequest { kind: crate::compiled::admission::UnitKind::Body, facts: Default::default() }).compiled().is_some()
}

#[quench_macros::ext(LaneViewSpecializeOps)]
impl LaneView {
    /// A mode with neither Cranelift function tiers nor `--b-jit`'s stencil bodies compiles no
    /// function body: the view's body is declined, once.
    fn declined_by_mode(&self) -> bool {
        let declined = !mode_compiles_bodies();
        if declined {
            // SAFETY: as `lane_body`.
            unsafe { *self.tier().tier_lane.get() = LaneBody::Declined };
        }
        declined
    }

    /// The body epoch: how many specialization attempts the view has made (`specialize_lane_body`).
    pub(crate) fn body_epoch(&self) -> u32 { self.tier().body_epoch.get() }

    /// Specializes the frameless body at the function's entry: the attempt its predicted tier admits (`lane_facts::body_pick`),
    /// or none, where the interpreter runs the function. Whether the body is still compiling or specialized, so its
    /// prologue stays. A specialized body is a fact of the view from then on: its prologue record enters it with `entry`,
    /// which tests nothing the specialization proved (`lane_body_entry`).
    pub(crate) fn specialize_lane_body(&self, entry: Handler, work: impl FnOnce(crate::compiled::LaneBodyWork) -> crate::compiled::LaneBodyOutcome) -> bool {
        if self.declined_by_mode() { return false; }
        self.settle_retired_body();
        // SAFETY: as `lane_body`; specializing runs no lane body, and the body is out of the view while the VM works on it.
        let body = unsafe { &mut *self.tier().tier_lane.get() };
        if awaits_look(body) { return true; }
        let (outcome, recompiles, stuck, looks) = match std::mem::replace(body, LaneBody::Declined) {
            LaneBody::Unspecialized { recompiles, stuck } => {
                if !self.affords_compile() {
                    *body = LaneBody::Unspecialized { recompiles, stuck };
                    return false;
                }
                crate::compiled::base_region::count(crate::compiled::base_region::Event::BodyCompile);
                // A predecessor's few leaves are the specialization's feedback (`stuck`).
                let generic: Vec<_> = stuck.exits().collect();
                (work(crate::compiled::LaneBodyWork::Start { generic: &generic, shrinks: self.tier().history.get().shrinks, looped: None, called: self.called_by_bodies(), caller: self }), recompiles, stuck, 0)
            }
            LaneBody::Compiling { pending, recompiles, stuck, looks, .. } =>
                (work(crate::compiled::LaneBodyWork::Poll(pending, self)), recompiles, stuck, looks.saturating_add(1)),
            settled => { *body = settled; return !matches!(body, LaneBody::Declined); }
        };
        *body = self.body_after(outcome, quench_runtime::vm::tier_policy::body_tier_in_lane(), entry, recompiles, stuck, looks);
        // A state change moves the epoch; a body still waiting for its next entry does not, so a refused loop backs off.
        if !matches!(body, LaneBody::Unspecialized { .. }) {
            let epochs = &self.tier().body_epoch;
            epochs.set(epochs.get().wrapping_add(1));
        }
        !matches!(body, LaneBody::Declined)
    }

    /// The body of a function of a program with a loaded AOT image: `install` takes it from the
    /// image (`Vm::install_ahead_body`) at once, with no compile budget and no tier gate, since an image
    /// has no profiling phase. It is the ladder's first body: its leaves
    /// respecialize it on the JIT's route (`specialize_in_lane`). Whether it has a body.
    pub(crate) fn install_ahead_lane_body(&self, entry: Handler, install: impl FnOnce() -> Option<std::rc::Rc<quench_runtime::vm::engine_types::PublishedLaneMethod>>) -> bool {
        self.settle_retired_body();
        // SAFETY: as `lane_body`; installing runs no lane body, and the body is out of the view while the VM works on it.
        let body = unsafe { &mut *self.tier().tier_lane.get() };
        match std::mem::replace(body, LaneBody::Declined) {
            LaneBody::Unspecialized { recompiles, stuck, .. } => {
                let outcome = install().map_or(crate::compiled::LaneBodyOutcome::Declined, crate::compiled::LaneBodyOutcome::Published);
                *body = self.body_after(outcome, quench_runtime::jit_mode::Tier::Ahead, entry, recompiles, stuck, 0);
                let epochs = &self.tier().body_epoch;
                epochs.set(epochs.get().wrapping_add(1));
            }
            settled => *body = settled,
        }
        !matches!(body, LaneBody::Declined)
    }

    /// The body a view's compile work came to (`LaneBodyOutcome`) makes of it; a compile in
    /// flight records the inlining its plan fits (`LaneBodyHistory::shrinks`).
    fn body_after(&self, outcome: crate::compiled::LaneBodyOutcome, tier: quench_runtime::jit_mode::Tier, entry: Handler, recompiles: u8, stuck: LaneLeaves, looks: u8) -> LaneBody {
        {
            use crate::compiled::{LaneBodyOutcome as O, base_region::{Event, count}};
            count(match &outcome {
                O::Published(_) => Event::BodyPublished, O::Compiling(_) => Event::BodyCompiling, O::Unpaid => Event::BodyUnpaid,
                O::Short => Event::BodyShort, O::Declined => Event::BodyDeclined, O::Stale => Event::BodyStale, O::Deferred => Event::BodyDeferred,
            });
        }
        match outcome {
            crate::compiled::LaneBodyOutcome::Compiling(pending) => {
                self.tier().history.set(LaneBodyHistory { shrinks: crate::compiled::pending_lane_ref(&*pending).shrinks(), ..self.tier().history.get() });
                let wait = 1u16.checked_shl(u32::from(looks)).map_or(u16::MAX, |entries| entries - 1).min(self.tiers.lane_body.compile_poll_interval);
                LaneBody::Compiling { pending, wait: std::cell::Cell::new(wait), looks, recompiles, stuck }
            }
            crate::compiled::LaneBodyOutcome::Published(published) => {
                self.respecialize_callers();
                self.specialized(entry, published, tier, recompiles, stuck)
            }
            // A compile a collection or a changed fact outran is a body a lifecycle edge dropped.
            crate::compiled::LaneBodyOutcome::Stale => {
                self.note_dropped_compile();
                LaneBody::Unspecialized { recompiles, stuck }
            }
            crate::compiled::LaneBodyOutcome::Deferred => LaneBody::Unspecialized { recompiles, stuck },
            // Its predicted tier is the interpreter (`lane_facts::body_pick`): the interpreter runs it.
            crate::compiled::LaneBodyOutcome::Unpaid => {
                self.tier().history.set(LaneBodyHistory { unpaid: true, ..self.tier().history.get() });
                LaneBody::Declined
            }
            crate::compiled::LaneBodyOutcome::Short => {
                self.tier().history.set(LaneBodyHistory { short: true, ..self.tier().history.get() });
                LaneBody::Declined
            }
            crate::compiled::LaneBodyOutcome::Declined => {
                if self.tier().history.get().lifetimes > 0 { crate::dispatch_fast::lane_body_stats::decline("dropped bodies"); }
                self.tier().body_stats.declined.set(crate::dispatch_fast::lane_body_stats::take_decline());
                LaneBody::Declined
            }
        }
    }

    /// Whether the predicted tier refused a compile of its body (`LaneBodyOutcome::Unpaid`), which the loop tier reads
    /// before it waits for its own iterations.
    pub(crate) fn body_unpaid(&self) -> bool { self.tier().history.get().unpaid }
}
