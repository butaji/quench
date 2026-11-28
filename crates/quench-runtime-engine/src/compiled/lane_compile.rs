//! A lane body's two halves: planning, which reads the VM (call targets, field proofs, direct
//! callees) and lowers the unit, and code generation, which reads nothing of it. The plan
//! keeps everything the method holds but its code, so code generation may run on the code
//! generator's thread (`jit_backend::OffThread`) while the program runs on.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::vm::program_store::lane_publication::LaneKey;

type LaneCode = Result<image::LaneImage, super::jit_backend::NativeCompileRejection>;
pub(crate) use super::lane_method::{finish_lane_method, LaneMethodFacts};
#[path = "lane_compile/image.rs"] pub(crate) mod image;

#[path = "lane_compile/planning.rs"]
mod planning;
pub(crate) use planning::*;
#[path = "lane_compile/fact_tables.rs"]
mod fact_tables;
pub(crate) use fact_tables::*;
#[path = "lane_compile/jobs.rs"]
mod jobs;
pub(crate) use jobs::*;
#[cfg_attr(not(feature = "jit"), allow(unused_imports))]
use crate::compiled::tier_policy::LANE_BACKTRACKING_MAX_INSTS;
#[cfg(test)] #[path = "lane_compile/ahead.rs"] pub(crate) mod ahead;
#[cfg_attr(not(feature = "jit"), allow(unused_imports))]
use lane_facts::{unit_closed, bind_callees, Census, plan_key, entry_pc, unit_direct_calls};

/// A lane body's work (`body_work_in_lane`), the route `jit_routes` installs.
pub(crate) type BodyWork = fn(&mut Vm, &ResidualProgram, u32,
    &dyn Fn(Value) -> (usize, usize), &dyn Fn(&mut Vm) -> Vec<DirectCandidates>, LaneBodyWork<'_>, Option<u32>) -> LaneBodyOutcome;

#[quench_macros::ext(CompiledLaneCompileVm)]
impl Vm {
    /// The AOT facts of `program`, where it is the running program: facts of another program name no call of it.
    pub(crate) fn aot_facts_of(&self, program: &ResidualProgram) -> Option<std::sync::Arc<quench_runtime::compile::aot_facts::AotFacts>> {
        self.programs.residual(self.active_program).filter(|running| std::ptr::eq(*running, program))
            .and_then(|_| self.programs.aot_facts(self.active_program))
    }

    /// The predicted tier of `function`'s lane body in `program` (`lane_facts::body_pick`), where it is the running
    /// program: a program not running has no table, so its bodies are the interpreter's.
    pub(crate) fn lane_pick(&self, program: &ResidualProgram, function: u32) -> quench_runtime::vm::tier_policy::PredictedTier {
        let interpreted = quench_runtime::vm::tier_policy::PredictedTier::Interpreter;
        if !self.programs.residual(self.active_program).is_some_and(|running| std::ptr::eq(running, program)) { return interpreted; }
        let predicted = self.programs.predicted_tiers(self.active_program);
        let aot = self.programs.ahead_images(self.active_program, program).is_some();
        super::lane_facts::body_pick(aot, predicted.as_deref().unwrap_or(&[]), function)
    }

    /// The call graph of `program`, where it is the running program (`ProgramStore::call_graph`): a graph of another
    /// program names no call of it.
    fn call_graph_of(&self, program: &ResidualProgram) -> Option<std::sync::Arc<quench_runtime::compile::call_graph::CallGraph>> {
        self.programs.residual(self.active_program).filter(|running| std::ptr::eq(*running, program))
            .and_then(|_| self.programs.call_graph(self.active_program))
    }

    /// The unit of `function`, planned under its call plans (`plans`) and their callees (`targets`), if each frame it
    /// inlines is admitted.
    fn plan_unit_in_lane(&mut self, program: &ResidualProgram, function: u32, graph: Option<&quench_runtime::compile::call_graph::CallGraph>,
        plans: &lane_region::bindings::CallPlans<'_>, targets: &dyn Fn(u32, u32) -> Vec<CallTarget>) -> Option<lane_unit::Unit> {
        let Some(body) = program.function_get(function as usize) else { return None; };
        let unit = lane_unit::unit(program, graph, function, body, targets, &|caller, pc| plans.at(caller, pc),
            &|callee| program.function_get(callee as usize))?;
        // Every planned callee binds by its function (`CalleeBinding::Function`). Any such frame is admitted, whatever
        // its body: a frame that leaves mid-body leaves through the frame-materialization exit (`leave_at`,
        // `materialize_inlined`), or restarts its call where its recovery says so (`CallRecovery`).
        unit.frames.iter().all(|frame| frame.target.callee == CalleeBinding::Function)
            .then_some(unit)
    }

    /// Plans `function`'s lane body: its unit, proofs and lowering, and the function the code
    /// generator compiles. `None` where it does not lower or runs nothing natively.
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    fn plan_lane_method(&mut self, program: &ResidualProgram, function: u32,
        captures: &dyn Fn(Value) -> (usize, usize), direct: &[DirectCandidates],
        entry: Option<u32>, gate: &PlanGate<'_>) -> Option<(LanePlan, ir::Function)> {
        let LanePrepared { unit, fields, global_reads, direct, direct_sites, closed, entry } =
            self.prepare_lane_plan(program, function, direct, &[], entry, None, gate)?;
        // The body folds Stable facts under its own invalidation word, which its publication
        // sets when one changes (`ProgramStore::publish_lane_method`).
        let invalidation = quench_runtime::code_dependencies::Invalidation::default();
        let job = self.lowering_job(LoweringProgram::Running(program), function, &unit, &fields, global_reads, direct_sites,
            captures, invalidation.address(), entry);
        let mut lowered = job.run()?;
        if lowered.region.native == 0 { return None; }
        let native = std::mem::replace(&mut lowered.region.function, ir::Function::new());
        Some((LanePlan { unit, lowered, fields, direct, closed, function, invalidation }, native))
    }

    /// What `function`'s lane body lowers from (`LanePrepared`): its unit, field proofs, global
    /// reads and direct calls, read from the VM. Its callees are the arms of its call plans
    /// (`planned_targets`), less the functions `excluded` names. `None` where it has no unit or
    /// entry, or `gate` decides against it.
    #[allow(clippy::too_many_arguments)]
    fn prepare_lane_plan(&mut self, program: &ResidualProgram, function: u32,
        direct: &[DirectCandidates], excluded: &[u32], entry: Option<u32>,
        view: Option<&crate::dispatch_fast::LaneView>, gate: &PlanGate<'_>) -> Option<LanePrepared> {
        let aot = self.aot_facts_of(program);
        let graph = self.call_graph_of(program);
        let plans = lane_region::bindings::CallPlans::new(program, aot.as_deref(), graph.as_deref());
        let targets = planned_targets(&plans, excluded);
        let unit = self.plan_unit_in_lane(program, function, graph.as_deref(), &plans, &targets)?;
        let entry = entry_pc(&unit, entry)?;
        let mut fields = self.unit_fields_in_lane(program, &unit, function, view);
        // Admitting the stores' writes once, here, widens what they widen; the fields then read the widened slots.
        let domains = lane_region::store_domains(&unit.instructions, |site| lane_region::site_loads(&fields, site).map(Vec::as_slice),
            |pc, instruction, load| self.store_proven(aot.as_deref(), &unit, function, pc, instruction, load));
        if !self.admit_lane_stores(domains) { fields = self.unit_fields_in_lane(program, &unit, function, view); }
        let global_reads = self.global_reads_in_lane(program, function, &unit);
        let (direct, direct_sites) = unit_direct_calls(&unit, direct);
        // What decides against a plan from its unit alone decides before its lowering, the most
        // of a plan's cost (`PlanGate`).
        if !gate(self, &unit, &fields, &direct) { return None; }
        let closed = unit_closed(&unit, function, |pc| direct_sites.contains_key(&pc), &targets);
        Some(LanePrepared { unit, fields, global_reads, direct, direct_sites, closed, entry })
    }

    /// Everything the lowering of `unit` reads (`LoweringJob`), taken from the VM now: the
    /// captures of its inlined frames, the VM's facts it may ask (`FactTables`), the names
    /// method caches hold, the running program's constants, the stores it lowers through the VM's
    /// adding entries and the typed kinds of its element records.
    #[allow(clippy::too_many_arguments)]
    fn lowering_job<'p>(&self, program: LoweringProgram<'p>, function: u32, unit: &lane_unit::Unit, fields: &[Option<Vec<NativeFieldLoad>>],
        global_reads: quench_runtime::vm::global_facts::GlobalReads, direct_sites: lane_region::DirectSites, captures: &dyn Fn(Value) -> (usize, usize),
        invalidation: usize, entry: Option<usize>) -> LoweringJob<'p> {
        let frame_captures: Vec<(usize, usize)> = unit.frames.iter()
            .map(|frame| frame.target.identity().map_or((0, 0), captures)).collect();
        let active = self.active_program;
        let aot = self.aot_facts_of(&program);
        let graph = self.call_graph_of(&program);
        let constants = self.programs.residual(active).filter(|running| std::ptr::eq(*running, &*program))
            .map_or(Vec::new(), |_| self.programs.constants(active).to_vec());
        let tables = FactTables::of(self, function, unit, fields);
        let leaves: Vec<usize> = self.adding_stores(unit).collect();
        // A typed element read the interpreter's record observed takes its kind (`body_views::typed_reads`).
        let kinds = super::body_views::typed_reads(self, function, unit);
        LoweringJob { views: super::body_views::body_views(self, function, unit), function, unit: unit.clone(), fields: fields.to_vec(), global_reads, direct_sites, frame_captures,
            helpers: Self::process_helpers(), facts: self.vm_facts(), aot, graph, method_cache_atoms: self.method_cache_atoms.bits().clone(),
            invalidation, constants, tables, leaves, kinds, entry, program }
    }

    /// The lane helpers of this process, noted for the coverage census.
    fn process_helpers() -> LaneHelpers {
        let helpers = LaneHelper::addresses();
        quench_runtime::jit_coverage::note_native_helpers(helpers);
        LaneHelpers::Process(helpers)
    }

    /// Specialize `function` for lane activations, admitting a body only where its first
    /// instruction runs natively: an entry that leaves at once is pure cost. Code its plan
    /// makes that the code generator rejects is declined.
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn compile_lane_method(&mut self, program: &ResidualProgram, function: u32,
        captures: &dyn Fn(Value) -> (usize, usize), direct: Vec<DirectCandidates>) -> Option<LaneMethod> {
        // A test-only compile: its unit is minted here, and its lowering takes the body's gate
        // (`LanePlan` lowering mints `UnitKind::Body`).
        let _unit = crate::compiled::admission::test_unit();
        self.host.before_native_compilation();
        let method = self.plan_lane_method(program, function, captures, &direct, None, &|_, _, _, _| true)
            .and_then(|(mut plan, native)| match image::compile(native, plan.lowered.scalar.take(), plan.lowered.proven.take(),
                super::jit_backend::CompileLevel::Optimized) {
                Ok(code) => plan.finish(&code.code, code.scalar, code.proven),
                Err(_) => None,
            });
        self.host.after_native_compilation();
        method.inspect(LaneMethod::reserve_scratch)
    }

    /// Plans `function`'s lane body and starts compiling its code on the code
    /// generator's thread (`PendingLaneMethod`). An execution-only build (no `jit`) starts no
    /// run-time compile: the plan declines, and the function runs the image's body where one
    /// published and the interpreter otherwise.
    #[allow(clippy::too_many_arguments)]
    fn start_lane_method(&mut self, program: &ResidualProgram, function: u32,
        captures: &dyn Fn(Value) -> (usize, usize), direct: &[DirectCandidates], entry: Option<u32>,
        caller: &crate::dispatch_fast::LaneView) -> Result<PendingLaneMethod, Unstarted> {
        #[cfg(feature = "jit")]
        { self.start_lane_method_compiled(program, function, captures, direct, entry, caller) }
        #[cfg(not(feature = "jit"))]
        {
            let _ = (program, function, captures, direct, entry, caller);
            Err(Unstarted::Declined)
        }
    }

    #[cfg(feature = "jit")]
    #[allow(clippy::too_many_arguments)]
    fn start_lane_method_compiled(&mut self, program: &ResidualProgram, function: u32,
        captures: &dyn Fn(Value) -> (usize, usize), direct: &[DirectCandidates], entry: Option<u32>,
        caller: &crate::dispatch_fast::LaneView) -> Result<PendingLaneMethod, Unstarted> {
        // A plan costs the program's own time, as much as its compile and on its thread: what
        // decides against the compile after planning decides before it, charging the plan too.
        // One whose last plan from this entry did not pay at its size is not planned again until its
        // entries would pay for that size.
        // The body's tier is its predicted tier through the policy (`lane_body_tier`): the interpreter's pick compiles
        // nothing, whoever calls the function.
        let level = super::jit_backend::CompileLevel::Optimized;
        let pick = self.lane_pick(program, function);
        if quench_runtime::vm::tier_policy::lane_body_tier(pick).is_none() { return Err(Unstarted::Unpaid) }
        let Some(graph) = self.call_graph_of(program) else { return Err(Unstarted::Declined) };
        // A body whose key a published body holds is known before its lowering. A member of the cycle is called, not inlined: inlined, its call back to `function`
        // leaves the body (a direct call runs only from the body's own frame), while called it
        // is a direct call of its own body once one exists, as `function`'s self calls are.
        let others: Vec<u32> = graph.cycle_of(function).iter().copied().filter(|member| *member != function).collect();
        let stopped: std::cell::RefCell<Option<Unstarted>> = std::cell::RefCell::new(None);
        let gate = |vm: &Self, unit: &lane_unit::Unit, fields: &dyn std::fmt::Debug, direct: &[DirectArm]| {
            let key: LaneKey = (vm.active_program.raw(), function, entry, plan_key(unit, fields, direct.len(), entry));
            if let Some(published) = vm.programs.installed_lane(&key) { *stopped.borrow_mut() = Some(Unstarted::Installed(published)); return false; }
            true
        };
        // Planning reads the VM here; the lowering, the most of a plan's cost, reads only what the
        // plan took (`LoweringJob`) and runs with the code generation on the code generator's
        // thread, while the program runs on.
        self.host.before_native_compilation();
        let prepared = self.prepare_lane_plan(program, function, direct, &others, entry, Some(caller), &gate);
        self.host.after_native_compilation();
        if let Some(stop) = stopped.into_inner() { return Err(stop); }
        let LanePrepared { unit, fields, global_reads, direct, direct_sites, closed, entry: entry_at } =
            prepared.ok_or(Unstarted::Declined)?;
        bind_callees(&direct, caller);
        let key: LaneKey = (self.active_program.raw(), function, entry, plan_key(&unit, &fields, direct.len(), entry));
        if let Some(published) = self.programs.installed_lane(&key) { return Err(Unstarted::Installed(published)); }
        let invalidation = quench_runtime::code_dependencies::Invalidation::default();
        // The functions the lowering reads: its own, its inlined callees' and its direct callees'.
        let functions: Vec<u32> = std::iter::once(function).chain(unit.frames.iter().map(|frame| frame.target.function))
            .chain(direct.iter().map(|callee| callee.body.target.function)).collect();
        let job = self.lowering_job(LoweringProgram::Detached(program.detached(&functions)), function, &unit, &fields,
            global_reads.clone(), direct_sites, captures, invalidation.address(), entry_at);
        let callee_summaries = direct.iter().map(|callee| callee.summary().is_some()).collect();
        let taken = TakenFacts { tables: job.tables.clone(), global_reads, frame_captures: job.frame_captures.clone(), callee_summaries };
        let census = quench_runtime::trace::Channel::JsJit.on().then(|| Census {
            function, osr: entry.is_some(), unit: unit.instructions.len(), ir: 0, key: key.3,
            direct: direct.len(), closed });
        let plan = LanePlan { unit, lowered: lane_region::LoweredLane::default(), fields, direct, closed, function, invalidation };
        let watch = Rc::new(quench_runtime::code_dependencies::PublishedCode::new(()));
        let code = super::jit_backend::OffThread::guarded((job, census, level), |(job, census, level)| {
            Lowered::of(&job, census, level)
        });
        self.programs.note_pending_native_calls(&watch, plan.callees());
        Ok(PendingLaneMethod { plan, watch, code, key, taken: Box::new(taken) })
    }

    /// The unit PCs of stores whose sites added their property to some receiver (they hold
    /// an add entry): each lowers as the store a previous body left at does, through the VM's
    /// adding entries (`LaneHelper::FieldAdd`), which a direct own-slot store cannot be.
    fn adding_stores<'a>(&'a self, unit: &'a lane_unit::Unit) -> impl Iterator<Item = usize> + 'a {
        unit.instructions.iter().enumerate().filter(|(_, instruction)| instruction.op().field_store().is_some()
            && self.field_add_indices.get(self.field_cache_index(instruction.cache_site_index())) != quench_runtime::vm::NO_FIELD_ADD_SET)
            .map(|(pc, _)| pc)
    }

    /// Does `work` for `function`'s lane body (`LaneBodyWork`), whose direct callees
    /// `direct` gives; publishes a body whose code compiled while its proofs held. An `entry`
    /// (a caller PC) plans the body to start there, as a running loop enters it.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn body_work_in_lane(&mut self, program: &ResidualProgram, function: u32,
        captures: &dyn Fn(Value) -> (usize, usize),
        direct: &dyn Fn(&mut Self) -> Vec<DirectCandidates>, work: LaneBodyWork<'_>, entry: Option<u32>) -> LaneBodyOutcome {
        // Without the JIT's routes (`jit_routes`) the body declines and the interpreter runs.
        match crate::jit_routes::jit() {
            Some(routes) => (routes.body)(self, program, function, captures, direct, work, entry),
            None => LaneBodyOutcome::Declined,
        }
    }

    /// `body_work_in_lane`'s route, which `jit_routes` installs.
    fn compile_body_work(&mut self, program: &ResidualProgram, function: u32,
        captures: &dyn Fn(Value) -> (usize, usize),
        direct: &dyn Fn(&mut Self) -> Vec<DirectCandidates>, work: LaneBodyWork<'_>, entry: Option<u32>) -> LaneBodyOutcome {
        // Native code reads slot representations, which the runtime keeps from its first compile on.
        let caller = match work {
            LaneBodyWork::Start { caller } => caller,
            LaneBodyWork::Poll(pending) => return self.body_poll_in_lane(program, pending),
        };
        // A function with an ahead-of-time body takes it where a first compile would.
        if let Some(published) = self.install_ahead_body(super::admission::ahead_unit(), program, function, entry) {
            return LaneBodyOutcome::Published(published);
        }
        let direct = direct(&mut *self);
        let started = match self.start_lane_method(program, function, captures, &direct, entry, caller) {
            Ok(started) => started,
            Err(Unstarted::Installed(published)) => return LaneBodyOutcome::Published(published),
            Err(Unstarted::Unpaid) => return LaneBodyOutcome::Unpaid,
            Err(Unstarted::Declined) => return LaneBodyOutcome::Declined,
        };
        // The plan's IR and lowering state, tens of megabytes for the largest bodies, are
        // gone; their pages go back instead of staying this thread's.
        quench_alloc::release_empty_pages();
        self.body_poll_in_lane(program, Box::new(started))
    }

    /// What a pending body's compile came to.
    fn body_poll_in_lane(&mut self, program: &ResidualProgram, pending: Box<dyn std::any::Any>) -> LaneBodyOutcome {
        let pending = crate::compiled::pending_lane(pending);
        if !pending.watch.is_published() || pending.plan.direct.iter().any(|callee| callee.body.method.as_ref().is_some_and(|method| !method.is_published())) {
            return LaneBodyOutcome::Stale;
        }
        let key = pending.key;
        match pending.poll() {
            Err(pending) => LaneBodyOutcome::Compiling(pending),
            Ok(LaneCompile::Lowered(mut pending, lowered, code)) => {
                // The facts it folds are subscribed from now on; what changed while it lowered
                // makes it stale here, as a change after this retires it.
                if !self.lowering_holds(program, &pending, &lowered) { return LaneBodyOutcome::Stale; }
                pending.plan.lowered = lowered;
                let watch = pending.watch.clone();
                let (mut facts, _) = self.fact_watch();
                facts.hold_roots(&watch, pending.plan.lowered.region.stable.roots.clone());
                let watcher: Rc<dyn quench_runtime::code_dependencies::DependentCode> = watch.clone();
                if !pending.plan.dependencies().subscribe(&pending.plan.lowered.region.stable, &watcher, &mut facts) {
                    return LaneBodyOutcome::Declined;
                }
                let PendingLaneMethod { plan, .. } = *pending;
                plan.finish(&code.code, code.scalar, code.proven)
                    .and_then(|method| { let (mut facts, programs) = self.fact_watch(); programs.publish_lane_method(method, &mut facts) })
                    .inspect(|published| self.programs.note_installed_lane(key, published))
                    .map_or(LaneBodyOutcome::Declined, LaneBodyOutcome::Published)
            }
            Ok(LaneCompile::Done(method)) => method
                .and_then(|method| { let (mut facts, programs) = self.fact_watch(); programs.publish_lane_method(method, &mut facts) })
                .inspect(|published| self.programs.note_installed_lane(key, published))
                .map_or(LaneBodyOutcome::Declined, LaneBodyOutcome::Published),
        }
    }

    /// Whether the VM answers now what `pending`'s plan took from it (`TakenFacts`): the facts
    /// its lowering asked, its global reads, field proofs and inlined frames' captures, and no
    /// method cache naming an atom `lowered` stores in place.
    fn lowering_holds(&mut self, program: &ResidualProgram, pending: &PendingLaneMethod, lowered: &lane_region::LoweredLane) -> bool {
        let (plan, taken) = (&pending.plan, &*pending.taken);
        // A field's formed layout id (the view's own-field hint) is guarded where the code reads
        // it, so a hint that moved since only leaves: the fields compare without it.
        let unformed = |fields: &[Option<Vec<NativeFieldLoad>>]| fields.iter()
            .map(|loads| loads.as_ref().map(|loads| loads.iter().map(|load| NativeFieldLoad { formed: None, ..*load }).collect::<Vec<_>>())).collect::<Vec<_>>();
        // A callee whose summary a withdrawal ended since the plan took it types a result the code no longer may.
        plan.direct.iter().map(|callee| callee.summary().is_some()).eq(taken.callee_summaries.iter().copied())
            && FactTables::of(self, plan.function, &plan.unit, &plan.fields) == taken.tables
            && self.global_reads_in_lane(program, plan.function, &plan.unit) == taken.global_reads
            && unformed(&self.unit_fields_in_lane(program, &plan.unit, plan.function, None)) == unformed(&plan.fields)
            && plan.unit.frames.iter().map(|frame| frame.target.identity().map_or((0, 0), |callee| self.callee_capture_slots(callee)))
                .eq(taken.frame_captures.iter().copied())
            && lowered.region.stable.atoms.iter().all(|atom| !self.method_cache_atoms.bits().contains(*atom as usize))
    }
}

#[quench_macros::ext(CompiledLaneCompileVm2)]
impl Vm {
    /// Registers the writes a unit's raw stores may make (`lane_region::store_domains`) with
    /// the slots' observations; whether every slot already admitted them. A body's slot
    /// dependencies hold its slots at the admitted types, so its runs need not admit again.
    fn admit_lane_stores(&mut self, domains: Vec<(u32, u32, quench_runtime::typed_ir::Type)>) -> bool {
        domains.into_iter().fold(true, |admitted, (shape, slot, domain)| self.admit_slot_writes(shape, slot as usize, domain) && admitted)
    }

    /// Whether the store `instruction` at unit pc `pc` of `unit` needs no admission at lane compile (`store_domains`): its receiver's
    /// proven class seeds the slot (`lane_region::tag_proofs::store_seeded`), or, where the store's shape is a constant of this
    /// program, the shape's constructed root names the class and the store's atom is the slot's union field (`shape_constructed_class`).
    /// No facts, or neither, keeps the admission.
    fn store_proven(&self, facts: Option<&quench_runtime::compile::aot_facts::AotFacts>, unit: &super::lane_unit::Unit, function: u32,
        pc: usize, instruction: quench_runtime::bytecode::WideInstruction, load: &crate::compiled::NativeFieldLoad) -> bool
    {
        let Some(facts) = facts else { return false };
        let atom = instruction.atom_index();
        let shaped = self.shape_constructed_class(load.shape).is_some_and(|(program, class)| program == self.active_program
            && lane_region::tag_proofs::slot_write_seeded(facts, class, atom, load.ty)
            && lane_region::tag_proofs::union_slot(facts, class, atom) == Some(load.field_index()));
        shaped || lane_region::tag_proofs::store_seeded(facts, unit, function, pc, instruction, load.ty)
    }
}

#[quench_macros::ext(PendingLaneMethodOps1)]
impl PendingLaneMethod {
    /// Makes it stale where its code names shape ids, which a compaction renumbers.
    pub(crate) fn retire_named_shapes(&self) {
        if self.plan.names_shapes() { self.watch.retire(); }
    }

    /// What its lowering and compile came to, or itself while they run.
    fn poll(mut self: Box<Self>) -> Result<LaneCompile, Box<Self>> {
        Ok(match self.code.poll() {
            std::task::Poll::Pending => return Err(self),
            std::task::Poll::Ready(Some(Lowered::Compiled { lowered, code: Ok(code) })) => LaneCompile::Lowered(self, lowered, code),
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
            .chain(self.direct.iter().map(|callee| callee.closure.callee)).collect()
    }

    /// The facts its code relies on, its direct callees' included.
    fn dependencies(&self) -> LaneDependencies {
        LaneDependencies::of(self.fields.iter().flatten().flatten().copied(), &self.direct)
    }

    /// As `finish`, over code already placed at the address it will run at: a linking lowering's
    /// page and branch relocations are relative to that address (`live::link_lane_at`), so they
    /// cannot be patched for a later copy.
    pub(crate) fn finish_in(self, memory: CodeMemory, scalar: Option<usize>, proven: Option<(usize, Box<[quench_runtime::typed_ir::Type]>)>) -> Option<LaneMethod> {
        self.finish_placed(memory, scalar, proven)
    }

    /// The method its compiled `code` makes, published to executable memory. The method
    /// takes its direct callees whole: the code names them by address.
    pub(crate) fn finish(self, code: &[u8], scalar: Option<usize>, proven: Option<(usize, Box<[quench_runtime::typed_ir::Type]>)>) -> Option<LaneMethod> {
        let memory = super::code_heap::publishing("u-jit-lane", self.function as usize, || CodeMemory::new(code))?;
        self.finish_placed(memory, scalar, proven)
    }

    /// The method `memory`'s code makes (`finish`, `finish_in`).
    fn finish_placed(self, memory: CodeMemory, scalar: Option<usize>, proven: Option<(usize, Box<[quench_runtime::typed_ir::Type]>)>) -> Option<LaneMethod> {
        let mut lowered = self.lowered;
        let facts = LaneMethodFacts {
            unit: self.unit, stable: std::mem::take(&mut lowered.region.stable), fields: self.fields,
            direct: self.direct, closed: self.closed, function: self.function, invalidation: self.invalidation,
            fault_poll: lowered.region.fault_poll,
            exits: std::mem::take(&mut lowered.region.exits), summary: lowered.region.summary.take(), clif: lowered.clif, proven_calls: false,
        };
        finish_lane_method(super::lane_method::LaneCode::Owned(memory), scalar, proven, facts)
    }
}

#[path = "lane_compile/dependencies.rs"]
pub(crate) mod dependencies;
pub(crate) use dependencies::LaneDependencies;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

#[cfg(test)] #[path = "lane_compile/tests.rs"] pub(crate) mod tests;

