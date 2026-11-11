//! A lane body's two halves: planning, which reads the VM (call targets, field proofs, direct
//! callees) and lowers the unit, and code generation, which reads nothing of it. The plan
//! keeps everything the method holds but its code, so code generation may run on the code
//! generator's thread (`jit_backend::run_in_background`) while the program runs on.

use super::*;
use std::collections::BTreeMap;
use super::super::program_store::lane_publication::LaneKey;

type LaneCode = Result<Vec<u8>, super::jit_backend::NativeCompileRejection>;

/// A lowered lane body: the method it becomes once its code exists.
pub(super) struct LanePlan {
    unit: lane_unit::Unit,
    lowered: lane_region::LoweredLane,
    fields: BTreeMap<u16, Vec<NativeFieldLoad>>,
    direct: Box<[DirectCallee]>,
    /// Whether every call of it that has run is inlined or direct (`LaneMethod::closed`).
    closed: bool,
    function: u32,
    /// The word its publication sets when one of its Stable facts changes.
    invalidation: crate::code_dependencies::Invalidation,
    /// The callee instructions its unit inlined at most (`lane_unit::with_budget`).
    budget: usize,
}

/// A plan whose code compiles on the code generator's thread. It holds while the facts it
/// took hold, under the lifecycle the published method has: `watch` subscribes to its slot
/// types and Stable facts from the plan on, a collection that frees a callee it names
/// retires it (`ProgramStore::note_pending_native_calls`), and so does a shape compaction
/// where it names shapes (`retire_named_shapes`). A collection alone leaves it valid: an
/// allocation-heavy loop collects more often than a body compiles.
pub(in crate::vm) struct PendingLaneMethod {
    plan: LanePlan,
    /// The leaves it was planned from (`LaneBodyWork::Start`), which a plan again reuses.
    generic: Vec<LaneExit>,
    watch: Rc<crate::code_dependencies::PublishedCode<()>>,
    code: super::jit_backend::Pending<LaneCode>,
    /// What it compiles from (`LanePlan::key`).
    key: LaneKey,
}

/// What a pending body's compile came to.
enum LaneCompile {
    /// Its code needed more memory than a body may take: plan again from its feedback,
    /// inlining half its budget.
    Replan(Vec<LaneExit>, usize),
    Done(Option<LaneMethod>),
}

/// What a function's lane body asks of the VM at an entry past its warm-up.
pub(in crate::vm) enum LaneBodyWork<'a> {
    /// Plan a body from a predecessor's leaves (`generic`) and start compiling its code, its
    /// inlining budget halved `shrinks` times (`PendingLaneMethod::shrinks`).
    Start { generic: &'a [LaneExit], shrinks: u8, observed: Option<u32>, called: bool, caller: &'a super::super::dispatch_fast::LaneView,
        /// The activation's array parameters sampled at entry, with their element type (`loop_tier::sample_array`).
        arrays: &'a [(crate::bytecode::Register, crate::typed_ir::Type)] },
    /// Look for the code of the body compiling.
    Poll(Box<PendingLaneMethod>),
}

/// Why a body did not start compiling.
enum Unstarted {
    /// A body published under the same key still holds.
    Installed(Rc<PublishedLaneMethod>),
    /// Not yet worth compiling.
    Unpaid,
    /// Too short for its entries to amortize (`amortizes_entry`), and no body calls it.
    Short,
    Declined,
}

/// What the VM did for it.
pub(in crate::vm) enum LaneBodyOutcome {
    Compiling(Box<PendingLaneMethod>),
    Published(Rc<PublishedLaneMethod>),
    /// A collection ran while its code compiled: its proofs no longer hold, and it warms again.
    Stale,
    /// Its function has not run long enough to pay for the compile, or callees it would call
    /// directly still warm toward their bodies: it counts on.
    Deferred,
    /// Its function's entries have not yet paid for the compile (`pays_compile`).
    Unpaid,
    /// Its unit is too short to amortize an entry and no body calls it: declined until a caller's
    /// compile meets a call to it (`LaneView::demand_body`).
    Short,
    Declined,
}

/// A replanned body inlines with this fraction of the budget whose compile failed.
const REPLAN_BUDGET_DIVISOR: usize = 2;

/// The IR size above which a lane body's registers are allocated in one pass. The backtracking
/// allocator's scratch memory grows with the function, and a body this large (JetStream-scale
/// units of 23k-28k IR instructions) set the process's peak resident size while its code
/// compiled: V8-v7 raytrace peaked at 40.0 MB with it and 33.0 MB without, richards and
/// deltablue, whose largest bodies are 17k, keep it and the speed it buys (2026-11-11, lane
/// mode, 5 interleaved runs). Smaller bodies keep backtracking: single-pass for all lost 20-30%
/// on richards and deltablue.
const LANE_BACKTRACKING_MAX_INSTS: usize = 20_000;

/// The code of one lane body, compiled on the calling thread.
fn lane_code(function: ir::Function, level: super::jit_backend::CompileLevel) -> LaneCode {
    let level = match level {
        super::jit_backend::CompileLevel::Optimized if function.dfg.num_insts() > LANE_BACKTRACKING_MAX_INSTS =>
            super::jit_backend::CompileLevel::Large,
        level => level,
    };
    let isa = super::jit_backend::target_isa_level(level)
        .ok_or(super::jit_backend::NativeCompileRejection::Publish)?;
    compile_native_function(function, &*isa, lane_region::LANE_BODY_MAX_CODE_BYTES, |code| Some(code.to_vec()))
}

use super::super::dispatch_fast::lane_body_stats::{miss as plan_miss, Miss};
use lane_facts::{unit_closed, bind_callees, Census, pays_compile, plan_key, amortizes_entry, each_once, entry_pc, numeric_callee, numeric_restartable, unit_direct_calls, unit_leaves};

/// The unit PCs a previous body left at, and whether it left at its entry. A leave at the first
/// instruction is a wrong entry speculation: the next body drops its entry contract
/// (`LaneExit::Entry`) and lowers the instruction as any other.
fn left_places(unit: &lane_unit::Unit, generic: &[LaneExit], first: usize) -> (Vec<usize>, bool) {
    let mut left_at = unit_leaves(unit, generic);
    let entry_left = left_at.contains(&first) || generic.contains(&LaneExit::Entry);
    left_at.retain(|pc| *pc != first);
    (left_at, entry_left)
}

impl<H: Host> Vm<H> {
    /// The call targets a unit binds at `caller`'s `pc`. A call the body left at takes no
    /// inlined callee: its call binds what the inlined frame could not. A pure numeric callee
    /// binds its definition at the edge, as AOT does, so fresh closures of it are one case;
    /// one that is not this realm's own definition makes the unit's bindings invalid.
    fn lane_bindings(&self, program: &ResidualProgram, function: u32, generic: &[LaneExit],
        targets: &dyn Fn(u32, u32) -> Vec<CallTarget>, valid: &std::cell::Cell<bool>, caller: u32, pc: u32) -> Vec<CallTarget> {
        if caller == function && generic.contains(&LaneExit::At(pc as usize)) { return Vec::new(); }
        let mut bound = targets(caller, pc);
        for target in &mut bound {
            // A forwarded target keeps its identity: the site calls it through its native.
            if !numeric_callee(program, target.function) || target.through().is_some() { continue; }
            if let Some(callee) = target.identity()
                && !self.same_realm_user_target(callee).is_some_and(|(owner, function, _)|
                    owner == self.active_program && function == target.function) {
                valid.set(false);
            }
            target.callee = CalleeBinding::Function;
        }
        each_once(bound)
    }

    /// The unit of `function` inlining at most `budget` callee instructions, if each frame
    /// it inlines is admitted.
    fn lane_plan_unit(&mut self, program: &ResidualProgram, function: u32, targets: &dyn Fn(u32, u32) -> Vec<CallTarget>,
        generic: &[LaneExit], budget: usize) -> Option<lane_unit::Unit> {
        let body = program.function_get(function as usize)?;
        let valid = std::cell::Cell::new(true);
        let unit = lane_unit::with_budget(program, function, body, budget,
            &|caller, pc| self.lane_bindings(program, function, generic, targets, &valid, caller, pc),
            &|callee| program.function_get(callee as usize)).or_else(|| { plan_miss(Miss::PlanNoUnit); None })?;
        if !valid.get() { plan_miss(Miss::PlanInvalidBindings); return None; }
        unit.frames.iter().all(|frame| match frame.target.callee {
            CalleeBinding::Identity(callee) | CalleeBinding::Forwarded { callee, .. } => self.lane_inlined_target_admitted(program, callee, frame.target.function),
            CalleeBinding::Function => frame.recovery == lane_unit::CallRecovery::RestartCall
                && numeric_restartable(program, frame.target.function),
        }).then_some(unit).or_else(|| { plan_miss(Miss::PlanFrameNotAdmitted); None })
    }

    /// Plans `function`'s lane body at `budget`: its unit, proofs and lowering, and the
    /// function the code generator compiles. `None` where it does not lower or runs nothing
    /// natively.
    #[allow(clippy::too_many_arguments)]
    fn plan_lane_method(&mut self, program: &ResidualProgram, function: u32, targets: &dyn Fn(u32, u32) -> Vec<CallTarget>,
        captures: &dyn Fn(Value) -> (usize, usize), direct: &[DirectCandidates],
        generic: &[LaneExit], budget: usize, entry: Option<u32>,
        arrays: &[(crate::bytecode::Register, crate::typed_ir::Type)]) -> Option<(LanePlan, ir::Function)> {
        let unit = self.lane_plan_unit(program, function, targets, generic, budget)?;
        let entry = entry_pc(&unit, entry).or_else(|| { plan_miss(Miss::PlanNoEntry); None })?;
        let (left_at, entry_left) = left_places(&unit, generic, entry.unwrap_or(0));
        let mut fields = self.lane_unit_fields(program, &unit, &left_at);
        // Admitting the stores' writes once, here, widens what they widen; the fields then
        // read the widened slots, and the body's entries admit nothing again.
        if !self.admit_lane_stores(lane_region::store_domains(&unit.instructions, |site| fields.get(&site).map(Vec::as_slice))) { fields = self.lane_unit_fields(program, &unit, &left_at); }
        let global_reads = self.lane_global_reads(program, function, &unit);
        let (direct, direct_sites) = unit_direct_calls(&unit, direct);
        let closed = unit_closed(&unit, function, |pc| direct_sites.contains_key(&pc), targets);
        let frame_captures: Vec<(usize, usize)> = unit.frames.iter()
            .map(|frame| frame.target.identity().map_or((0, 0), captures)).collect();
        let helpers = LaneHelper::addresses::<H>();
        crate::jit_coverage::note_native_helpers(helpers);
        let helpers = LaneHelpers::Process(helpers);
        // The body folds Stable facts under its own invalidation word, which its publication
        // sets when one changes (`ProgramStore::publish_lane_method`).
        let invalidation = crate::code_dependencies::Invalidation::default();
        let method_cache_atoms = self.method_cache_atoms.bits().clone();
        let active = self.active_program;
        let constants = self.programs.residual(active).filter(|running| std::ptr::eq(*running, program))
            .map_or(&[][..], |_| self.programs.constants(active));
        let facts = VmFacts { online: Some(OnlineFacts { method_cache_atoms: &method_cache_atoms,
            invalidation: invalidation.address(), constants }), ..self.vm_facts() };
        let leaves: Vec<usize> = left_at.iter().copied().chain(entry_left.then(|| lane_region::entry_leave(&unit))).chain(self.adding_stores(&unit)).collect();
        let number_reads = super::osr_compile::observed_reads(&unit, arrays, &[], &left_at);
        let mut lowered = lane_region::lower_body(program, function, &unit, helpers, &fields, &global_reads, facts,
            &leaves, &direct_sites, &frame_captures, entry, &number_reads).or_else(|| { plan_miss(Miss::PlanNotLowered); None })?;
        if lowered.region.native == 0 { plan_miss(Miss::PlanNothingNative); return None; }
        let native = std::mem::replace(&mut lowered.region.function, ir::Function::new());
        Some((LanePlan { unit, lowered, fields, direct, closed, function, invalidation, budget }, native))
    }

    /// Specialize `function` for lane activations, admitting a body only where its first
    /// instruction runs natively: an entry that leaves at once is pure cost. Code that needs
    /// more memory than a body may take is planned again inlining less.
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(in crate::vm) fn compile_lane_method(&mut self, program: &ResidualProgram, function: u32,
        targets: &dyn Fn(u32, u32) -> Vec<CallTarget>, captures: &dyn Fn(Value) -> (usize, usize),
        direct: Vec<DirectCandidates>, generic: &[LaneExit]) -> Option<LaneMethod> {
        self.host.before_native_compilation();
        let mut budget = lane_unit::MAX_UNIT_INLINED_INSTRUCTIONS;
        let method = loop {
            let Some((plan, native)) = self.plan_lane_method(program, function, targets, captures, &direct, generic, budget, None, &[])
            else { break None };
            match lane_code(native, super::jit_backend::CompileLevel::run_default()) {
                Ok(code) => break plan.finish(&code),
                Err(reason) if plan.replans(reason) => budget /= 2,
                Err(_) => break None,
            }
        };
        self.host.after_native_compilation();
        method
    }

    /// Plans `function`'s lane body at `budget`, again with less inlined where its IR would make
    /// more code than a body may take, before anything compiles: compiling it first costs all
    /// of its compile.
    #[allow(clippy::too_many_arguments)]
    fn plan_lane_method_sized(&mut self, program: &ResidualProgram, function: u32,
        targets: &dyn Fn(u32, u32) -> Vec<CallTarget>, captures: &dyn Fn(Value) -> (usize, usize),
        direct: &[DirectCandidates], generic: &[LaneExit], mut budget: usize, entry: Option<u32>,
        arrays: &[(crate::bytecode::Register, crate::typed_ir::Type)]) -> Option<(LanePlan, ir::Function)> {
        loop {
            self.host.before_native_compilation();
            let planned = self.plan_lane_method(program, function, targets, captures, direct, generic, budget, entry, arrays);
            self.host.after_native_compilation();
            let (plan, native) = planned?;
            let oversized = native.dfg.num_insts() * lane_region::LANE_BODY_CODE_BYTES_PER_IR > lane_region::LANE_BODY_MAX_CODE_BYTES;
            if !oversized || plan.unit.frames.is_empty() || budget == 0 { return Some((plan, native)); }
            budget /= REPLAN_BUDGET_DIVISOR;
        }
    }

    /// Plans `function`'s lane body at `budget` and starts compiling its code on the code
    /// generator's thread (`PendingLaneMethod`).
    #[allow(clippy::too_many_arguments)]
    fn start_lane_method(&mut self, program: &ResidualProgram, function: u32,
        targets: &dyn Fn(u32, u32) -> Vec<CallTarget>, captures: &dyn Fn(Value) -> (usize, usize),
        direct: &[DirectCandidates], generic: &[LaneExit], budget: usize, entry: Option<u32>, observed: Option<u32>, called: bool, caller: &super::super::dispatch_fast::LaneView,
        arrays: &[(crate::bytecode::Register, crate::typed_ir::Type)]) -> Result<PendingLaneMethod, Unstarted> {
        let (plan, native) = self.plan_lane_method_sized(program, function, targets, captures, direct, generic, budget, entry, arrays)
            .ok_or(Unstarted::Declined)?;
        // A body that cannot amortize a lane entry would serve native callers only
        // (`LaneView::specialized`); the lane's warm-up is not one, so it declines here rather
        // than compile code no caller may ever run.
        if entry.is_none() && !called && !amortizes_entry(&plan.unit, function, &plan.direct) { return Err(Unstarted::Short); }
        bind_callees(&plan.direct, caller);
        let key: LaneKey = (self.active_program.raw(), function, entry, plan_key(&plan.unit, &plan.fields, plan.direct.len(), generic, entry, budget, arrays));
        if let Some(published) = self.programs.installed_lane(&key) { return Err(Unstarted::Installed(published)); }
        if let Some(entries) = observed && !pays_compile(program, function, entries, native.dfg.num_insts()) { return Err(Unstarted::Unpaid); }
        if !self.programs.admit_lane_compile(&key, TIERS.lane_body.max_compiles) { plan_miss(Miss::PlanCompileBound); return Err(Unstarted::Declined); }
        let watch = Rc::new(crate::code_dependencies::PublishedCode::new(()));
        let watcher: Rc<dyn crate::code_dependencies::DependentCode> = watch.clone();
        if !plan.dependencies().subscribe(&plan.lowered.region.stable,
            &watcher, &mut self.heap, self.method_cache_atoms.readers()) {
            return Err(Unstarted::Declined);
        }
        let census = self.programs.js_jit_diagnostics_enabled().then(|| Census {
            function, osr: entry.is_some(), unit: plan.unit.instructions.len(), ir: native.dfg.num_insts(), key: key.3,
            generic: generic.len(), direct: plan.direct.len(), budget, observed, closed: plan.closed });
        let code = super::jit_backend::run_in_background(move || {
            let started = std::time::Instant::now();
            let code = lane_code(native, super::jit_backend::CompileLevel::run_default());
            if let Some(census) = census { eprintln!("{}", census.report(started.elapsed(), code.as_ref().map_or(0, |code| code.len()))); }
            code
        });
        self.programs.note_pending_native_calls(&watch, plan.callees());
        Ok(PendingLaneMethod { plan, generic: generic.to_vec(), watch, code, key })
    }

    /// The unit PCs of stores whose sites added their property to some receiver (they hold
    /// an add entry): each lowers as the store a previous body left at does, through the VM's
    /// adding entries (`LaneHelper::FieldAdd`), which a direct own-slot store cannot be.
    fn adding_stores<'a>(&'a self, unit: &'a lane_unit::Unit) -> impl Iterator<Item = usize> + 'a {
        unit.instructions.iter().enumerate().filter(|(_, instruction)| instruction.op().field_store().is_some()
            && self.field_add_indices.get(self.field_cache_index(instruction.cache_site_index())) != crate::vm::NO_FIELD_ADD_SET)
            .map(|(pc, _)| pc)
    }

    /// Does `work` for `function`'s lane body (`LaneBodyWork`), whose direct callees
    /// `direct` gives; publishes a body whose code compiled while its proofs held. An `entry`
    /// (a caller PC) plans the body to start there, as a running loop enters it.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::vm) fn lane_body_work(&mut self, program: &ResidualProgram, function: u32,
        targets: &dyn Fn(u32, u32) -> Vec<CallTarget>, captures: &dyn Fn(Value) -> (usize, usize),
        direct: &dyn Fn(&Self) -> Vec<DirectCandidates>, work: LaneBodyWork<'_>, entry: Option<u32>) -> LaneBodyOutcome {
        let (mut observed, mut called, mut caller, mut arrays) = (None, false, None, Vec::new());
        let mut pending = match work {
            LaneBodyWork::Start { generic, shrinks, observed: seen, called: by_bodies, caller: view, arrays: sampled } => { (observed, called, caller, arrays) = (seen, by_bodies, Some(view), sampled.to_vec()); (generic.to_vec(), lane_unit::MAX_UNIT_INLINED_INSTRUCTIONS >> shrinks) }
            LaneBodyWork::Poll(pending) => match self.lane_body_poll(pending) {
                Ok(outcome) => return outcome,
                Err(replan) => replan,
            },
        };
        // A plan again after its code was too large starts at once, from the background
        // thread's result as from the calling thread's; code the thread compiled at once (or
        // the calling thread, without one) publishes at once.
        loop {
            let (generic, budget) = pending;
            let direct = direct(self);
            let started = match self.start_lane_method(program, function, targets, captures, &direct, &generic, budget, entry, observed, called, caller.expect("a started body names its view"), &arrays) {
                Ok(started) => started,
                Err(Unstarted::Installed(published)) => return LaneBodyOutcome::Published(published),
                Err(Unstarted::Unpaid) => return LaneBodyOutcome::Unpaid,
                Err(Unstarted::Short) => return LaneBodyOutcome::Short,
                Err(Unstarted::Declined) => return LaneBodyOutcome::Declined,
            };
            // The plan's IR and lowering state, tens of megabytes for the largest bodies, are
            // gone; their pages go back instead of staying this thread's.
            quench_alloc::release_empty_pages();
            match self.lane_body_poll(Box::new(started)) {
                Ok(outcome) => return outcome,
                Err(replan) => pending = replan,
            }
        }
    }

    /// What a pending body's compile came to, or the feedback and budget to plan it again with
    /// where its code needed more memory than a body may take: half the inlining.
    fn lane_body_poll(&mut self, pending: Box<PendingLaneMethod>) -> Result<LaneBodyOutcome, (Vec<LaneExit>, usize)> {
        if !pending.watch.is_published() || pending.plan.direct.iter().any(|callee| callee.method.as_ref().is_some_and(|method| !method.is_published())) {
            return Ok(LaneBodyOutcome::Stale);
        }
        let key = pending.key;
        Ok(match pending.poll() {
            Err(pending) => LaneBodyOutcome::Compiling(pending),
            Ok(LaneCompile::Replan(generic, budget)) => return Err((generic, budget / REPLAN_BUDGET_DIVISOR)),
            Ok(LaneCompile::Done(method)) => method
                .and_then(|method| self.programs.publish_lane_method(method, &mut self.heap, self.method_cache_atoms.readers()))
                .inspect(|published| self.programs.note_installed_lane(key, published))
                .map_or(LaneBodyOutcome::Declined, LaneBodyOutcome::Published),
        })
    }
}

impl<H: Host> Vm<H> {
    /// Registers the writes a unit's raw stores may make (`lane_region::store_domains`) with
    /// the slots' observations; whether every slot already admitted them. A body's slot
    /// dependencies hold its slots at the admitted types, so its runs need not admit again.
    fn admit_lane_stores(&mut self, domains: Vec<(u32, u16, crate::typed_ir::Type)>) -> bool {
        domains.into_iter().fold(true, |admitted, (shape, slot, domain)| self.heap.admit_slot_writes(shape, usize::from(slot), domain) && admitted)
    }
}

impl PendingLaneMethod {
    /// Makes it stale where its code names shape ids, which a compaction renumbers.
    pub(in crate::vm) fn retire_named_shapes(&self) {
        if self.plan.names_shapes() { self.watch.retire(); }
    }

    /// How many times its plan halved the inlining budget for its code to fit, which a
    /// successor plan of the function starts from.
    pub(in crate::vm) fn shrinks(&self) -> u8 {
        (lane_unit::MAX_UNIT_INLINED_INSTRUCTIONS / self.plan.budget.max(1)).ilog2() as u8
    }

    /// What its compile came to, or itself while its code compiles.
    fn poll(mut self: Box<Self>) -> Result<LaneCompile, Box<Self>> {
        Ok(match self.code.poll() {
            std::task::Poll::Pending => return Err(self),
            std::task::Poll::Ready(Some(Ok(code))) => LaneCompile::Done(self.plan.finish(&code)),
            std::task::Poll::Ready(Some(Err(reason))) if self.plan.replans(reason) =>
                LaneCompile::Replan(std::mem::take(&mut self.generic), self.plan.budget),
            std::task::Poll::Ready(_) => LaneCompile::Done(None),
        })
    }
}

impl LanePlan {
    /// Whether its code names shape ids: its field sites', or its direct callees' bodies'.
    fn names_shapes(&self) -> bool {
        !self.fields.is_empty() || !self.direct.is_empty()
    }

    /// The callees its code names by identity: inlined frames' and direct calls'.
    fn callees(&self) -> Box<[Value]> {
        self.unit.frames.iter().filter_map(|frame| frame.target.identity())
            .chain(self.direct.iter().map(|callee| callee.callee)).collect()
    }

    /// Whether a rejection of its code is one planning again with less inlined addresses.
    fn replans(&self, reason: super::jit_backend::NativeCompileRejection) -> bool {
        reason.exceeds_code_budget() && !self.unit.frames.is_empty() && self.budget > 0
    }

    /// The facts its code relies on, its direct callees' included.
    fn dependencies(&self) -> LaneDependencies {
        LaneDependencies::of(self.fields.values().flatten().copied(), &self.direct)
    }

    /// The method its compiled `code` makes, published to executable memory. The method
    /// takes its direct callees whole: the code names them by address.
    fn finish(mut self, code: &[u8]) -> Option<LaneMethod> {
        let stable = std::mem::take(&mut self.lowered.region.stable);
        let unit = &self.unit;
        let amortizes = amortizes_entry(unit, self.function, &self.direct);
        let (dependencies, recovery_space) = (self.dependencies(), super::dispatch_fast::LaneRecoverySpace::inlined(&unit.frames));
        let (scratch_width, caller_registers) = (usize::from(unit.registers - unit.caller_registers), unit.caller_registers);
        let names_shapes = self.names_shapes();
        let memory = CodeMemory::new(code)?;
        if let (Some(directory), Some(clif)) = (native_dump_directory(), self.lowered.clif.as_deref()) {
            dump_native_code(directory, self.function, &memory, clif);
        }
        // SAFETY: the lane region lowers the shared native-entry ABI.
        let entry = unsafe { std::mem::transmute::<*mut u8, Entry>(memory.pointer()) };
        #[cfg(test)]
        LANE_METHODS_COMPILED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        #[cfg(test)]
        LANE_CALLS_INLINED.fetch_add(unit.frames.len(), std::sync::atomic::Ordering::Relaxed);
        Some(LaneMethod {
            dependencies, stable, invalidation: self.invalidation, _code: memory, entry, recovery_space,
            fault_poll: self.lowered.region.fault_poll || self.direct.iter().any(|callee| callee.body.as_ref().is_some_and(|body| body.fault_poll)),
            // A body that names a callee's body retires with it (`DirectCallee::method`).
            names_shapes, inlined: self.unit.frames,
            scratch_width, caller_registers,
            amortizes, direct: self.direct, closed: self.closed,
        })
    }
}

impl LaneMethod {
    /// A leave as the next body's feedback (`LaneLeaves`): one at an inlined call whose
    /// callee was none of the alternatives it inlined is the leave of the caller's call that
    /// inlined its frame, which the next body then makes itself (`Vm::lane_bindings`), so its
    /// callees run in their own bodies instead of the whole caller's remainder in the lane.
    /// A leave at a construction inside an inlined frame is the same: a construction no plan
    /// makes is generic only at a caller record (`Lowering::construct`), so the callee that
    /// holds it keeps its own body.
    pub(in crate::vm) fn feedback(&self, exit: LaneExit, program: &ResidualProgram) -> LaneExit {
        let LaneExit::Inlined { frame, pc } = exit else { return exit };
        let Some(inlined) = self.inlined.get(frame) else { return exit };
        let calls_here = self.inlined.iter().any(|call| call.parent == Some(frame) && call.call == pc);
        let constructs_here = program.function_get(inlined.target.function as usize)
            .and_then(|body| body.instructions().nth(pc as usize))
            .is_some_and(|instruction| instruction.op().class() == crate::bytecode::Class::Construct);
        if !calls_here && !constructs_here {
            return exit;
        }
        let mut root = inlined;
        while let Some(parent) = root.parent { root = &self.inlined[parent]; }
        LaneExit::At(root.call as usize)
    }
}

/// What a lane body's code relies on beyond its guards, its direct callees' included, once
/// each. A receiver's shape is guarded at each field use (and proved again after any call,
/// `State::call_effect`), so an object leaving it falls out at the guard: only its slots'
/// observations are dependencies, and a compaction retires code that names shape ids
/// (`LaneView::invalidate_shape_bindings`). A holder's (prototype's) layout is read
/// without a guard, so its departures are dependencies too.
#[derive(Default)]
pub(in crate::vm) struct LaneDependencies {
    pub(in crate::vm) slots: Vec<crate::code_dependencies::SlotDependency>,
    departure_shapes: Vec<u32>,
}

impl LaneDependencies {
    fn of(loads: impl Iterator<Item = NativeFieldLoad> + Clone, direct: &[DirectCallee]) -> Self {
        let mut slots: Vec<_> = loads.clone().filter_map(NativeFieldLoad::slot_dependency)
            .chain(direct.iter().filter_map(|callee| callee.body.as_ref()).flat_map(|body| body.dependencies.slots.iter().copied())).collect();
        slots.sort_unstable_by_key(|dependency| (dependency.shape, dependency.slot));
        slots.dedup();
        let mut departure_shapes: Vec<_> = loads.filter_map(|load| load.holder)
            .chain(direct.iter().filter_map(|callee| callee.body.as_ref()).flat_map(|body| body.dependencies.departure_shapes.iter().copied())).collect();
        departure_shapes.sort_unstable();
        departure_shapes.dedup();
        Self { slots, departure_shapes }
    }

    /// Makes `code` retire when a fact it took changes: a slot's observation, a holder
    /// shape's departure, or one of its Stable facts; `false` where a reader table is full.
    pub(in crate::vm) fn subscribe(&self, stable: &lane_region::StableFacts, code: &Rc<dyn crate::code_dependencies::DependentCode>,
        heap: &mut crate::heap::Heap, method_atoms: &mut crate::code_dependencies::CodeReaders) -> bool {
        self.slots.iter().all(|dependency| heap.subscribe_slot_observation(*dependency, code))
            && self.departure_shapes.iter().all(|shape| heap.subscribe_shape(*shape, code))
            && stable.holders.iter().all(|holder| crate::heap::subscribe_prototype_values(*holder, code))
            && stable.atoms.iter().all(|atom| method_atoms.subscribe(*atom, code))
            && stable.globals.iter().all(|fact| crate::heap::subscribe_global_value(*fact, code))
    }
}

#[cfg(test)] #[path = "lane_compile/tests.rs"] mod tests;
