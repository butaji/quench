//! A lane body's specialization at its function's entries and loops (`specialize_lane_body`):
//! the attempt a view makes at its body, and the state each attempt leaves (`body_after`).

use super::*;

#[quench_macros::ext(LaneViewSpecializeOps)]
impl LaneView {
    /// A mode with neither Cranelift function tiers nor `--b-jit`'s stencil bodies compiles no
    /// function body: the view's body is declined, once.
    fn declined_by_mode(&self) -> bool {
        let declined = !quench_runtime::jit_mode::tiers_allowed(quench_runtime::jit_mode::Tier::CRANELIFT) && !quench_runtime::jit_mode::base_only();
        if declined {
            // SAFETY: as `lane_body`.
            unsafe { *self.tier().tier_lane.get() = LaneBody::Declined };
        }
        declined
    }

    /// The body epoch: how many specialization attempts the view has made (`specialize_lane_body`).
    pub(crate) fn body_epoch(&self) -> u32 { self.tier().body_epoch.get() }

    /// Counts an entry toward the frameless body and specializes it at the warm-up; whether
    /// the body is still warming or specialized, so its prologue stays. A specialized body is
    /// a fact of the view from then on: its prologue record enters it with `entry`, which
    /// tests nothing the specialization proved (`lane_body_entry`).
    pub(crate) fn specialize_lane_body(&self, entry: Handler, looped: bool, work: impl FnOnce(crate::compiled::LaneBodyWork) -> crate::compiled::LaneBodyOutcome) -> bool {
        if self.declined_by_mode() { return false; }
        self.settle_retired_body();
        // SAFETY: as `lane_body`; specializing runs no lane body, and the body is out of the view while the VM works on it.
        let body = unsafe { &mut *self.tier().tier_lane.get() };
        if awaits_look(body) { return true; }
        let (outcome, entries, recompiles, stuck, looks) = match std::mem::replace(body, LaneBody::Declined) {
            // A looping caller's trips ask for the attempt at once: its entries may not advance (a single call).
            LaneBody::Unspecialized { entries, recompiles, stuck } if looped || self.attempts_at(entries.saturating_add(1)) || self.recurrent(entries.saturating_add(1)) => {
                if !self.affords_compile() {
                    *body = LaneBody::Unspecialized { entries, recompiles, stuck };
                    return false;
                }
                crate::compiled::base_region::count(crate::compiled::base_region::Event::BodyCompile);
                if !self.attempts_at(entries.saturating_add(1)) { crate::compiled::base_region::count(crate::compiled::base_region::Event::BodyRecurrent); }
                // A predecessor's few leaves are the specialization's feedback (`stuck`).
                let generic: Vec<_> = stuck.exits().collect();
                (work(crate::compiled::LaneBodyWork::Start { generic: &generic, shrinks: self.tier().history.get().shrinks, observed: self.attempts_at(entries.saturating_add(1)).then(|| entries.saturating_add(1)), looped: None, called: self.called_by_bodies(), caller: self, arrays: &[] }), entries, recompiles, stuck, 0)
            }
            LaneBody::Unspecialized { entries, recompiles, stuck } => {
                let ticks = warm_ticks();
                if entries == 0 { self.tier().warm_since.set(ticks); }
                *body = LaneBody::Unspecialized { entries: entries.saturating_add(1), recompiles, stuck };
                return true;
            }
            LaneBody::Compiling { pending, recompiles, stuck, looks, .. } =>
                (work(crate::compiled::LaneBodyWork::Poll(pending, self)), 0, recompiles, stuck, looks.saturating_add(1)),
            settled => { *body = settled; return !matches!(body, LaneBody::Declined); }
        };
        *body = self.body_after(outcome, entry, entries, recompiles, stuck, looks);
        // A state change moves the epoch; a body still warming (its entries counted) does not, so a refused loop backs off.
        if !matches!(body, LaneBody::Unspecialized { .. }) {
            let epochs = &self.tier().body_epoch;
            epochs.set(epochs.get().wrapping_add(1));
        }
        !matches!(body, LaneBody::Declined)
    }

    /// The body of a function of a program with a loaded AOT image: `install` takes it from the
    /// image (`Vm::install_ahead_body`) at once, with no warm-up, no compile budget and no tier
    /// gate, since an image has no profiling phase; a function the image has no body for runs in
    /// the lane, never in a runtime tier. Whether it has a body.
    pub(crate) fn install_ahead_lane_body(&self, entry: Handler, install: impl FnOnce() -> Option<std::rc::Rc<quench_runtime::vm::engine_types::PublishedLaneMethod>>) -> bool {
        self.settle_retired_body();
        // SAFETY: as `lane_body`; installing runs no lane body, and the body is out of the view while the VM works on it.
        let body = unsafe { &mut *self.tier().tier_lane.get() };
        match std::mem::replace(body, LaneBody::Declined) {
            LaneBody::Unspecialized { recompiles, stuck, .. } => {
                let outcome = install().map_or(crate::compiled::LaneBodyOutcome::Declined, crate::compiled::LaneBodyOutcome::Published);
                *body = self.body_after(outcome, entry, 0, recompiles, stuck, 0);
                let epochs = &self.tier().body_epoch;
                epochs.set(epochs.get().wrapping_add(1));
            }
            settled => *body = settled,
        }
        !matches!(body, LaneBody::Declined)
    }

    /// The body a view's compile work came to (`LaneBodyOutcome`) makes of it; a compile in
    /// flight records the inlining its plan fits (`LaneBodyHistory::shrinks`).
    fn body_after(&self, outcome: crate::compiled::LaneBodyOutcome, entry: Handler, entries: u32, recompiles: u8, stuck: LaneLeaves, looks: u8) -> LaneBody {
        {
            use crate::compiled::{LaneBodyOutcome as O, base_region::{Event, count}};
            count(match &outcome {
                O::Published(_) => Event::BodyPublished, O::Compiling(_) => Event::BodyCompiling, O::Unpaid => Event::BodyUnpaid,
                O::Short => Event::BodyShort, O::Declined => Event::BodyDeclined, O::Stale => Event::BodyStale, O::Deferred => Event::BodyDeferred, O::Base => Event::BodyBase,
            });
        }
        match outcome {
            crate::compiled::LaneBodyOutcome::Compiling(pending) => {
                self.tier().history.set(LaneBodyHistory { shrinks: pending.shrinks(), ..self.tier().history.get() });
                let wait = 1u16.checked_shl(u32::from(looks)).map_or(u16::MAX, |entries| entries - 1).min(self.tiers.lane_body.compile_poll_interval);
                LaneBody::Compiling { pending, wait: std::cell::Cell::new(wait), looks, recompiles, stuck }
            }
            crate::compiled::LaneBodyOutcome::Published(published) => {
                self.respecialize_callers();
                self.specialized(entry, published, recompiles, stuck)
            }
            // A compile a collection or a changed fact outran is a body a lifecycle edge dropped.
            crate::compiled::LaneBodyOutcome::Stale => {
                self.note_dropped_compile();
                LaneBody::Unspecialized { entries: 0, recompiles, stuck }
            }
            crate::compiled::LaneBodyOutcome::Deferred => LaneBody::Unspecialized { entries: entries.saturating_add(1), recompiles, stuck },
            // A b-jit body serves it better: the entry stitches that body (`Vm::base_body_work`), and the u-jit body waits.
            crate::compiled::LaneBodyOutcome::Base => LaneBody::Unspecialized { entries: entries.saturating_add(1), recompiles, stuck },
            // Its entries decide when it looks again (`attempts_at`), no hot loop's skip.
            crate::compiled::LaneBodyOutcome::Unpaid => {
                self.tier().history.set(LaneBodyHistory { unpaid: true, ..self.tier().history.get() });
                let next = entries.saturating_add(1);
                // The attempts double with each warm-up; past the last one the counter can
                // name, the model has priced every entry this function can give and refused.
                // The lane would keep running it through its records for nothing, slower than
                // the general interpreter, so decline it there (`release_from_tier`).
                if self.last_attempt(next) { return LaneBody::Declined }
                LaneBody::Unspecialized { entries: next, recompiles, stuck }
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

    /// `specialize_lane_body` at once, past the warm-up: a hot loop's backward jump has run
    /// its calls, so their sites have linked (`lane_loop_retire`).
    pub(crate) fn specialize_lane_body_now(&self, entry: Handler, looped: bool, specialize: impl FnOnce(crate::compiled::LaneBodyWork) -> crate::compiled::LaneBodyOutcome) {
        self.skip_warm_up();
        self.specialize_lane_body(entry, looped, specialize);
        // Its entries are few (the loop is what is hot): the next one compiles where this one
        // waited for its callees' bodies (`LaneBodyOutcome::Deferred`).
        self.skip_warm_up();
    }

    /// Whether the tier model priced a compile of its body and found it would not pay
    /// (`LaneBodyOutcome::Unpaid`): its entries alone decide when it looks again.
    pub(crate) fn body_unpaid(&self) -> bool { self.tier().history.get().unpaid }

    /// A function whose bodies lifecycle edges discarded warms as any entry does.
    fn skip_warm_up(&self) {
        // SAFETY: as `lane_body`; no lane body runs here.
        if let LaneBody::Unspecialized { entries, .. } = unsafe { &mut *self.tier().tier_lane.get() }
            && self.tier().history.get().lifetimes == 0 && !self.tier().history.get().unpaid
        {
            *entries = u32::from(self.tiers.lane_body.warmup()) - 1;
        }
    }
}
