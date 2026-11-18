//! What a loop site's entries pay back, and how long the interpreter runs it before the model
//! weighs its compile (`compiled::loop_model`).

use crate::vm::compiled::loop_model;

/// The widest backoff after declined entries: `2^16 - 1` entries pass untried between attempts.
const MAX_ENTRY_BACKOFF: u32 = 16;

/// The entries passed untried after `declines` declined entries in a row.
fn backoff(declines: u8) -> u32 {
    (1u32 << u32::from(declines).min(MAX_ENTRY_BACKOFF)) - 1
}

impl super::LoopSite {
    /// The bytecodes of one iteration, from the header to the backward jump.
    pub(super) fn span(&self) -> u64 {
        u64::from(self.backedge - self.header + 1)
    }

    /// Weighs a native run that took `backedges` backward jumps against the entry it paid.
    pub(super) fn note_run(&self, backedges: u64) {
        self.declines_in_row.set(0);
        self.tier.borrow_mut().weigh_run(backedges);
        self.settle();
    }

    /// Weighs an entry the guards declined: an entry paid for no iteration. The `k`th decline in
    /// a row passes the next `2^k - 1` entries (at most `MAX_ENTRY_BACKOFF`) to the interpreter
    /// untried, so a cause that persists is retried at doubling distances. The entries passed
    /// since the last decline weigh as declined ones too: each took the site's backward jump
    /// and ran no iteration natively, so the site retires where it would retrying every entry.
    pub(super) fn note_decline(&self) {
        let before = self.declines_in_row.get();
        let declines = before.saturating_add(1);
        self.declines_in_row.set(declines);
        self.skipped_entries.set(backoff(declines));
        self.tier.borrow_mut().weigh_declines(1 + backoff(before));
        self.settle();
    }

    /// Whether this entry passes to the interpreter untried, counting it off the backoff.
    #[inline(always)]
    pub(super) fn skips_entry(&self) -> bool {
        let skipped = self.skipped_entries.get();
        if skipped == 0 { return false; }
        self.skipped_entries.set(skipped - 1);
        true
    }

    /// Stops entering native code once the entries cost more than the iterations they ran save.
    fn settle(&self) {
        let tier = self.tier.borrow();
        if !loop_model::repays(self.tiers, self.span(), tier.entries(), tier.iterations()) {
            self.entering.set(false);
        }
    }

    /// Whether a loop enclosing this one is admitted by the region analysis and still enters native code
    /// (`enclosing`): this loop then runs as plain jumps inside it, and does not warm up or enter its own
    /// region, which the enclosing loop's region covers. Once that loop stops entering (it retires, or its
    /// runs cost more than they save) this loop takes its own region again.
    pub(super) fn inside_admitted_loop(&self) -> bool {
        self.enclosing.get().is_some_and(|enclosing| {
            enclosing.iter().any(|outer| {
                // SAFETY: the sites are boxed with the lane view, which outlives the lane run that reads them
                // (`install_loop_sites`), and an enclosing site is another of them.
                let outer = unsafe { &**outer };
                outer.is_admitted() && outer.enters()
            })
        })
    }

    /// Whether the interpreter takes this backward jump: while the site warms up, and until the
    /// model, weighing the compile where the count reaches `needed`, finds it pays.
    #[inline(always)]
    pub(super) fn interprets(&self) -> bool {
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

    /// Whether the model has admitted the compile.
    pub(super) fn is_hot(&self) -> bool {
        self.taken.get() > self.needed.get()
    }
}
