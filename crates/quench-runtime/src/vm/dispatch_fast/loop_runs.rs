//! What a loop site's entries pay back, and how long the interpreter runs it before the model
//! weighs its compile (`compiled::loop_model`).

use crate::vm::compiled::loop_model;

impl super::LoopSite {
    /// The bytecodes of one iteration, from the header to the backward jump.
    pub(super) fn span(&self) -> u64 {
        u64::from(self.backedge - self.header + 1)
    }

    /// Weighs a native run that took `backedges` backward jumps against the entry it paid.
    pub(super) fn note_run(&self, backedges: u64) {
        self.tier.borrow_mut().weigh_run(backedges);
        self.settle();
    }

    /// Weighs an entry the guards declined: an entry paid for no iteration.
    pub(super) fn note_decline(&self) {
        self.tier.borrow_mut().weigh_decline();
        self.settle();
    }

    /// Stops entering native code once the entries cost more than the iterations they ran save.
    fn settle(&self) {
        let tier = self.tier.borrow();
        if !loop_model::repays(self.span(), tier.entries(), tier.iterations()) {
            self.entering.set(false);
        }
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
        let required = loop_model::admission_taken(self.span());
        if required <= taken { return false; }
        self.needed.set(required);
        true
    }

    /// Whether the model has admitted the compile.
    pub(super) fn is_hot(&self) -> bool {
        self.taken.get() > self.needed.get()
    }
}
