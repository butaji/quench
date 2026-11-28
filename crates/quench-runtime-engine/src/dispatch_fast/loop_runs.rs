//! What a loop site's entries pay back, and how long the interpreter runs it before the model
//! weighs its compile (`compiled::loop_model`).

#[allow(unused_imports)]
use crate::ext_traits::*;

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
