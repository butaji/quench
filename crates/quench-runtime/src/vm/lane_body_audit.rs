//! Whether a function's compiled body makes the program faster than the interpreter does:
//! after the body publishes, its entries alternate in epochs of `audit_period` entries between
//! the body and the interpreter, each run by the lane's own path (the cell or prologue entry of
//! the body; the plain activation of the interpreter), and each epoch's clock time from its
//! first entry to the next epoch's is booked to its side. Once each side has `audit_samples`
//! epochs, a body whose median epoch is not `audit_margin_permille` below the interpreter's
//! retires, and its function stays interpreted (`TIERS.lane_body`). An epoch holds everything
//! the program did between those entries, so the comparison prices what the body costs where it
//! is entered (the entry's binding of the lane frame, its helpers' calls back into the VM) and
//! not only its own instructions; the median keeps a collection that lands in one epoch from
//! deciding it. Per function this holds the invariant that compiled code is never slower than
//! the interpreter, from the one statistic it is about.

use std::time::Instant;

use super::{LaneBody, LaneView, TIERS};
use super::super::compiled::tier_policy::PER_MILLE;

/// The most epochs a side keeps (`audit_samples` is capped by it).
const MAX_EPOCHS: usize = 32;

/// The entry count of the running audit, in the view's history (`LaneBodyHistory`).
#[derive(Clone, Copy, Default)]
pub(in crate::vm) struct Audit {
    seen: u32,
    done: bool,
}

/// The audit's epochs: when the running one began, and each side's booked durations.
#[derive(Clone, Copy, Default)]
pub(in crate::vm) struct Epochs {
    started: Option<Instant>,
    nanos: [[u32; MAX_EPOCHS]; SIDES],
    filled: [u8; SIDES],
}

/// The sides an audit alternates between: the compiled body (epochs 0, 2, ...) and the
/// interpreter (epochs 1, 3, ...).
const SIDES: usize = 2;
const BODY: usize = 0;
const INTERPRETER: usize = 1;

impl LaneView {
    /// Counts an entry of the published body while its audit runs; whether the entry runs
    /// interpreted, its epoch being the interpreter's. A cell entry (`counts_interpreted`
    /// false) of an interpreted epoch counts nothing and declines the body: the activation it
    /// makes enters the prologue, which counts it.
    #[inline(always)]
    pub(in crate::vm) fn audit_entry(&self, counts_interpreted: bool) -> bool {
        let mut history = self.tier().history.get();
        if history.audit.done {
            return false;
        }
        let period = TIERS.lane_body.audit_period;
        let seen = history.audit.seen;
        let interpreted = (seen / period) % SIDES as u32 == INTERPRETER as u32;
        if interpreted && !counts_interpreted {
            return true;
        }
        history.audit.seen = seen.wrapping_add(1);
        self.tier().history.set(history);
        if seen % period == 0 {
            self.audit_epoch_boundary(seen / period);
        }
        interpreted
    }

    /// Ends the epoch before `epoch`, booking its time to its side, and starts `epoch`; takes
    /// the verdict once both sides have their epochs.
    #[cold]
    fn audit_epoch_boundary(&self, epoch: u32) {
        let now = Instant::now();
        let mut epochs = self.tier().audit_epochs.get();
        if let Some(started) = epochs.started
            && epoch > 0
        {
            let side = ((epoch - 1) % SIDES as u32) as usize;
            let filled = usize::from(epochs.filled[side]);
            if filled < MAX_EPOCHS {
                epochs.nanos[side][filled] = u32::try_from(now.duration_since(started).as_nanos()).unwrap_or(u32::MAX);
                epochs.filled[side] += 1;
            }
        }
        epochs.started = Some(now);
        self.tier().audit_epochs.set(epochs);
        let samples = usize::from(TIERS.lane_body.audit_samples).min(MAX_EPOCHS);
        if epochs.filled.iter().all(|&filled| usize::from(filled) >= samples) {
            self.audit_verdict(&epochs, samples);
        }
    }

    /// Ends the audit: a body whose median epoch is not `audit_margin_permille` below the
    /// interpreter's retires, and its function runs in the lane for good.
    fn audit_verdict(&self, epochs: &Epochs, samples: usize) {
        let mut history = self.tier().history.get();
        history.audit.done = true;
        self.tier().history.set(history);
        let median = |side: usize| {
            let mut nanos = epochs.nanos[side][..samples].to_vec();
            nanos.sort_unstable();
            u64::from(nanos[samples / 2])
        };
        let verdict = Verdict::of(median(BODY), median(INTERPRETER), TIERS.lane_body.audit_margin_permille);
        self.tier().body_stats.audit.set(Some(verdict));
        if !verdict.retire {
            return;
        }
        // SAFETY: as `lane_body`; the verdict runs between runs, at an entry's count.
        let lane_body = unsafe { &mut *self.tier().tier_lane.get() };
        if matches!(lane_body, LaneBody::Specialized { .. }) {
            self.retire_body(lane_body, LaneBody::Declined);
            self.release_from_tier();
        }
    }

    /// Starts the audit of a body just specialized.
    pub(super) fn audit_restart(&self) {
        let mut history = self.tier().history.get();
        history.audit = Audit::default();
        self.tier().history.set(history);
        self.tier().audit_epochs.set(Epochs::default());
        self.tier().body_stats.audit.set(None);
    }
}

/// What an audit concluded: each side's median epoch and whether the body retires.
#[derive(Clone, Copy)]
pub(in crate::vm) struct Verdict {
    body_nanos: u64,
    interpreter_nanos: u64,
    retire: bool,
}

impl Verdict {
    /// A body retires unless its median epoch is `margin_permille` below the interpreter's:
    /// `body < interpreted x (1 - margin)`.
    fn of(body: u64, interpreted: u64, margin_permille: u64) -> Self {
        let retire = u128::from(body) * u128::from(PER_MILLE) >= u128::from(interpreted) * u128::from(PER_MILLE - margin_permille);
        Self { body_nanos: body, interpreter_nanos: interpreted, retire }
    }

    /// The `QUENCH_LANE_BODY` report's `audit` value.
    pub(in crate::vm) fn report(self) -> String {
        format!("{{\"body_ns\":{},\"interpreter_ns\":{},\"retired\":{}}}", self.body_nanos, self.interpreter_nanos, self.retire)
    }
}
