//! What a loop site's native runs pay back of their entries.

impl super::LoopSite {
    /// Weighs a native run that took `backedges` backward jumps: a run that took none paid its
    /// entry for no iteration and counts toward `TIERS.loops.max_exits`, and each iteration
    /// a run took pays one such entry back.
    pub(super) fn note_run(&self, backedges: u64) {
        let exits = self.exits.get();
        self.exits.set(match u32::try_from(backedges) {
            Ok(0) => exits.saturating_add(1),
            Ok(paid) => exits.saturating_sub(paid),
            Err(_) => 0,
        });
    }
}
