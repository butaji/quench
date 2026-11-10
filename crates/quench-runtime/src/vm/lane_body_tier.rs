//! When a lane body compiles again, keeps its code or retires: decisions from what its runs
//! and its predecessors' did.

use super::{Handler, LaneBody, LaneLeaves, LaneView, TIERS};
use super::super::compiled::LaneExit;

/// A view's lane-body record over the run, which a lifecycle edge keeps: the bodies such
/// edges (a shape compaction, a dead native callee) dropped, and those of them no run of
/// which completed (`LaneView::drop_body`). Each dropped body doubles the next one's warm-up,
/// so a function whose bodies keep being discarded stops paying a compile for each;
/// `TIERS.lane_body.max_lifetimes` dropped bodies, decline it.
#[derive(Clone, Copy, Default)]
pub(super) struct LaneBodyHistory {
    pub(super) lifetimes: u8,
    /// The inlining budget halvings its last plan needed to fit (`PendingLaneMethod::shrinks`).
    shrinks: u8,
    /// The warm-ups its compile waited for its callees' bodies (`LaneView::waits_for_callees`).
    callee_waits: u8,
    /// The bodies it specialized past because a callee published one (`respecialize_callers`).
    callee_recompiles: u8,
}

/// The base `TIERS.lane_body.reoptimize_share` is a share of.
const PERCENT: u64 = 100;

/// The entries and the milliseconds of warming past which a function specializes by its
/// recurrence alone (`LaneView::recurrent`).
const RECURRENT_ENTRIES: u16 = 16;
const RECURRENT_TICKS: u32 = 256;

/// The warming entries every function has counted so far: a clock of the work the program does
/// while its functions warm, which, unlike wall time, a rerun repeats.
fn warm_ticks() -> u32 {
    thread_local! { static TICKS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) }; }
    TICKS.with(|ticks| { let next = ticks.get().wrapping_add(1); ticks.set(next); next })
}

/// The reading `warm_ticks` would give next, without counting.
fn warm_clock() -> u32 { warm_ticks().wrapping_sub(1) }

/// What a body whose leaves reached the reoptimization threshold does next: it specializes
/// again, these places generically, until `max_recompiles` bodies have; the last of them has
/// every place any body left at generic, and is permanent. A body never retires to the lane.
fn verdict(recompiles: u8, leaves: LaneLeaves, stuck: LaneLeaves) -> Option<LaneLeaves> {
    // Leaves only where the predecessors' bodies left, and those places already generic,
    // would plan the same body again: it stays.
    let nothing_new = leaves != LaneLeaves::Scattered && stuck != LaneLeaves::Scattered
        && leaves.exits().all(|exit| stuck.exits().any(|known| known == exit));
    (recompiles < TIERS.lane_body.max_recompiles && !(recompiles > 0 && nothing_new)).then(|| stuck.union(leaves))
}

impl LaneView {
    /// Counts how a run of the specialized body ended, at `exit`; at
    /// `TIERS.lane_body.reoptimize_exits` leaves, doubled per recompile, it takes its
    /// `verdict`. A body it compiles past keeps its code for the direct callers that name it.
    pub(super) fn note_lane_body_end(&self, exit: LaneExit, completed_region: bool) {
        // SAFETY: as `lane_body`; the run that ended has finished, and nothing reads its body.
        let body = unsafe { &mut *self.tier_lane.get() };
        let LaneBody::Specialized { exits, leaves_at, completed, completions, recompiles, tier_prologue, stuck, .. } = body else { return };
        if completed_region || matches!(exit, LaneExit::Returned { .. }) {
            completed.set(true);
            completions.set(completions.get().saturating_add(1));
            return;
        }
        exits.set(exits.get().saturating_add(1));
        leaves_at.set(leaves_at.get().with(exit));
        // Leaves past the first `reoptimize_exits` that exceed `reoptimize_share` percent of the runs.
        let (leaves, runs) = (u64::from(exits.get()), u64::from(exits.get()) + u64::from(completions.get()));
        if exits.get() < TIERS.lane_body.reoptimize_exits || leaves * PERCENT <= runs * u64::from(TIERS.lane_body.reoptimize_share) {
            return;
        }
        let Some(stuck) = verdict(*recompiles, leaves_at.get(), *stuck) else {
            // A body that keeps leaving where no other body would, and has never completed a
            // run, gains nothing from its entry: its function runs in the lane again.
            let costly = runs >= u64::from(TIERS.lane_body.retire_after_runs) && leaves * PERCENT > runs * u64::from(TIERS.lane_body.retire_share);
            if !completed.get() || costly {
                // SAFETY: as `claim_for_tier`; the prologue's handler runs no more for this entry.
                unsafe { (*self.prologue.get()).handler = *tier_prologue };
                let LaneBody::Specialized { method, .. } = std::mem::replace(body, LaneBody::Declined) else { unreachable!() };
                self.retired.borrow_mut().push(method);
                self.release_from_tier();
            }
            return;
        };
        let next = LaneBody::Unspecialized { entries: 0, recompiles: *recompiles + 1, stuck };
        // SAFETY: as `claim_for_tier`; the prologue's handler runs no more for this entry.
        unsafe { (*self.prologue.get()).handler = *tier_prologue };
        let LaneBody::Specialized { method, .. } = std::mem::replace(body, next) else { unreachable!() };
        self.retired.borrow_mut().push(method);
    }

    /// The entries before its next body specializes: the warm-up, doubled per dropped body.
    pub(super) fn warmup_entries(&self) -> u16 {
        TIERS.lane_body.warmup_entries << self.history.get().lifetimes.min(7)
    }

    /// Whether a function entered rarely has still been entered at least `RECURRENT_ENTRIES` times
    /// over `RECURRENT_TICKS` warming entries of the program: an entry that runs long (a driver whose callees
    /// are hot) costs more than the many cheap entries the warm-up counts, and its callees'
    /// bodies run from the lane until it specializes and inlines them. Read every
    /// `RECURRENT_ENTRIES`th entry.
    fn recurrent(&self, entries: u16) -> bool {
        entries >= RECURRENT_ENTRIES && entries % RECURRENT_ENTRIES == 0 && self.history.get().lifetimes == 0
            && warm_clock().wrapping_sub(self.warm_since.get()) >= RECURRENT_TICKS
    }

    /// Whether `entries` is a count at which its function weighs a compile: the warm-up, then
    /// each doubling of it, so a function that has not paid for its compile yet looks again
    /// when it has done twice the work.
    fn attempts_at(&self, entries: u16) -> bool {
        let warmup = self.warmup_entries();
        entries % warmup == 0 && (entries / warmup).is_power_of_two()
    }

    /// Whether its history affords another compile.
    pub(super) fn affords_compile(&self) -> bool {
        let history = self.history.get();
        history.lifetimes <= TIERS.lane_body.max_lifetimes
    }

    /// Whether its compile waits one more warm-up for callees still warming, whose bodies it
    /// then calls by their entries (`compiled::DirectCallee`).
    pub(super) fn waits_for_callees(&self) -> bool {
        let history = self.history.get();
        let waits = history.callee_waits < TIERS.lane_body.max_callee_waits;
        if waits {
            self.history.set(LaneBodyHistory { callee_waits: history.callee_waits + 1, ..history });
        }
        waits
    }

    /// Records that `caller`'s body compiled a call of this function, by its entry or through
    /// the lane's helper: a fact of this view's body, whose publication invalidates it.
    pub(super) fn note_caller(&self, caller: &LaneView) {
        let caller = std::ptr::from_ref(caller);
        let mut callers = self.callers.borrow_mut();
        if !std::ptr::eq(caller, self) && !callers.contains(&caller) { callers.push(caller); }
    }

    /// Its body published: each caller that compiled against the one before, or against
    /// none, specializes again at its next entry and calls this one by its entry
    /// (`compiled::DirectCallee`). A caller's running body keeps its code (`retired`).
    fn respecialize_callers(&self) {
        for caller in self.callers.take() {
            // SAFETY: a program's views outlive its lane runs, and callers are views of it.
            let caller = unsafe { &*caller };
            let history = caller.history.get();
            // SAFETY: as `lane_body`; a caller's running body holds its own lease.
            let body = unsafe { &mut *caller.tier_lane.get() };
            let LaneBody::Specialized { tier_prologue, recompiles, stuck, .. } = body else { continue };
            if history.callee_recompiles >= TIERS.lane_body.max_callee_recompiles { continue; }
            caller.history.set(LaneBodyHistory { callee_recompiles: history.callee_recompiles + 1, ..history });
            // SAFETY: as `claim_for_tier`; the prologue's handler runs no more for this entry.
            unsafe { (*caller.prologue.get()).handler = *tier_prologue };
            let next = LaneBody::Unspecialized { entries: TIERS.lane_body.warmup_entries - 1, recompiles: *recompiles, stuck: *stuck };
            let LaneBody::Specialized { method, .. } = std::mem::replace(body, next) else { unreachable!() };
            caller.retired.borrow_mut().push(method);
        }
    }

    /// A compile a lifecycle edge outran counts as a dropped body: no run
    /// of it took place.
    pub(super) fn note_dropped_compile(&self) {
        let history = self.history.get();
        self.history.set(LaneBodyHistory { lifetimes: history.lifetimes.saturating_add(1), ..history });
    }

    /// A specialized body that went at a lifecycle edge warms again from the start, keeping
    /// where it and its predecessors left, and its recompiles, as its successor's feedback: a call site's behavior
    /// outlives the shape ids the edge renumbered. One no run of which completed counts as
    /// wasted (`LaneBodyHistory`).
    pub(super) fn drop_body(&self, body: &mut LaneBody) {
        let LaneBody::Specialized { leaves_at, stuck, recompiles, .. } = body else { return };
        let (stuck, recompiles) = (stuck.union(leaves_at.get()), *recompiles);
        let history = self.history.get();
        self.history.set(LaneBodyHistory {
            lifetimes: history.lifetimes.saturating_add(1),
            ..history
        });
        *body = LaneBody::Unspecialized { entries: 0, recompiles, stuck };
    }

    /// The body a just published method makes of this view.
    pub(super) fn specialized(&self, entry: Handler, method: std::rc::Rc<super::super::compiled::PublishedLaneMethod>, recompiles: u8, stuck: LaneLeaves) -> LaneBody {
        // A body whose entry would not pay for itself serves native callers only
        // (`lane_body_call`, direct calls); the lane's own entries keep the records, and it
        // has no trial to run.
        let tier_prologue = if method.lease().expect("a body just published is live").amortizes_entry() {
            // SAFETY: as `claim_for_tier`; no handler reads the prologue now.
            unsafe { std::mem::replace(&mut (*self.prologue.get()).handler, entry as *const ()) }
        } else {
            self.release_from_tier();
            unsafe { (*self.prologue.get()).handler }
        };
        use std::cell::Cell;
        LaneBody::Specialized { method, tier_prologue, completions: Cell::new(0), exits: Cell::new(0),
            leaves_at: Cell::new(LaneLeaves::NONE), completed: Cell::new(false), recompiles, stuck }
    }

    /// Whether its body's code is compiling (`LaneBody::Compiling`).
    pub(super) fn compiling(&self) -> bool {
        // SAFETY: as `lane_body`.
        matches!(unsafe { &*self.tier_lane.get() }, LaneBody::Compiling { .. })
    }

    /// Counts an entry toward the frameless body and specializes it at the warm-up; whether
    /// the body is still warming or specialized, so its prologue stays. A specialized body is
    /// a fact of the view from then on: its prologue record enters it with `entry`, which
    /// tests nothing the specialization proved (`lane_body_entry`).
    pub(super) fn specialize_lane_body(&self, entry: Handler, work: impl FnOnce(super::super::compiled::LaneBodyWork) -> super::super::compiled::LaneBodyOutcome) -> bool {
        self.settle_retired_body();
        // SAFETY: as `lane_body`; specializing runs no lane body, and the body is taken out
        // of the view while the VM works on it.
        let body = unsafe { &mut *self.tier_lane.get() };
        if awaits_look(body) {
            return true;
        }
        let (outcome, entries, recompiles, stuck, looks) = match std::mem::replace(body, LaneBody::Declined) {
            LaneBody::Unspecialized { entries, recompiles, stuck } if self.attempts_at(entries.saturating_add(1)) || self.recurrent(entries.saturating_add(1)) => {
                if !self.affords_compile() {
                    *body = LaneBody::Unspecialized { entries, recompiles, stuck };
                    return false;
                }
                // A predecessor's few leaves are the specialization's feedback (`stuck`).
                let generic: Vec<_> = stuck.exits().collect();
                (work(super::super::compiled::LaneBodyWork::Start { generic: &generic, shrinks: self.history.get().shrinks, observed: self.attempts_at(entries.saturating_add(1)).then(|| u32::from(entries) + 1) }), entries, recompiles, stuck, 0)
            }
            LaneBody::Unspecialized { entries, recompiles, stuck } => {
                let ticks = warm_ticks();
                if entries == 0 { self.warm_since.set(ticks); }
                *body = LaneBody::Unspecialized { entries: entries.saturating_add(1), recompiles, stuck };
                return true;
            }
            LaneBody::Compiling { pending, recompiles, stuck, looks, .. } =>
                (work(super::super::compiled::LaneBodyWork::Poll(pending)), 0, recompiles, stuck, looks.saturating_add(1)),
            settled => { *body = settled; return !matches!(body, LaneBody::Declined); }
        };
        *body = self.body_after(outcome, entry, entries, recompiles, stuck, looks);
        !matches!(body, LaneBody::Declined)
    }

    /// The body a view's compile work came to (`LaneBodyOutcome`) makes of it; a compile in
    /// flight records the inlining its plan fits (`LaneBodyHistory::shrinks`).
    fn body_after(&self, outcome: super::super::compiled::LaneBodyOutcome, entry: Handler, entries: u16, recompiles: u8, stuck: LaneLeaves, looks: u8) -> LaneBody {
        match outcome {
            super::super::compiled::LaneBodyOutcome::Compiling(pending) => {
                self.history.set(LaneBodyHistory { shrinks: pending.shrinks(), ..self.history.get() });
                let wait = 1u16.checked_shl(u32::from(looks)).map_or(u16::MAX, |entries| entries - 1).min(TIERS.lane_body.compile_poll_interval);
                LaneBody::Compiling { pending, wait: std::cell::Cell::new(wait), looks, recompiles, stuck }
            }
            super::super::compiled::LaneBodyOutcome::Published(published) => {
                self.respecialize_callers();
                self.specialized(entry, published, recompiles, stuck)
            }
            // A compile a collection or a changed fact outran is a body a lifecycle edge dropped.
            super::super::compiled::LaneBodyOutcome::Stale => {
                self.note_dropped_compile();
                LaneBody::Unspecialized { entries: 0, recompiles, stuck }
            }
            super::super::compiled::LaneBodyOutcome::Deferred => LaneBody::Unspecialized { entries: entries.saturating_add(1), recompiles, stuck },
            super::super::compiled::LaneBodyOutcome::Declined => LaneBody::Declined,
        }
    }

    /// `specialize_lane_body` at once, past the warm-up: a hot loop's backward jump has run
    /// its calls, so their sites have linked (`lane_loop_retire`).
    pub(super) fn specialize_lane_body_now(&self, entry: Handler, specialize: impl FnOnce(super::super::compiled::LaneBodyWork) -> super::super::compiled::LaneBodyOutcome) {
        self.skip_warm_up();
        self.specialize_lane_body(entry, specialize);
        // Its entries are few (the loop is what is hot): the next one compiles where this one
        // waited for its callees' bodies (`LaneBodyOutcome::Deferred`).
        self.skip_warm_up();
    }

    /// A function whose bodies lifecycle edges discarded warms as any entry does.
    fn skip_warm_up(&self) {
        // SAFETY: as `lane_body`; no lane body runs here.
        if let LaneBody::Unspecialized { entries, .. } = unsafe { &mut *self.tier_lane.get() }
            && self.history.get().lifetimes == 0
        {
            *entries = TIERS.lane_body.warmup_entries - 1;
        }
    }
}

/// The target of the callee the call at `pc` of `view` last reached itself, by the site's own
/// record (`lane_site_targets`): the entry its record links is another callee's once the
/// target cache replaced it.
pub(super) fn site_target<H: super::Host>(vm: &super::Vm<H>, view: &LaneView, pc: usize) -> Option<super::ScriptCallTarget> {
    let callee = super::lane_site_targets(vm, view, pc as u32).first().filter(|site| site.through().is_none())?.identity()?;
    // SAFETY: as `Vm::script_call_entry`; the entry is copied before anything resolves a target.
    vm.script_call_entry(callee).map(|target| unsafe { *target })
}

/// `site_target` for a compile of `view`'s body, which the callee's view records
/// (`LaneView::note_caller`).
pub(super) fn compiled_site_target<H: super::Host>(vm: &super::Vm<H>, view: &LaneView, pc: usize) -> Option<super::ScriptCallTarget> {
    let target = site_target(vm, view, pc)?;
    // SAFETY: a resolved target's view outlives its program.
    if let Some(callee) = unsafe { target.view.as_ref() } { callee.note_caller(view); }
    Some(target)
}

/// What a first compile of `view` comes to before any planning. Entry warm-up leaves closed
/// loops to their admitted regions while those still enter native code (a region that leaves
/// for calls still lets a lane body inline those callees into its typed unit). A function
/// whose callees still compile waits for their bodies, which it then calls by their entries
/// (`compiled::DirectCallee`), and `waits_for_callees` for those still counting entries.
pub(super) fn start_gate<H: super::Host>(vm: &super::Vm<H>, view: &LaneView, body: Option<&crate::bytecode::Function>) -> Option<super::super::compiled::LaneBodyOutcome> {
    use super::super::compiled::LaneBodyOutcome;
    if view.loops.iter().any(|site| site.call_boundary_pcs.is_empty() && site.active_region().is_some() && site.enters()) {
        return Some(LaneBodyOutcome::Declined);
    }
    let body = body?;
    let calls = |pending: fn(&LaneView) -> bool| body.code.iter().enumerate().any(|(pc, packed)| {
        super::decoded_instruction(body, *packed).op().is_call()
            && site_target(vm, view, pc).is_some_and(|target| {
                // SAFETY: a resolved target's view outlives its program.
                target.program == vm.active_program && !target.view.is_null() && !std::ptr::eq(target.view, view) && pending(unsafe { &*target.view })
            })
    });
    (calls(LaneView::compiling) || (calls(LaneView::warming) && view.waits_for_callees())).then_some(LaneBodyOutcome::Deferred)
}

/// Whether a compiling `body` lets this entry pass without looking for its code, counting it
/// down: a compile takes milliseconds and a hot loop enters at each backward jump.
fn awaits_look(body: &LaneBody) -> bool {
    let LaneBody::Compiling { wait, .. } = body else { return false };
    let left = wait.get();
    wait.set(left.saturating_sub(1));
    left > 0
}
