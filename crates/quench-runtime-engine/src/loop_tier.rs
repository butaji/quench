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
    /// Weighs a run that took `backedges` backward edges; `left` where it left inside the loop rather than at its
    /// exit. Runs that each leave before their first backward edge, `widen_after_leaves` in a row, pay their entry
    /// for nothing: the code goes and the site compiles no more, so its backward jump stays the interpreter's.
    /// A run that keeps the code may move it to u-jit (`promote`).
    pub(crate) fn weigh_run(&mut self, tiers: &Tiers, backedges: u64, left: bool) {
        self.runs = self.runs.saturating_add(1);
        // A run does not clear `entry_misses`: a site whose published code misses its entry guard and then
        // runs another way (the interpreter or a lower tier) would otherwise reset the count on every outer
        // iteration and never widen (v_array_push_grow: a fresh array each outer trip, ~2,350 Ir per miss).
        self.iterations = self.iterations.saturating_add(backedges);
        self.fruitless = if left && backedges == 0 { self.fruitless.saturating_add(1) } else { 0 };
        self.tier_runs = self.tier_runs.saturating_add(1);
        self.tier_left = self.tier_left.saturating_add(u32::from(left));
        if self.demotes(tiers) {
            // Measured: the u-jit region leaves more often than the b-jit region it replaced, so each of its
            // runs pays more exits than its code saves. The site goes back to b-jit for good.
            self.demoted = true;
            self.base = true;
            self.promote_at = None;
            if let Some(code) = self.code.take() { code.retire(); }
        }
        if self.fruitless >= tiers.loops.widen_after_leaves {
            self.compilations = tiers.loops.max_compilations;
            if let Some(code) = self.code.take() { code.retire(); }
        }
        self.promote(tiers);
    }

    /// The run share that left inside the loop, per mille, of the current code's runs.
    fn left_permille(&self) -> u32 {
        (u64::from(self.tier_left) * quench_runtime::vm::tier_policy::PER_MILLE / u64::from(self.tier_runs.max(1))) as u32
    }

    /// Whether the u-jit region a promotion published has left more often than the b-jit region it
    /// replaced (`base_left_permille`), over a window of `loops.warmup_taken` runs, where a compilation remains.
    fn demotes(&self, tiers: &Tiers) -> bool {
        let Some(base) = self.base_left_permille else { return false };
        !self.base && !self.demoted && self.compilations < tiers.loops.max_compilations
            && self.tier_runs >= tiers.loops.warmup_taken && self.left_permille() > base
    }

    /// Moves a b-jit region the default mode picked to u-jit once the loop's iterations reach
    /// `promote_at` (`loop_model::promotion_taken`): the stencil code goes, and the next entry compiles
    /// the u-jit region, where a compilation remains.
    pub(crate) fn promote(&mut self, tiers: &Tiers) {
        let due = self.promote_at.is_some_and(|at| self.iterations >= at);
        if !self.base || self.compilations >= tiers.loops.max_compilations || !due { return; }
        // The stencil code keeps running while the u-jit region compiles (`promoting`): the
        // entry that finds it ready publishes it in the stencil code's place.
        self.base = false;
        self.promoting = true;
    }

    /// The backward edges a run of the current code may take before it leaves to move to u-jit
    /// (`LoopEntry::set_backedge_budget`): for a b-jit region the default mode picked, the
    /// iterations left until the u-jit region is ahead of it; none otherwise.
    pub(crate) fn backedge_budget(&self, tiers: &Tiers) -> Option<u64> {
        if !self.base || self.compilations >= tiers.loops.max_compilations { return None; }
        self.promote_at.map(|at| at.saturating_sub(self.iterations))
    }

    /// Counts an entry the published code's guards declined: past `widen_after_leaves` over the code's life,
    /// the code goes and the next compilation versions no loop bound on entry, once.
    pub(crate) fn missed_entry(&mut self, tiers: &Tiers) {
        if self.unbounded || self.compilations >= tiers.loops.max_compilations { return; }
        self.entry_misses += 1;
        if self.entry_misses < tiers.loops.widen_after_leaves { return; }
        self.unbounded = true;
        if let Some(code) = self.code.take() { code.retire(); }
    }

    /// A callee its code calls by the helper got a body after it compiled (`record_late_call`): the
    /// code goes where a compilation remains, and the next one calls the callee by its entry.
    pub(crate) fn late_callee(&mut self, tiers: &Tiers) -> bool {
        let retires = self.compilations < tiers.loops.max_compilations && self.code.is_some();
        if retires && let Some(code) = self.code.take() { code.retire(); }
        retires
    }

    pub(crate) fn weigh_declines(&mut self, declines: u32) {
        self.declined = self.declined.saturating_add(declines);
    }

    /// The entries made, native runs and declined ones, and the iterations the runs took.
    pub(crate) fn entries(&self) -> u64 { u64::from(self.runs) + u64::from(self.declined) }

    pub(crate) fn iterations(&self) -> u64 { self.iterations }

    /// Iterations per native run, or `None` before the first run.
    /// The native runs of its code so far.
    pub(crate) fn runs(&self) -> u64 {
        u64::from(self.runs)
    }

    pub(crate) fn mean_trip(&self) -> Option<u64> {
        self.iterations.checked_div(u64::from(self.runs))
    }

    /// Whether published code can re-enter `region` at `pc` (`CompiledOsrRegion::can_run_from`).
    pub(crate) fn resumable(&self, region: &LoopRegion, pc: u32) -> bool {
        self.code.as_ref().and_then(|code| code.lease()).is_some_and(|code| code.can_run_from(region, pc))
    }

    /// Makes the compile under way stale where its code names shape ids, which a shape
    /// compaction renumbers.
    pub(crate) fn retire_named_shapes(&self) {
        if let Some(pending) = &self.pending { crate::compiled::pending_osr_ref(&**pending).retire_named_shapes(); }
    }

    /// Counts a run of the code that left at `pc`. An operation or element read left `widen_after_leaves`
    /// times joins the widened ones and the code goes, where a compilation remains: the next
    /// entry compiles the loop holding that operation's result as a double.
    pub(crate) fn left_at(&mut self, tiers: &Tiers, pc: usize, read: bool) {
        let widened = self.widened.binary_search(&pc).is_ok();
        if !(read || !widened) || self.abandoned.binary_search(&pc).is_ok() || self.compilations >= tiers.loops.max_compilations { return; }
        let at = self.leaves.iter().position(|(left, _)| *left == pc).unwrap_or_else(|| { self.leaves.push((pc, 0)); self.leaves.len() - 1 });
        self.leaves[at].1 += 1;
        if self.leaves[at].1 < tiers.loops.widen_after_leaves { return; }
        self.leaves.swap_remove(at);
        let list = if widened { &mut self.abandoned } else { &mut self.widened };
        list.push(pc);
        list.sort_unstable();
        if let Some(code) = self.code.take() { code.retire(); }
    }

    /// Counts a run that left at the field store at `pc`, whose in-place arms missed
    /// (`Lowering::store_leaves`). The first time it has left `widen_after_leaves` times the code
    /// goes and the next compilation plans the store again from what its cache learned since
    /// (a store the loop had not run yet had no plan); after that it widens as `left_at` does.
    pub(crate) fn left_store(&mut self, tiers: &Tiers, pc: usize) {
        if self.refed.binary_search(&pc).is_ok() { return self.left_at(tiers, pc, false); }
        if self.compilations >= tiers.loops.max_compilations { return; }
        let at = self.leaves.iter().position(|(left, _)| *left == pc).unwrap_or_else(|| { self.leaves.push((pc, 0)); self.leaves.len() - 1 });
        self.leaves[at].1 += 1;
        if self.leaves[at].1 < tiers.loops.widen_after_leaves { return; }
        self.leaves.swap_remove(at);
        self.refed.push(pc);
        self.refed.sort_unstable();
        if let Some(code) = self.code.take() { code.retire(); }
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
                self.promoting = false;
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
            self.promoting = false;
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
        // The b-jit region's measured exits, which the u-jit region taking its place is held to (`demotes`).
        if self.promoting { self.base_left_permille = Some(self.left_permille()); }
        (self.tier_runs, self.tier_left) = (0, 0);
        self.promoting = false;
        self.code = Some({ let (mut facts, programs) = vm.fact_watch(); programs.publish_region(region, &mut facts) });
    }
}

