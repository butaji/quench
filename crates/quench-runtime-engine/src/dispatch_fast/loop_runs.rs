//! What a loop site's entries pay back, and how long the interpreter runs it before the model
//! weighs its compile (`compiled::loop_model`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use crate::compiled::loop_model;

use crate::compiled::tier_policy::MAX_ENTRY_BACKOFF;
/// The entries passed untried after `declines` declined entries in a row.
fn backoff(declines: u8) -> u32 {
    (1u32 << u32::from(declines).min(MAX_ENTRY_BACKOFF)) - 1
}

/// The widest wait between a refused loop's re-asks, in back edges.
const MAX_REASK_WAIT: u32 = 1 << 16;

/// The epoch no body state carries: a site with no refusal holds it.
pub(crate) const NO_REFUSAL: u32 = u32::MAX;

#[quench_macros::ext(LoopSiteRunsOps)]
impl super::LoopSite {
    /// The bytecodes of one iteration, from the header to the backward jump.
    pub(crate) fn span(&self) -> u64 {
        u64::from(self.backedge - self.header + 1)
    }

    /// Weighs a native run that took `backedges` backward jumps against the entry it paid; `left` where it left inside
    /// the loop (`LoopTier::weigh_run`).
    pub(crate) fn note_run(&self, backedges: u64, left: bool) {
        self.declines_in_row.set(0);
        let mut tier = self.tier.borrow_mut();
        tier.weigh_run(self.tiers, backedges, left);
        self.settle(&mut tier);
    }

    /// Weighs an entry the guards declined: an entry paid for no iteration. The `k`th decline in
    /// a row passes the next `2^k - 1` entries (at most `MAX_ENTRY_BACKOFF`) to the interpreter
    /// untried, so a cause that persists is retried at doubling distances. The entries passed
    /// since the last decline weigh as declined ones too: each took the site's backward jump
    /// and ran no iteration natively, so the site retires where it would retrying every entry.
    pub(crate) fn note_decline(&self) {
        let before = self.declines_in_row.get();
        let declines = before.saturating_add(1);
        self.declines_in_row.set(declines);
        self.skipped_entries.set(backoff(declines));
        let mut tier = self.tier.borrow_mut();
        tier.weigh_declines(1 + backoff(before));
        self.settle(&mut tier);
    }

    /// Whether this entry passes to the interpreter untried, counting it off the backoff.
    #[inline(always)]
    pub(crate) fn skips_entry(&self) -> bool {
        let skipped = self.skipped_entries.get();
        if skipped == 0 { return false; }
        self.skipped_entries.set(skipped - 1);
        true
    }

    /// Stops entering native code once the entries cost more than the iterations they ran save.
    /// The check is due only once the entries pass `settle_at`, the count the last check found the
    /// spare covers: iterations only add, so until then every entry is repaid, and a run or decline
    /// that cannot change the decision costs one compare.
    fn settle(&self, tier: &mut super::loop_tier::LoopTier) {
        let entries = tier.entries();
        if entries <= tier.settle_at { return; }
        match loop_model::repaid_entries(self.tiers, self.span(), entries, tier.iterations()) {
            Some(spare) => tier.settle_at = entries.saturating_add(spare),
            None => self.entering.set(false),
        }
    }

    /// Whether a loop enclosing this one is admitted by the region analysis and still enters native code
    /// (`enclosing`): this loop then runs as plain jumps inside it, and does not warm up or enter its own
    /// region, which the enclosing loop's region covers. Once that loop stops entering (it retires, or its
    /// runs cost more than they save) this loop takes its own region again.
    pub(crate) fn inside_admitted_loop(&self) -> bool {
        // An enclosing site that stops entering never enters again (`entering` only falls, and
        // `is_admitted` is fixed at install), so a walk that finds none stays none.
        if !self.enclosed.get() { return false; }
        let inside = self.enclosing.get().is_some_and(|enclosing| {
            enclosing.iter().any(|outer| {
                // SAFETY: the sites are boxed with the lane view, which outlives the lane run that reads them
                // (`install_loop_sites`), and an enclosing site is another of them.
                let outer = unsafe { &**outer };
                outer.is_admitted() && outer.enters()
            })
        });
        self.enclosed.set(inside);
        inside
    }

    /// Whether the body's dispatch counts this header dispatch down to its plan (`body_wait`),
    /// counting it as an iteration of the loop either way.
    pub(crate) fn body_waits(&self) -> bool {
        self.body_seen.set(self.body_seen.get().saturating_add(1));
        let wait = self.body_wait.get();
        if wait == 0 { return false; }
        self.body_wait.set(wait - 1);
        true
    }

    /// The warm-up part of `interprets`, as a loop site's backward jump row takes it: true, with the
    /// same count `interprets` makes, while the site warms up; false, changing nothing, once warm,
    /// where `interprets` decides.
    #[inline(always)]
    pub(crate) fn warm_backedge(&self) -> bool {
        let (taken, needed) = (self.taken.get(), self.needed.get());
        if taken >= needed {
            return false;
        }
        self.taken.set(taken + 1);
        true
    }

    /// Whether the interpreter takes this backward jump: while the site warms up, and until the
    /// model, weighing the compile where the count reaches `needed`, finds it pays.
    #[inline(always)]
    pub(crate) fn interprets(&self) -> bool {
        let (taken, needed) = (self.taken.get(), self.needed.get());
        if taken < needed {
            self.taken.set(taken + 1);
            return true;
        }
        taken == needed && self.decide()
    }

    /// The model's decision at the end of the count: a compile that needs more jumps to pay
    /// raises `needed` to them and keeps interpreting; one that pays moves `taken` past it.
    #[cold]
    fn decide(&self) -> bool {
        let taken = self.taken.get();
        self.taken.set(taken.saturating_add(1));
        let required = loop_model::admission_taken(self.tiers, self.span());
        if required <= taken { return false; }
        self.needed.set(required);
        true
    }

    /// Refuses the site's compile while its function's body state is `epoch`: its back edges
    /// are plain jumps until the wait (back edges, doubling per refusal under one epoch) runs
    /// out, and the first refusal under a new epoch waits the warm-up's worth of jumps.
    pub(crate) fn refuse_warming(&self, own: &super::LaneView, warmup: u32) {
        let epoch = own.body_epoch();
        let same = self.refused_epoch.get() == epoch;
        let span = if same { self.refused_span.get().saturating_mul(2).min(MAX_REASK_WAIT) } else { warmup.max(1) };
        self.refused_view.set(std::ptr::from_ref(own));
        self.refused_epoch.set(epoch);
        self.refused_span.set(span);
        self.refused_wait.set(span);
    }

    /// Whether this back edge is a refused site's plain jump: one compare of the body epoch
    /// and, while the wait lasts, one decrement.
    #[inline(always)]
    pub(crate) fn refused_now(&self) -> bool {
        let refused = self.refused_epoch.get();
        // SAFETY: a refused site lies in the view that refused it, which outlives the site's runs.
        if refused == NO_REFUSAL || refused != unsafe { (*self.refused_view.get()).body_epoch() } { return false; }
        let wait = self.refused_wait.get();
        if wait == 0 { return false; }
        self.refused_wait.set(wait - 1);
        true
    }
}
