//! A loop site's compilations (`LoopSite::tier`): its current code, the compile under way on
//! the code generator's thread, the element views it assumes, and the values earlier code
//! found were no `i32`.

use super::*;
use super::super::compiled::{ElementViews, LoopEntry, LoopRegion};

/// A loop site's compilations: the current code, if any, the compile under way, the element
/// views both assume (those the latest entry found), and the values earlier code found were
/// no `i32`, which the next compilation holds as `f64`s.
#[derive(Default)]
pub(super) struct LoopTier {
    views: ElementViews,
    pub(super) code: Option<Rc<crate::code_dependencies::PublishedCode<super::super::compiled::CompiledOsrRegion>>>,
    /// The compile under way, and the entries left before the next look for its code
    /// (`Tiers::lane_body`'s `compile_poll_interval`, as a lane body's).
    pending: Option<Box<super::super::compiled::PendingOsrRegion>>,
    wait: u16,
    pub(super) compilations: u32,
    /// Native runs of the region and the iterations they took: the loop's mean trip count.
    runs: u32,
    iterations: u64,
    /// Entries the guards declined: each paid for the entry's guards and ran no iteration.
    declined: u32,
    /// Each operation the code left at, with its leaves so far; and those whose int32
    /// speculation the next compilation drops (`lane_region`'s `generic`), sorted.
    leaves: Vec<(usize, u16)>,
    widened: Vec<usize>,
    /// The element reads among them that left again as Numbers, which the next compilation reads untyped.
    abandoned: Vec<usize>,
    /// The field stores the code left at often enough once, which the next compilation planned
    /// again from their sites' caches (`left_store`), sorted.
    refed: Vec<usize>,
    /// Entries the published code's guards declined in a row, and whether the next compilation
    /// versions no loop bound on entry (`RegionPlan::entry_bounds`): a loop that appends past its
    /// bound misses every versioned entry.
    entry_misses: u16,
    pub(super) unbounded: bool,
    /// Whether the current code (or the compile under way) is a b-jit region the default mode
    /// picked (`loop_model::base_region`), which moves to u-jit once its iterations repay that
    /// compile (`promote`).
    pub(super) base: bool,
    /// The iterations at which a b-jit region is due to move to u-jit (`loop_model::promotion_taken`),
    /// where the default mode picked b-jit for the region; none where u-jit never gets ahead.
    promote_at: Option<u64>,
    /// Whether the site moves from its b-jit region to u-jit (`promote`): the stencil code runs
    /// until the u-jit region publishes in its place.
    promoting: bool,
    /// The entries up to which the site is known to repay its native code (`LoopSite::settle`):
    /// the iterations only add, so until the entries pass this count the check is not due.
    pub(super) settle_at: u64,
}

impl LoopTier {
    pub(super) fn weigh_run(&mut self, backedges: u64) {
        self.runs = self.runs.saturating_add(1);
        self.entry_misses = 0;
        self.iterations = self.iterations.saturating_add(backedges);
    }

    /// Moves a b-jit region the default mode picked to u-jit once the loop's iterations reach
    /// `promote_at` (`loop_model::promotion_taken`): the stencil code goes, and the next entry compiles
    /// the u-jit region, where a compilation remains.
    pub(super) fn promote(&mut self, tiers: &Tiers) {
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
    pub(super) fn backedge_budget(&self, tiers: &Tiers) -> Option<u64> {
        if !self.base || self.compilations >= tiers.loops.max_compilations { return None; }
        self.promote_at.map(|at| at.saturating_sub(self.iterations))
    }

    /// Whether the u-jit region of a promoted site is still to start compiling.
    pub(super) fn promotion_due(&self) -> bool {
        self.promoting && self.pending.is_none()
    }

    /// Counts an entry the published code's guards declined: past `widen_after_leaves` in a row,
    /// the code goes and the next compilation versions no loop bound on entry, once.
    pub(super) fn missed_entry(&mut self, tiers: &Tiers) {
        if self.unbounded || self.compilations >= tiers.loops.max_compilations { return; }
        self.entry_misses += 1;
        if self.entry_misses < tiers.loops.widen_after_leaves { return; }
        self.unbounded = true;
        if let Some(code) = self.code.take() { code.retire(); }
    }

    pub(super) fn weigh_declines(&mut self, declines: u32) {
        self.declined = self.declined.saturating_add(declines);
    }

    /// The entries made, native runs and declined ones, and the iterations the runs took.
    pub(super) fn entries(&self) -> u64 { u64::from(self.runs) + u64::from(self.declined) }

    pub(super) fn iterations(&self) -> u64 { self.iterations }

    /// Iterations per native run, or `None` before the first run.
    /// The native runs of its code so far.
    pub(super) fn runs(&self) -> u64 {
        u64::from(self.runs)
    }

    pub(super) fn mean_trip(&self) -> Option<u64> {
        self.iterations.checked_div(u64::from(self.runs))
    }

    pub(super) fn is_published(&self) -> bool {
        self.code.as_ref().is_some_and(|code| code.is_published())
    }

    /// Whether published code can re-enter `region` at `pc` (`CompiledOsrRegion::can_run_from`).
    pub(super) fn resumable(&self, region: &LoopRegion, pc: u32) -> bool {
        self.code.as_ref().and_then(|code| code.lease()).is_some_and(|code| code.can_run_from(region, pc))
    }

    /// Makes the compile under way stale where its code names shape ids, which a shape
    /// compaction renumbers.
    pub(super) fn retire_named_shapes(&self) {
        if let Some(pending) = &self.pending { pending.retire_named_shapes(); }
    }

    /// Counts a run of the code that left at `pc`. An operation or element read left `widen_after_leaves`
    /// times joins the widened ones and the code goes, where a compilation remains: the next
    /// entry compiles the loop holding that operation's result as a double.
    pub(super) fn left_at(&mut self, tiers: &Tiers, pc: usize, read: bool) {
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
    pub(super) fn left_store(&mut self, tiers: &Tiers, pc: usize) {
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
    pub(super) fn select(&mut self, tiers: &Tiers, views: ElementViews) -> bool {
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
    pub(super) fn may_run(&self, tiers: &Tiers, policy: crate::MemoryPolicy) -> bool {
        self.is_published()
            || (self.compilations < tiers.loops.max_compilations && super::super::compiled::loop_tier_compiles(policy))
    }

    /// Whether this entry passes to the interpreter's jump while the code compiles: entries
    /// between looks count down, and a look publishes code that is ready.
    #[inline(always)]
    pub(super) fn awaits_code<H: Host>(&mut self, vm: &mut Vm<H>, site: &LoopSite) -> bool {
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
    fn poll_pending<H: Host>(&mut self, vm: &mut Vm<H>, site: &LoopSite) -> bool {
        let Some(pending) = self.pending.take() else { return false };
        if self.wait > 0 {
            self.wait -= 1;
            self.pending = Some(pending);
            return true;
        }
        use super::super::compiled::OsrCompile;
        let program = vm.programs.get(vm.active_program);
        match pending.poll(vm, program.as_deref()) {
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
    fn start<H: Host>(&mut self, started: Option<super::super::compiled::PendingOsrRegion>, vm: &mut Vm<H>, site: &LoopSite) {
        self.pending = started.filter(|pending| pending.watch(&mut vm.heap, vm.method_cache_atoms.readers(), &vm.programs)).map(Box::new);
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
    fn publish<H: Host>(&mut self, vm: &mut Vm<H>, region: super::super::compiled::CompiledOsrRegion) {
        // A promoted site's stencil code goes as the u-jit region takes its place.
        if let Some(replaced) = self.code.take() { replaced.retire(); }
        self.promoting = false;
        self.code = Some(vm.programs.publish_region(region, &mut vm.heap, vm.method_cache_atoms.readers()));
    }
}

/// Counts a compilation of `site`'s region, in the tier and, with diagnostics, at the site.
fn count_compilation(tier: &mut LoopTier, site: &LoopSite) {
    tier.compilations += 1;
    super::super::compiled::base_region::count(super::super::compiled::base_region::Event::Compiled);
    if site.diagnostics {
        site.compile_attempts.set(site.compile_attempts.get().saturating_add(1));
    }
}

/// The type each of the region's entry registers holds in the running activation's registers `r`, by
/// register: `Type::INT32` for an int32, `Type::NUMBER` for another Number, `Type::ANY` otherwise.
fn entry_types(region: &super::super::compiled::LoopRegion, r: Registers) -> Vec<crate::typed_ir::Type> {
    let entries: Vec<(crate::bytecode::Register, crate::typed_ir::Type)> = region.entry_registers().map(|register| {
        let value = r.get(register);
        let ty = if value.as_int().is_some() {
            crate::typed_ir::Type::INT32
        } else if value.as_number().is_some() {
            crate::typed_ir::Type::NUMBER
        } else {
            crate::typed_ir::Type::ANY
        };
        (register, ty)
    }).collect();
    let len = entries.iter().map(|(register, _)| usize::from(*register) + 1).max().unwrap_or(0);
    let mut types = vec![crate::typed_ir::Type::ANY; len];
    for (register, ty) in entries {
        types[usize::from(register)] = ty;
    }
    types
}

/// Starts compiling `site`'s region over the tier's element views from the running activation,
/// and looks for its code at once: a code generator without its own thread has it ready.
pub(super) fn loop_compile_in_lane<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    site: &LoopSite,
    region: &super::super::compiled::LoopRegion,
    tier: &mut LoopTier,
    number_arrays: &[(crate::bytecode::Register, crate::typed_ir::Type)],
    int_entries: &[(crate::bytecode::Register, crate::typed_ir::IntRange)],
    typed_captures: &[u16],
    int_captures: &[u16],
) {
    count_compilation(tier, site);
    // SAFETY: the lane's VM is live, and nothing borrows it across the call.
    let machine = unsafe { vm(cx) };
    // The tier of this compile: b-jit where the u-jit region is not yet ahead of it, counting
    // the interpreter's jumps and the native iterations the loop took so far.
    let base = super::super::compiled::loop_model::base_region(machine.programs.tiers(), site.span(),
        u64::from(site.taken.get()).saturating_add(tier.iterations));
    tier.base = base && !crate::jit_mode::base_regions();
    tier.promote_at = super::super::compiled::loop_model::promotion_taken(machine.programs.tiers(), site.span());
    let program = machine.programs.get(machine.active_program);
    let function = site.function();
    let helpers = super::super::compiled::process_lane_helpers::<H>();
    machine.host.before_native_compilation();
    // SAFETY: the program's views outlive the lane run.
    let own = unsafe { &*(*cx).views }.get(function as usize).and_then(super::super::program_store::derived_lane_view);
    let (native_field_loads, store_loads) = (native_field_loads(machine, own.filter(|_| !base), region), native_store_loads(machine, region));
    let types = entry_types(region, r);
    // A loop array's base is no entry register of the region: its string is sampled here too.
    let strings: Vec<crate::bytecode::Register> = region.entry_registers()
        .chain(region.arrays.iter().filter(|array| array.field.is_none()).map(|array| array.register))
        .filter(|register| machine.heap.is_string(r.get(*register))).collect();
    let operand_kinds = machine.region_operand_kinds(machine.active_program, function, region.header as usize..=region.backedge as usize);
    let frame_captures: Vec<(usize, usize)> = region.inlining.as_deref()
        .map(|inlining| inlining.frames.iter().map(|frame| machine.callee_capture_slots(frame.callee)).collect()).unwrap_or_default();
    // A construction site's plan, where the region may keep its receiver as its field values.
    let construct_plans = |function: u32, pc: u32| super::unit_targets_in_lane(cx, function, pc).first().and_then(|target| target.identity())
        .and_then(|callee| super::scalar_construct_plan(unsafe { vm(cx) }, callee));
    let observation = super::super::compiled::Observation { native_field_loads: &native_field_loads, store_loads: &store_loads, number_arrays, abandoned: &tier.abandoned, int_entries, typed_captures, int_captures,
        entry_types: &types, string_entries: &strings, operand_kinds: &operand_kinds, entry_bounds: !tier.unbounded, frame_captures: &frame_captures,
        construct_plans: Some(&construct_plans) };
    let policy = machine.heap.memory_policy();
    // A BaseJIT region inlines the calls its sites recorded.
    // A Cranelift region calls the callees its sites reached by their entries.
    let calls = program.as_deref().map(|program| if base { region_calls(cx, machine, program, function) }
        else { unit_region_calls(cx, machine, program, function) }).unwrap_or_default();
    // Stores run in place for the atoms no method cache names, while the VM and the program run specialized.
    let stores = program.as_deref().filter(|program| machine.specialized && program.specialized).map(|_| machine.method_cache_atoms.bits().clone());
    let started = program.as_deref().and_then(|program| super::super::compiled::start_osr_region(region, tier.views,
        &observation, (policy, machine.programs.tiers()), program, function, helpers,
        &mut |unit, table| machine.global_table(program, function, unit, table), (&tier.widened, stores.as_ref()), calls, base,
        // A Cranelift region folds Stable facts under its code's dependencies, as a lane body does (`Division::Online`).
        if base { None } else { Some(super::super::compiled::OsrOnline { atoms: unsafe { vm(cx) }.method_cache_atoms.bits().clone(),
            constants: unsafe { vm(cx) }.programs.constants(unsafe { vm(cx) }.active_program).to_vec(),
            // SAFETY: the lane run owns the VM, as for the global reads above.
            tables: Box::new(move |unit, fields, reads| super::super::compiled::FactTables::of(unsafe { vm(cx) }, function, unit, fields, reads, &|_, _| None)) }) }));
    if let Some(pending) = &started {
        let (watch, callees) = pending.native_calls();
        machine.programs.note_pending_native_calls(&watch, callees);
    }
    machine.host.after_native_compilation();
    tier.start(started, machine, site);
}

/// The calls of `function` that a BaseJIT region of it may inline: the callees its call sites
/// reached, which the lane would activate in place (`direct_callees_in_lane`).
fn region_calls<H: Host>(cx: *mut LaneContext<H>, machine: &mut Vm<H>, program: &ResidualProgram, function: u32) -> super::super::compiled::RegionCalls {
    // SAFETY: the program's views outlive the lane run.
    let own = unsafe { &*(*cx).views }.get(function as usize).and_then(super::super::program_store::derived_lane_view);
    let (Some(own), Some(body)) = (own, program.function_get(function as usize)) else { return Default::default() };
    let candidates = direct_callees_in_lane(machine, own, function, program, body);
    machine.region_calls(program, function, own, &candidates)
}

/// The calls of `function` a Cranelift region of it calls by their entries: the callees its call
/// sites reached, whose bodies are then in demand (`bind_callees`), as a lane body's are.
fn unit_region_calls<H: Host>(cx: *mut LaneContext<H>, machine: &mut Vm<H>, program: &ResidualProgram, function: u32) -> super::super::compiled::RegionCalls {
    // SAFETY: the program's views outlive the lane run.
    let own = unsafe { &*(*cx).views }.get(function as usize).and_then(super::super::program_store::derived_lane_view);
    let (Some(own), Some(body)) = (own, program.function_get(function as usize)) else { return Default::default() };
    // A callee with no published body yet is called through its entry cell, which holds its body
    // once that publishes: the region compiled before its callees still calls them natively.
    let candidates = direct_callees_in_lane(machine, own, function, program, body);
    for callees in &candidates { super::super::compiled::bind_region_callees(callees, own); }
    super::super::compiled::RegionCalls::direct(candidates)
}

/// The descriptors of the dynamic field reads of `region` native code makes itself.
fn native_field_loads<H: Host>(machine: &Vm<H>, own: Option<&super::LaneView>, region: &super::super::compiled::LoopRegion)
    -> Vec<Vec<super::super::compiled::NativeFieldLoad>> {
    let (properties_base, properties_len) = machine.heap.native_property_values();
    let available = machine.heap.object_record_base().is_some() && !properties_base.is_null() && properties_len > 0;
    // Each read's lane record hint names the storage form its receivers had (`own_field_hint`),
    // which code that embeds its shapes guards whole. Base regions read the live cache instead,
    // whose entries name no form (the caller passes no view for one).
    let reads: std::collections::HashMap<usize, usize> = region.field_reads().iter()
        .map(|(offset, index)| (*index, region.header as usize + *offset as usize)).collect();
    region.fields.iter().enumerate()
        .map(|(index, field)| if available && region.inlining.is_none() && region.is_dynamic_field(index) {
            let hint = own.zip(reads.get(&index)).and_then(|(view, pc)| view.own_field_hint(*pc));
            machine.native_field_load_descriptors(field.cache_site).into_iter()
                .map(|load| super::super::compiled::NativeFieldLoad { formed: hint, ..load }).collect()
        } else {
            Default::default()
        })
        .collect()
}

/// The descriptors of the stores of `region`, by cache site: native
/// code stores in place through the arms they give (`site_store_plan`), as a read does.
fn native_store_loads<H: Host>(machine: &Vm<H>, region: &super::super::compiled::LoopRegion)
    -> Vec<(u16, Vec<super::super::compiled::NativeFieldLoad>)> {
    let available = machine.heap.object_record_base().is_some() && region.inlining.is_none();
    // A receiver the region keeps stores through the same arms: a store with no read of its
    // field carries no projected slot (`Lowering::loop_stores`), and with no arm it would leave.
    let mut sites: Vec<u16> = region.dynamic_store_sites().chain(region.field_store_sites.iter().map(|store| store.cache_site)).collect();
    sites.sort_unstable();
    sites.dedup();
    sites.into_iter().filter(|_| available)
        .map(|site| (site, machine.native_field_load_descriptors(site))).collect()
}

/// The element views of `region`'s arrays on entry (`ElementViews`), each pushed to `entry`:
/// every one a record array, each written one a dense array whose elements a store replaces
/// in place (`Vm::try_replace_record_element`), no prototype claiming an index of it; or every
/// one a typed array a register holds whose elements the loop may address in place
/// (`Vm::typed_array_elements_in_lane`), which its code views at the loop's header. A mix, or
/// any other value, declines the entry; a string the loop only reads holds an empty slot.
pub(super) fn loop_arrays_in_lane<H: Host>(vm: &mut Vm<H>, r: Registers, region: &LoopRegion, entry: &mut LoopEntry)
    -> Option<ElementViews> {
    let value_of = |(register, field): (crate::bytecode::Register, Option<usize>), entry: &LoopEntry| match field {
        None => r.get(register),
        Some(index) => entry.field_value(region, index),
    };
    let doubles = !region.arrays.is_empty() && region.arrays.iter().all(|array| {
        vm.heap.record_array(value_of((array.register, array.field), &*entry)).is_some_and(|object| object.elements().double_values().is_some())
    });
    let mut views = None;
    for array in &region.arrays {
        let value = value_of((array.register, array.field), &*entry);
        // A string the loop only reads is no array: its element reads take the string row
        // (`Lowering::string_or_row`), and its slot holds no view, so the others keep their places.
        if !array.written && vm.heap.is_string(value) {
            entry.push_array(std::ptr::null_mut::<u64>(), 0, 0)?;
            continue;
        }
        let found = if vm.heap.record_array(value).is_some() && doubles {
            push_double_array(vm, value, array.written, entry)?;
            ElementViews::Double
        } else if vm.heap.record_array(value).is_some() {
            push_dense_array(vm, value, array.written, entry)?;
            ElementViews::Dense
        } else {
            // A typed array read through a field is no loop's hoisted view.
            array.field.is_none().then_some(())?;
            let (elements, length, kind) = vm.typed_array_elements_in_lane(value, array.written)?;
            entry.push_array(elements, length, length)?;
            ElementViews::Typed(Some(kind))
        };
        views = Some(views.map_or(Some(found), |seen: ElementViews| seen.common(found))?);
    }
    Some(views.unwrap_or_default())
}

/// Pushes the raw doubles of the record array `value`, whose backing is double
/// (`ElementViews::Double`), read in place; a `written` one must be dense, its backing owned
/// alone, and no prototype may claim an index of it, as `push_dense_array` requires.
fn push_double_array<H: Host>(vm: &mut Vm<H>, value: Value, written: bool, entry: &mut LoopEntry) -> Option<()> {
    if written && (vm.dense_array_length(value).is_none() || !vm.prototypes_hold_no_index(value)) {
        return None;
    }
    let elements = vm.heap.record_array(value)?.elements();
    if written && elements.owner_count() != 1 {
        return None;
    }
    let words = elements.double_values()?;
    entry.push_array(words.as_ptr().cast_mut(), words.len(), words.len())
}

/// Pushes the record array `value`'s elements; a `written` one must be dense, its backing
/// owned alone, and no prototype may claim an index of it: a store below the length or an
/// append at it then defines or replaces an own data element and nothing else
/// (`append_dense_element`, `fill_dense_hole`).
fn push_dense_array<H: Host>(vm: &mut Vm<H>, value: Value, written: bool, entry: &mut LoopEntry) -> Option<()> {
    if !written {
        // A region reads the values in place: a narrow backing widens first.
        vm.heap.widen_array_elements(value);
        let elements = vm.heap.record_array(value)?.elements().wide_values()?;
        return entry.push_array(elements.as_ptr().cast_mut(), elements.len(), elements.len());
    }
    if vm.dense_array_length(value).is_none() || !vm.prototypes_hold_no_index(value) {
        return None;
    }
    // A Number store logs nothing.
    let elements = vm.heap.record_array_store_mut_unlogged(value, Value::integer(0))?.elements_mut();
    if elements.owner_count() != 1 {
        return None;
    }
    let elements = super::super::index::mutable_array_elements(elements);
    entry.push_array(elements.as_mut_ptr(), elements.len(), elements.capacity())
}

/// The running activation's function in `code`, the context's program: its index, body and
/// decoded code, and the program's constants, from which a loop site's region is analyzed.
pub(super) fn loop_function_in_lane<H: Host>(cx: *mut LaneContext<H>, code: &ResidualProgram)
    -> Option<(u32, &crate::bytecode::Function, Vec<WideInstruction>, Vec<Value>)> {
    let machine = unsafe { vm(cx) };
    let function_index = machine
        .lane_stack
        .top_frame()
        .map_or_else(|| machine.frames[unsafe { (*cx).frame() }].function, |activation| activation.function());
    let function = code.function_get(function_index as usize)?;
    let constants = machine.programs.constants(unsafe { (*cx).program }).to_vec();
    Some((function_index, function, function.instructions().collect(), constants))
}

/// The plain region of `site`'s loop, which leaves at each method call the inliner declined
/// (`MethodCalls::Exit`).
#[inline(never)]
pub(super) fn method_call_region_in_lane<H: Host>(cx: *mut LaneContext<H>, site: &LoopSite) -> Option<LoopRegion> {
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let (function_index, function, decoded, constants) = loop_function_in_lane(cx, code)?;
    // A native method the loop holds inline is run in place by the lane (`intrinsic_call`), so its call
    // is no exit of the region (`native_method_call`). The lane run owns the VM here.
    // A member of a Stable global read by name (`Math.abs`) is one too (`Vm::native_callees`).
    let machine = unsafe { vm(cx) };
    let members = machine.native_callees(code, function_index, &decoded);
    let natives: Vec<bool> = (0..decoded.len())
        .map(|pc| super::super::compiled::native_method_call(machine, &decoded, pc)
            || members[pc].is_some_and(|(_, native, _)| super::super::compiled::inlines_in_loop_call(native,
                crate::bytecode::ImmediateLayout::argument_count(decoded[pc].imm())))).collect();
    super::super::compiled::analyze_osr_region_detailed(function, &decoded, &constants, &code.field_sites,
        code.shared_register_root_masks(function_index), site.header, site.backedge,
        super::super::compiled::MethodCalls::Exit, &|pc| natives.get(pc).copied().unwrap_or(false)).ok()
}

/// The depth-zero capture slots the running function reads that hold typed arrays now (an observation
/// the compiled loop's entry checks): a loop indexing such a capture views it as a typed array.
pub(super) fn typed_capture_slots<H: Host>(cx: *mut LaneContext<H>) -> Vec<u16> {
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let Some((_, function, _, _)) = loop_tier::loop_function_in_lane(cx, code) else { return Vec::new() };
    let mut slots: Vec<u16> = function.instructions()
        .filter(|instruction| instruction.op() as u16 == crate::staged::LOAD_CAPTURE_ROW && instruction.capture_depth() == 0)
        .map(|instruction| crate::bytecode::ImmediateLayout::capture_slot(instruction.imm())).collect();
    slots.sort_unstable();
    slots.dedup();
    slots.retain(|slot| super::read_capture_value::<H, true>(cx, 0, *slot)
        .is_some_and(|value| unsafe { vm(cx) }.typed_array_elements_in_lane(value, false).is_some()));
    slots
}

/// The depth-zero capture slots the running function reads that hold int32s now (an observation
/// the compiled loop's entry guard checks): a loop's integer arithmetic on such a capture stays int32.
pub(super) fn int_capture_slots<H: Host>(cx: *mut LaneContext<H>) -> Vec<u16> {
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let Some((_, function, _, _)) = loop_function_in_lane(cx, code) else { return Vec::new() };
    let mut slots: Vec<u16> = function.instructions()
        .filter(|instruction| instruction.op() as u16 == crate::staged::LOAD_CAPTURE_ROW && instruction.capture_depth() == 0)
        .map(|instruction| crate::bytecode::ImmediateLayout::capture_slot(instruction.imm())).collect();
    slots.sort_unstable();
    slots.dedup();
    slots.retain(|slot| super::read_capture_value::<H, true>(cx, 0, *slot).is_some_and(|value| value.as_int().is_some()));
    slots
}

/// Whether `instruction` is a `LoadCapture`: the shared decode's answer, with its result flags
/// set aside.
fn is_capture_load(instruction: &crate::bytecode::WideInstruction) -> bool {
    matches!(
        crate::vm::compiled::decoded_completed(instruction),
        Some(crate::staged::Operation::LoadCapture { .. })
    )
}

/// How many elements of an entry array are sampled for Numbers.
const SAMPLED_ELEMENTS: usize = 64;

/// The registers of `region`'s arrays whose first elements, sampled on entry, are all int32s or all
/// Numbers (int32 or double, no hole), with that type: an observation (a guess the code checks at
/// every read), not a proof.
pub(super) fn number_arrays<H: Host>(vm: &Vm<H>, r: Registers, region: &LoopRegion) -> Vec<(Register, crate::typed_ir::Type)> {
    region.arrays.iter().filter(|array| array.field.is_none())
        .filter_map(|array| sample_array(vm, r.get(array.register)).map(|ty| (array.register, ty))).collect()
}

/// The type the first elements of the array `value` all have, int32 or Number (no hole), sampled:
/// an observation (a guess the code checks at every read), not a proof.
/// The array-typed values among `values` (register order), sampled: what a lane body's loops read typed.
pub(super) fn sampled_arrays<H: Host>(vm: &Vm<H>, values: impl Iterator<Item = Value>) -> Vec<(crate::bytecode::Register, crate::typed_ir::Type)> {
    values.enumerate().filter_map(|(register, value)| sample_array(vm, value).map(|ty| (register as crate::bytecode::Register, ty))).collect()
}

pub(super) fn sample_array<H: Host>(vm: &Vm<H>, value: Value) -> Option<crate::typed_ir::Type> {
    use crate::typed_ir::Type;
    // A float typed array's kind fixes its elements as Numbers, as a C array's element type does.
    if let Some(kind) = vm.typed_array_kind(value) {
        use crate::heap::TypedArrayKind as Kind;
        return matches!(kind, Kind::Float16 | Kind::Float32 | Kind::Float64).then_some(Type::NUMBER);
    }
    let elements = vm.heap.record_array(value)?.elements().as_slice();
    let sample = &elements[..elements.len().min(SAMPLED_ELEMENTS)];
    if sample.is_empty() { return None; }
    if sample.iter().all(|value| value.as_int().is_some()) { Some(Type::INT32) }
    else { sample.iter().all(|value| value.as_number().is_some()).then_some(Type::NUMBER) }
}

/// The registers of the running activation that hold int32s at entry and that the loop writes,
/// each with the range its value gives the entry (`IntRange::entering`): an observation the
/// compiled code's entry contract checks, as `number_arrays`'.
pub(super) fn int_entries(r: Registers, region: &LoopRegion) -> Vec<(Register, crate::typed_ir::IntRange)> {
    region.entry_registers().filter_map(|register| r.get(register).as_int()
        .map(|value| (register, crate::typed_ir::IntRange::entering(value)))).collect()
}
