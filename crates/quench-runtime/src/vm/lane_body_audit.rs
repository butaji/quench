//! Whether a function's compiled body runs faster than the interpreter runs it: after the body
//! publishes, one entry in `audit_period` runs timed, alternating the body and the interpreter
//! (the activation run nested to its return, `body_calls::interpret_timed`). Once each side has
//! `audit_samples` timed runs, a body not faster by `audit_margin_permille` retires and its
//! function stays interpreted (`TIERS.lane_body`). Per function this holds the invariant that
//! compiled code is never slower than the interpreter, from the one statistic it is about.

use std::time::Instant;

use super::{LaneBody, LaneView, TIERS};
use super::super::compiled::tier_policy::PER_MILLE;

/// The timed runs of one body and of its function's interpreted runs.
#[derive(Clone, Copy, Default)]
pub(in crate::vm) struct Audit {
    seen: u32,
    sides: [Side; Timed::SIDES],
    done: bool,
}

#[derive(Clone, Copy, Default)]
struct Side {
    nanos: u64,
    runs: u16,
}

/// Which side an entry runs timed on.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::vm) enum Timed {
    Body,
    Interpreter,
}

impl Timed {
    const SIDES: usize = 2;
}

impl LaneView {
    /// Counts an entry of the published body while its audit runs: the side it runs timed on,
    /// one entry in `audit_period`, alternating; `None` for the rest and once the audit ended.
    #[inline(always)]
    pub(in crate::vm) fn audit_entry(&self) -> Option<Timed> {
        let mut history = self.history.get();
        if history.audit.done {
            return None;
        }
        history.audit.seen = history.audit.seen.wrapping_add(1);
        self.history.set(history);
        let period = TIERS.lane_body.audit_period;
        (history.audit.seen % period == 0)
            .then(|| if (history.audit.seen / period) % 2 == 0 { Timed::Body } else { Timed::Interpreter })
    }

    /// Books a completed timed run of `nanos` on `side`, then takes the verdict once both
    /// sides have their samples.
    pub(in crate::vm) fn audit_record(&self, side: Timed, nanos: u64) {
        let mut history = self.history.get();
        let booked = &mut history.audit.sides[side as usize];
        booked.nanos = booked.nanos.saturating_add(nanos);
        booked.runs = booked.runs.saturating_add(1);
        self.history.set(history);
        self.audit_verdict();
    }

    /// Books the body's timed run that ended at `started`'s clock, if this entry was timed.
    pub(in crate::vm) fn audit_body_run(&self, started: Option<Instant>) {
        if let Some(started) = started {
            self.audit_record(Timed::Body, audit_nanos(started));
        }
    }

    /// Ends the audit: a body whose mean timed run is not `audit_margin_permille` below the
    /// interpreter's retires, and its function runs in the lane for good.
    fn audit_verdict(&self) {
        let mut history = self.history.get();
        let lane = &TIERS.lane_body;
        let [body, interpreted] = history.audit.sides;
        if history.audit.done || body.runs < lane.audit_samples || interpreted.runs < lane.audit_samples {
            return;
        }
        history.audit.done = true;
        self.history.set(history);
        let verdict = Verdict::of(body, interpreted, lane.audit_margin_permille);
        self.body_stats.audit.set(Some(verdict));
        if !verdict.retire {
            return;
        }
        // SAFETY: as `lane_body`; the verdict runs between runs, after the run that ended it.
        let lane_body = unsafe { &mut *self.tier_lane.get() };
        if matches!(lane_body, LaneBody::Specialized { .. }) {
            self.retire_body(lane_body, LaneBody::Declined);
            self.release_from_tier();
        }
    }

    /// Starts the audit of a body just specialized.
    pub(super) fn audit_restart(&self) {
        let mut history = self.history.get();
        history.audit = Audit::default();
        self.history.set(history);
        self.body_stats.audit.set(None);
    }
}

/// The nanoseconds since `started`.
pub(in crate::vm) fn audit_nanos(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

/// What an audit concluded: each side's mean timed run and whether the body retires.
#[derive(Clone, Copy)]
pub(in crate::vm) struct Verdict {
    body_nanos: u64,
    interpreter_nanos: u64,
    retire: bool,
}

impl Verdict {
    /// A body retires unless its mean run is `margin_permille` below the interpreter's mean,
    /// compared without a division: `body / b_runs < interpreted / i_runs x (1 - margin)`.
    fn of(body: Side, interpreted: Side, margin_permille: u64) -> Self {
        let retire = u128::from(body.nanos) * u128::from(interpreted.runs) * u128::from(PER_MILLE)
            >= u128::from(interpreted.nanos) * u128::from(body.runs) * u128::from(PER_MILLE - margin_permille);
        Self { body_nanos: body.mean(), interpreter_nanos: interpreted.mean(), retire }
    }

    /// The `QUENCH_LANE_BODY` report's `audit` value.
    pub(in crate::vm) fn report(self) -> String {
        format!("{{\"body_ns\":{},\"interpreter_ns\":{},\"retired\":{}}}", self.body_nanos, self.interpreter_nanos, self.retire)
    }
}

impl Side {
    fn mean(self) -> u64 {
        self.nanos / u64::from(self.runs.max(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MARGIN: u64 = 50;

    fn side(nanos: u64, runs: u16) -> Side {
        Side { nanos, runs }
    }

    #[test]
    fn a_body_clearly_faster_stays() {
        assert!(!Verdict::of(side(800, 4), side(1000, 4), MARGIN).retire);
    }

    #[test]
    fn a_body_only_as_fast_retires() {
        assert!(Verdict::of(side(1000, 4), side(1000, 4), MARGIN).retire);
        assert!(Verdict::of(side(960, 4), side(1000, 4), MARGIN).retire);
    }

    #[test]
    fn the_margin_boundary_is_inclusive_and_means_not_totals_compare() {
        // 950 against 1000 is exactly the margin: not faster than it, so retired.
        assert!(Verdict::of(side(950, 1), side(1000, 1), MARGIN).retire);
        assert!(!Verdict::of(side(949, 1), side(1000, 1), MARGIN).retire);
        // Twice the runs at twice the total is the same mean.
        assert!(Verdict::of(side(2000, 8), side(1000, 4), MARGIN).retire);
    }
}
