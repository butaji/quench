//! Whether a loop site runs inside an enclosing admitted loop's region.

#[allow(unused_imports)]
use crate::ext_traits::*;

#[quench_macros::ext(LoopSiteRunsOps)]
impl super::LoopSite {
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
}
