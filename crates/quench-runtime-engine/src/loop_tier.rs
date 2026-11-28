//! A loop site's compilations (`LoopSite::tier`): its current code, the compile under way on
//! the code generator's thread, the element views it assumes, and the values earlier code
//! found were no `i32`.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use crate::compiled::{ElementViews, LoopEntry, LoopRegion};

pub(crate) use quench_runtime::vm::engine_types::LoopTier;

#[quench_macros::ext(LoopTierOps1)]
impl LoopTier {
    /// Whether published code can re-enter `region` at `pc` (`CompiledOsrRegion::can_run_from`).
    pub(crate) fn resumable(&self, region: &LoopRegion, pc: u32) -> bool {
        self.code.as_ref().and_then(|code| code.lease()).is_some_and(|code| code.can_run_from(region, pc))
    }

    /// Makes the compile under way stale where its code names shape ids, which a shape
    /// compaction renumbers.
    pub(crate) fn retire_named_shapes(&self) {
        if let Some(pending) = &self.pending { crate::compiled::pending_osr_ref(&**pending).retire_named_shapes(); }
    }

    /// Whether code over the element `views` an entry found may run: typed code of no kind runs
    /// over any typed kind; code over any other views goes where a compilation remains, which the
    /// entry then makes over these, or over no kind where two kinds were found.
    pub(crate) fn select(&mut self, tiers: &Tiers, views: ElementViews) -> bool {
        let common = self.views.common(views);
        if common == Some(self.views) { return true; }
        if self.compilations >= tiers.loops.max_compilations { return false; }
        self.views = common.unwrap_or(views);
        if let Some(code) = self.code.take() { code.retire(); }
        true
    }

    /// Whether the site has native code or may still compile it. A site whose compilations
    /// ran out, or under a policy that compiles no loop regions, takes its backward jump in
    /// the interpreter without reading an entry: reading one at every iteration had cost
    /// V8-v7 Crypto `--jit` a third of its instructions under memory-first.
    pub(crate) fn may_run(&self, tiers: &Tiers, vm: &Vm) -> bool {
        self.is_published()
            || (self.compilations < tiers.loops.max_compilations && crate::compiled::admission::loop_regions_compile(vm))
    }

    /// Whether this entry passes to the interpreter's jump while the code compiles: entries
    /// between looks count down, and a look publishes code that is ready.
    #[inline(always)]
    pub(crate) fn awaits_code(&mut self, vm: &mut Vm, site: &LoopSite) -> bool {
        // A site with published code, or none under way, has nothing to wait for. A look still
        // counting down is one decrement here; only the look itself leaves the inline path.
        if self.pending.is_none() { return false; }
        if self.wait > 0 {
            self.wait -= 1;
            return true;
        }
        self.poll_pending(vm, site)
    }

    #[cold]
    fn poll_pending(&mut self, vm: &mut Vm, site: &LoopSite) -> bool {
        let Some(pending) = self.pending.take() else { return false };
        if self.wait > 0 {
            self.wait -= 1;
            self.pending = Some(pending);
            return true;
        }
        use crate::compiled::OsrCompile;
        let program = vm.programs.get(vm.active_program);
        match crate::compiled::pending_osr(pending).poll(vm, program.as_deref()) {
            Err(pending) => {
                self.wait = site.tiers.lane_body.compile_poll_interval;
                self.pending = Some(pending);
                return true;
            }
            Ok(OsrCompile::Done(region)) => self.publish(vm, region),
            Ok(OsrCompile::Stale) => {}
            Ok(OsrCompile::Failed) => {
                // A region that lowered no code (it is lowered off the loop's thread) or whose
                // code failed retires the site, as one declined at the start does.
                self.compilations = site.tiers.loops.max_compilations;
                if !self.is_published() { site.entering.set(false); }
            }
        }
        if site.diagnostics && self.is_published() {
            site.compile_successes.set(site.compile_successes.get().saturating_add(1));
        }
        false
    }

    /// Holds the compile `started`, while the facts its code names hold, and looks for its
    /// code at once.
    fn start(&mut self, started: Option<crate::compiled::PendingOsrRegion>, vm: &mut Vm, site: &LoopSite) {
        self.pending = started.filter(|pending| { let (mut facts, programs) = vm.fact_watch(); pending.watch(&mut facts, programs) }).map(|pending| Box::new(pending) as Box<dyn std::any::Any>);
        self.wait = 0;
        if self.pending.is_none() {
            self.compilations = site.tiers.loops.max_compilations;
            // A declined or failed region retires the site now: later entries build no entry
            // record. A promoted site keeps entering the stencil code it has.
            if !self.is_published() { site.entering.set(false); }
        }
        self.awaits_code(vm, site);
    }

    /// Publishes `region` while the facts its code names hold (`ProgramStore::publish_region`).
    fn publish(&mut self, vm: &mut Vm, region: crate::compiled::CompiledOsrRegion) {
        // A promoted site's stencil code goes as the u-jit region takes its place.
        if let Some(replaced) = self.code.take() { replaced.retire(); }
        self.code = Some({ let (mut facts, programs) = vm.fact_watch(); programs.publish_region(region, &mut facts) });
    }
}

