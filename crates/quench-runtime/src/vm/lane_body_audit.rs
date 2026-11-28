//! Whether a function's compiled body makes the program faster than the interpreter does:
//! after the body publishes, its entries alternate in epochs of `audit_period` entries between
//! the body and the interpreter, each run by the lane's own path (the cell or prologue entry of
//! the body; the plain activation of the interpreter), and each epoch's CPU time on the
//! program's thread from its first entry to the next epoch's is booked to its side
//! (`thread_cpu_nanos`: wall time would book the time the thread waited for a core, which
//! other processes decide, and the same run would keep or retire a body by the load on the host). Once each side has `audit_samples`
//! epochs, a body whose median epoch is not `audit_margin_permille` below the interpreter's
//! retires, and its function stays interpreted (`Tiers::lane_body`). An epoch holds everything
//! the program did between those entries, so the comparison prices what the body costs where it
//! is entered (the entry's binding of the lane frame, its helpers' calls back into the VM) and
//! not only its own instructions; the median keeps a collection that lands in one epoch from
//! deciding it. Per function this holds the invariant that compiled code is never slower than
//! the interpreter, from the one statistic it is about.

use super::engine_types::{LaneBody, LaneView};
use crate::vm::tier_policy::PER_MILLE;

/// The CPU time the calling thread has run, in nanoseconds (`CLOCK_THREAD_CPUTIME_ID`); where
/// the clock cannot be read, the monotonic clock's time.
fn thread_cpu_nanos() -> u64 {
    #[cfg(unix)]
    {
        // SAFETY: `time` is a valid out-pointer for the call.
        let mut time: libc::timespec = unsafe { std::mem::zeroed() };
        if unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut time) } == 0 && time.tv_sec >= 0 {
            return time.tv_sec as u64 * 1_000_000_000 + time.tv_nsec as u64;
        }
    }
    static BASE: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    u64::try_from(BASE.get_or_init(std::time::Instant::now).elapsed().as_nanos()).unwrap_or(u64::MAX)
}

thread_local! {
    /// The view whose audit runs now (its address), or 0: audits run one at a time, so no
    /// audit's epochs hold another's alternation. Two bodies audited at once confound each
    /// other: a mutually recursive pair's audits interleaved, and the one whose body epochs
    /// fell on the other's interpreted epochs retired (f's body 172 us against 80 us
    /// interpreted, though both bodies together ran faster).
    static AUDITOR: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// The entries other bodies made, uncounted, since the auditor last counted one: past
    /// `AUDIT_LEASE_ENTRIES`, an auditor no longer entered (retired, or its program gone)
    /// gives way.
    static WAITED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

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
pub struct Epochs {
    started: Option<u64>,
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
    ///
    /// A `recursive` entry, made within `RECURSION_DEPTH` activations of another of the same
    /// function (`f` calling `f`, or `f` calling `g` calling `f`), counts nothing either: the
    /// body makes its own recursive calls directly, and a mutual recursion's through its
    /// callees' bodies, so its epochs never count them, and an interpreted epoch that did would
    /// hold a fraction of the body epoch's work (fib(10)'s 177 activations against the body's
    /// one entry retired a body 3.8x cheaper). Asked only while the audit runs.
    #[inline(always)]
    pub fn audit_entry(&self, counts_interpreted: bool, recursive: impl FnOnce() -> bool) -> bool {
        let mut history = self.tier().history.get();
        // `QUENCH_DEV=keep-bodies` runs every entry in the body, so a body's own cost can be measured.
        if history.audit.done || crate::trace::Dev::KeepBodies.on() {
            return false;
        }
        // Another body's audit runs: this entry runs the body, uncounted, until it ends.
        let me = std::ptr::from_ref(self) as usize;
        if AUDITOR.get() != me {
            let waited = WAITED.get();
            if AUDITOR.get() != 0 && waited < crate::vm::tier_policy::AUDIT_LEASE_ENTRIES {
                WAITED.set(waited + 1);
                return false;
            }
            // Taking the audit starts it afresh: epochs counted under another's ran beside it.
            AUDITOR.set(me);
            history.audit = Audit::default();
            self.tier().history.set(history);
            self.tier().audit_epochs.set(Epochs::default());
        }
        WAITED.set(0);
        let period = self.tiers.lane_body.audit_period;
        let seen = history.audit.seen;
        let interpreted = (seen / period) % SIDES as u32 == INTERPRETER as u32;
        if (interpreted && !counts_interpreted) || recursive() {
            return interpreted;
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
        let now = thread_cpu_nanos();
        let mut epochs = self.tier().audit_epochs.get();
        if let Some(started) = epochs.started
            && epoch > 0
        {
            let side = ((epoch - 1) % SIDES as u32) as usize;
            let filled = usize::from(epochs.filled[side]);
            if filled < MAX_EPOCHS {
                epochs.nanos[side][filled] = u32::try_from(now.saturating_sub(started)).unwrap_or(u32::MAX);
                epochs.filled[side] += 1;
            }
        }
        epochs.started = Some(now);
        self.tier().audit_epochs.set(epochs);
        let samples = usize::from(self.tiers.lane_body.audit_samples).min(MAX_EPOCHS);
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
        if AUDITOR.get() == std::ptr::from_ref(self) as usize {
            AUDITOR.set(0);
        }
        let median = |side: usize| {
            let mut nanos = epochs.nanos[side][..samples].to_vec();
            nanos.sort_unstable();
            u64::from(nanos[samples / 2])
        };
        let verdict = Verdict::of(median(BODY), median(INTERPRETER), self.tiers.lane_body.audit_margin_permille);
        self.tier().body_stats.audit.set(Some(verdict));
        // A loop region that calls it would leave to run it in the lane, a crossing the interpreter's
        // epochs do not time (`called_by_regions`): its body stays.
        if !verdict.retire || self.called_by_regions() {
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
    pub fn audit_restart(&self) {
        let mut history = self.tier().history.get();
        history.audit = Audit::default();
        self.tier().history.set(history);
        self.tier().audit_epochs.set(Epochs::default());
        self.tier().body_stats.audit.set(None);
    }
}

/// What an audit concluded: each side's median epoch and whether the body retires.
#[derive(Clone, Copy)]
pub struct Verdict {
    body_nanos: u64,
    interpreter_nanos: u64,
    pub retire: bool,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_within_the_margin_of_the_interpreter_is_kept() {
        assert!(!Verdict::of(900, 1000, 20).retire);
    }

    #[test]
    fn a_body_less_than_the_margin_below_the_interpreter_retires() {
        assert!(Verdict::of(990, 1000, 20).retire);
    }

    #[test]
    fn an_equal_body_retires() {
        assert!(Verdict::of(1000, 1000, 20).retire);
    }

    #[test]
    fn thread_cpu_nanos_is_monotone() {
        let first = thread_cpu_nanos();
        let mut work = 0u64;
        for i in 0..100_000u64 {
            work = work.wrapping_add(std::hint::black_box(i));
        }
        std::hint::black_box(work);
        let second = thread_cpu_nanos();
        assert!(second >= first);
    }
}
