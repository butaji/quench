//! Whether a function's compiled body runs faster than the interpreter runs it: after the body
//! publishes, one entry in `audit_period` runs timed, alternating the body and the interpreter
//! (the activation run nested to its return, `body_calls::interpret_timed`). Once each side has
//! `audit_samples` timed runs, a body not faster by `audit_margin_permille` retires and its
//! function stays interpreted (`TIERS.lane_body`). Per function this holds the invariant that
//! compiled code is never slower than the interpreter, from the one statistic it is about.

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
        // Means compared without a division: body / b_runs >= interpreted / i_runs x (1 - margin).
        let slower = u128::from(body.nanos) * u128::from(interpreted.runs) * u128::from(PER_MILLE)
            >= u128::from(interpreted.nanos) * u128::from(body.runs) * u128::from(PER_MILLE - lane.audit_margin_permille);
        if std::env::var_os("QDBG_AUDIT").is_some() { eprintln!("DBGA body {} ns/run x{} interp {} ns/run x{} retire {}", body.nanos / u64::from(body.runs), body.runs, interpreted.nanos / u64::from(interpreted.runs), interpreted.runs, slower); }
        if !slower {
            return;
        }
        // SAFETY: as `lane_body`; the verdict runs between runs, and the caller's lease of the
        // body keeps its code alive past the retirement, as a direct caller's does.
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
    }
}
