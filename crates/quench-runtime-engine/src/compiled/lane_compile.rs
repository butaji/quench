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

/// A lowered lane body: the method it becomes once its code exists.
pub(crate) struct LanePlan {
    pub(crate) unit: lane_unit::Unit,
    pub(crate) lowered: lane_region::LoweredLane,
    pub(crate) fields: Vec<Option<Vec<NativeFieldLoad>>>,
    pub(crate) direct: Box<[DirectCallee]>,
    /// Whether every call of it that has run is inlined or direct (`LaneMethod::closed`).
    pub(crate) closed: bool,
    pub(crate) function: u32,
    /// The word its publication sets when one of its Stable facts changes.
    pub(crate) invalidation: quench_runtime::code_dependencies::Invalidation,
    /// The thresholds of the VM that planned it, which its publication reads (`amortizes_entry`).
    pub(crate) tiers: &'static Tiers,
}

/// The VM's facts a lane body's lowering may ask (`OnlineFacts`), answered when it is planned for
/// the identities, shapes and naming sites its unit names: the constructors its global reads
/// and frames name (`Vm::instanceof_fact`), the shapes its field loads prove
/// (`Vm::shape_is_stable`), and its `SetFunctionName` sites (`Vm::native_function_name`). A
/// question outside them answers that no fact holds, which folds less and stays exact.
#[derive(Clone, Default, PartialEq)]
pub(crate) struct FactTables {
    instanceofs: std::collections::BTreeMap<u64, quench_runtime::code_dependencies::InstanceofFact>,
    stable_shapes: std::collections::BTreeSet<u32>,
    names: std::collections::BTreeMap<(u16, Atom), (quench_runtime::staged::add_plan::Transition, u64)>,
    /// The own members of constant-marked objects the VM holds, by atom (`Vm::own_constants`).
    own_constants: std::collections::BTreeMap<Atom, OwnConstant>,
    /// The capture reads of the unit's own instructions that reach the program root's environment
    /// at a slot the root writes once, by PC, with the function each holds now (`root_constants`).
    root_constants: std::collections::BTreeMap<usize, Value>,
    /// The program those root constants belong to: the active one (`Vm::root_slot_written_once`).
    root_program: quench_runtime::vm::program_store::ProgramId,
}

/// Whether the `depth`-th enclosing function of `function`, by the program's static parent chain,
/// is the root: depth zero is the parent (a capture read's environment is its closure's scope).
fn reaches_root(program: &ResidualProgram, function: u32, depth: u16) -> bool {
    let mut current = function;
    for _ in 0..=depth {
        let Some(parent) = program.function_get(current as usize).and_then(|body| body.parent) else { return false };
        current = parent;
    }
    current == quench_runtime::vm::ROOT_FUNCTION_ID
}

/// The capture reads of `unit`'s own instructions that read a written-once root binding of the
/// active program (`Vm::root_slot_written_once`), by PC, with the value the root environment's
/// slot holds now, where that value is an initialized function. A read reaches the root by
/// `reaches_root`; the root environment is the one its activation promoted (`root_environment`).
fn root_constants(vm: &Vm, function: u32, unit: &lane_unit::Unit) -> std::collections::BTreeMap<usize, Value> {
    let program_id = vm.active_program;
    let (Some(program), Some(environment)) = (vm.programs.residual(program_id), vm.programs.root_environment(program_id)) else {
        return Default::default();
    };
    unit.instructions.iter().enumerate().filter_map(|(pc, instruction)| {
        let Some(quench_runtime::staged::Operation::LoadCapture { depth, slot, .. }) = crate::compiled::decoded(instruction) else { return None };
        let own = matches!(unit.origins.get(pc)?, lane_unit::Origin::Caller(_));
        if !own || !reaches_root(program, function, depth) || !vm.root_slot_written_once(program_id, slot) { return None; }
        let value = vm.heap_reads().environment_slot(environment, usize::from(slot))?;
        (value != Value::DELETED && vm.function_record(value).is_some()).then_some((pc, value))
    }).collect()
}

impl FactTables {
    /// What `vm` answers now to the questions `function`'s lowering of `unit` may ask.
    pub(crate) fn of(vm: &Vm, function: u32, unit: &lane_unit::Unit, fields: &[Option<Vec<NativeFieldLoad>>],
        global_reads: &quench_runtime::vm::global_facts::GlobalReads) -> Self {
        use quench_runtime::vm::global_facts::GlobalRead;
        let mut tables = Self::default();
        let read_values = (0..unit.instructions.len()).filter_map(|pc| match global_reads.get(&pc)? {
            GlobalRead::NativeMember { value, .. } => Some(*value),
            read => read.folded_value(),
        });
        let frame_values = unit.frames.iter().filter_map(|frame| frame.target.identity());
        for value in read_values.chain(frame_values) {
            if tables.instanceofs.contains_key(&value.to_bits()) { continue; }
            if let Some(fact) = vm.instanceof_fact(value) { tables.instanceofs.insert(value.to_bits(), fact); }
        }
        for load in fields.iter().flatten().flatten() {
            for shape in std::iter::once(load.shape).chain(load.holder) {
                if vm.shape_is_stable(shape) { tables.stable_shapes.insert(shape); }
            }
        }
        for instruction in &unit.instructions {
            if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::SetFunctionName) {
                let key = (instruction.cache_site_index(), instruction.atom_index());
                if let Some(name) = vm.native_function_name(key.0, key.1) { tables.names.insert(key, name); }
            }
        }
        tables.own_constants = vm.own_constants();
        tables.root_program = vm.active_program;
        tables.root_constants = root_constants(vm, function, unit);
        tables
    }

    pub(crate) fn own_constant(&self, atom: Atom) -> Option<OwnConstant> {
        self.own_constants.get(&atom).copied()
    }

    pub(crate) fn instanceof(&self, constructor: Value) -> Option<quench_runtime::code_dependencies::InstanceofFact> {
        self.instanceofs.get(&constructor.to_bits()).copied()
    }

    pub(crate) fn shape_stable(&self, shape: u32) -> bool {
        self.stable_shapes.contains(&shape)
    }

    pub(crate) fn name(&self, site: u16, atom: Atom) -> Option<(quench_runtime::staged::add_plan::Transition, u64)> {
        self.names.get(&(site, atom)).copied()
    }

    /// The root constant the capture read at `pc` takes (`root_constants`), with its program.
    pub(crate) fn root_constant(&self, pc: usize) -> Option<(quench_runtime::vm::program_store::ProgramId, Value)> {
        self.root_constants.get(&pc).map(|value| (self.root_program, *value))
    }
}

/// The program a lowering reads: the running one, on the program's thread, or a copy of it that
/// shares nothing with it (`ResidualProgram::detached`), on the code generator's.
pub(crate) enum LoweringProgram<'p> {
    #[cfg_attr(not(test), allow(dead_code))]
    Running(&'p ResidualProgram),
    Detached(quench_runtime::bytecode::DetachedProgram),
}

impl std::ops::Deref for LoweringProgram<'_> {
    type Target = ResidualProgram;
    fn deref(&self) -> &ResidualProgram {
        match self {
            Self::Running(program) => program,
            Self::Detached(program) => program,
        }
    }
}

/// Everything a lane body's lowering reads, owned: it reads nothing of the VM, so it runs where
/// its program does (`LoweringProgram`), on the program's thread or the code generator's.
pub(crate) struct LoweringJob<'p> {
    program: LoweringProgram<'p>,
    function: u32,
    unit: lane_unit::Unit,
    fields: Vec<Option<Vec<NativeFieldLoad>>>,
    global_reads: quench_runtime::vm::global_facts::GlobalReads,
    /// Its direct calls, by unit PC: pointers into the plan's `direct`, which its code names
    /// as descriptors, so the plan outlives the job (`PendingLaneMethod`'s drop waits for it).
    direct_sites: lane_region::DirectSites,
    frame_captures: Vec<(usize, usize)>,
    helpers: LaneHelpers,
    /// The VM's static facts (`Vm::vm_facts`); its online ones are the fields below.
    facts: VmFacts<'static>,
    /// The active program's AOT facts (`ProgramStore::aot_facts`), shared with every lowering of it.
    aot: Option<std::sync::Arc<quench_runtime::compile::aot_facts::AotFacts>>,
    /// The active program's call graph (`ProgramStore::call_graph`), which its call plans' inlining reads.
    graph: Option<std::sync::Arc<quench_runtime::compile::call_graph::CallGraph>>,
    method_cache_atoms: quench_runtime::vm::method_cache::AtomBits,
    invalidation: usize,
    constants: Vec<Value>,
    tables: FactTables,
    leaves: Vec<usize>,
    kinds: Vec<super::body_views::ObservedKind>,
    entry: Option<usize>,
    views: quench_runtime::vm::engine_types::ElementViews, // Its loop arrays' view (`body_views`).
}

impl LoweringJob<'_> {
    /// The lowered body, `None` where it does not lower.
    pub(crate) fn run(&self) -> Option<lane_region::LoweredLane> {
        let instanceofs = |constructor: Value| self.tables.instanceof(constructor);
        let names = |site: u16, atom| self.tables.name(site, atom);
        let shape_stable = |shape: u32| self.tables.shape_stable(shape);
        let own_constant = |atom: Atom| self.tables.own_constant(atom);
        let root_constant = |pc: usize| self.tables.root_constant(pc);
        let facts = VmFacts { online: Some(OnlineFacts { method_cache_atoms: &self.method_cache_atoms,
            invalidation: self.invalidation, constants: &self.constants, instanceofs: &instanceofs, shape_stable: &shape_stable,
            names: Some(&names), own_constant: &own_constant, root_constant: &root_constant }),
            facts: self.aot.as_deref(), graph: self.graph.as_deref(), ..self.facts };
        let admitted = super::admission::specialize(super::admission::UnitRequest { kind: super::admission::UnitKind::Body, facts: Default::default() }).compiled()?;
        lane_region::lower_body(&self.program, self.function, &self.unit, self.helpers, &self.fields, &self.global_reads, facts,
            &self.leaves, &self.direct_sites, &self.frame_captures, self.entry, &self.kinds, self.views, admitted)
    }
}

/// A plan whose code compiles on the code generator's thread. It holds while the facts it
/// took hold, under the lifecycle the published method has: `watch` subscribes to its slot
/// types and Stable facts from the plan on, a collection that frees a callee it names
/// retires it (`ProgramStore::note_pending_native_calls`), and so does a shape compaction
/// where it names shapes (`retire_named_shapes`). A collection alone leaves it valid: an
/// allocation-heavy loop collects more often than a body compiles.
pub(crate) struct PendingLaneMethod {
    /// Its lowering and code, on the code generator's thread; a drop waits for the lowering,
    /// which reads the plan's direct callees by pointer (`LoweringJob::direct_sites`). First,
    /// so it drops before the plan.
    pub code: super::jit_backend::OffThread<Lowered>,
    pub plan: LanePlan,
    pub watch: Rc<quench_runtime::code_dependencies::PublishedCode<()>>,
    /// What it compiles from (`LanePlan::key`).
    pub key: LaneKey,
    /// The site its planned size is noted for (`ProgramStore::note_planned_lane_ir`).
    pub site: (u32, u32, Option<u32>),
    /// What the plan took from the VM, which must still hold when its code arrives.
    pub taken: Box<TakenFacts>,
}


/// What a lowering and its code generation came to.
pub(crate) enum Lowered {
    /// Nothing of the unit lowers natively.
    Nothing,
    Compiled { lowered: lane_region::LoweredLane, ir: usize, code: LaneCode },
}

impl Lowered {
    /// Lowers `job` and compiles its code, reporting it under `census`.
    fn of(job: &LoweringJob<'_>, census: Option<Census>, level: super::jit_backend::CompileLevel) -> Self {
        let Some(mut lowered) = job.run().filter(|lowered| lowered.region.native != 0) else { return Self::Nothing };
        let native = std::mem::replace(&mut lowered.region.function, ir::Function::new());
        let ir = native.dfg.num_insts();
        super::lane_facts::note_lane_compile(super::lane_facts::Lowers::Body, job.unit.instructions.len(), ir);
        let code = image::compile(native, lowered.scalar.take(), lowered.proven.take(), level);
        if let Some(census) = census {
            eprintln!("{}", Census { ir, ..census }.report(code.as_ref().map_or(0, |image| image.code.len())));
        }
        Self::Compiled { lowered, ir, code }
    }
}

/// What `prepare_lane_plan` read from the VM for a plan.
struct LanePrepared {
    unit: lane_unit::Unit,
    fields: Vec<Option<Vec<NativeFieldLoad>>>,
    global_reads: quench_runtime::vm::global_facts::GlobalReads,
    direct: Box<[DirectCallee]>,
    direct_sites: lane_region::DirectSites,
    closed: bool,
    entry: Option<usize>,
    entry_left: bool,
    left_at: Vec<usize>,
}

/// The VM state a plan lowered under, which its code is published under only where the VM
/// answers the same when the code arrives (`Vm::lowering_holds`): the facts it asked, its global
/// reads, its inlined frames' captures and the leaves its field proofs were read for.
struct TakenFacts {
    tables: FactTables,
    global_reads: quench_runtime::vm::global_facts::GlobalReads,
    frame_captures: Vec<(usize, usize)>,
    left_at: Vec<usize>,
    /// Which direct callees had a summary to type their calls' results by (`DirectCallee::summary`), in `LanePlan::direct`'s order.
    callee_summaries: Vec<bool>,
}

/// What a pending body's compile came to.
enum LaneCompile {
    /// Its body lowered (to IR of the size given) and its code compiled; it publishes where
    /// what it took from the VM still holds (`Vm::lowering_holds`).
    Lowered(Box<PendingLaneMethod>, lane_region::LoweredLane, usize, image::LaneImage),
    Done(Option<LaneMethod>),
}

/// What a function's lane body asks of the VM at an entry past its warm-up.
pub(crate) enum LaneBodyWork<'a> {
    /// Plan a body from a predecessor's leaves (`generic`) and start compiling its code.
    Start { generic: &'a [LaneExit], called: bool, caller: &'a crate::dispatch_fast::LaneView,
        /// A loop's body: the loop's span and the iterations it has taken, which weigh its
        /// compile (`loop_model::body_taken`).
        looped: Option<(u64, u64)>,
    },
    /// Look for the code of the body compiling.
    Poll(Box<dyn std::any::Any>),
}

/// The callees a lane body's plan may inline at each call: the arms of the call's plan (`CallPlans::targets`), less the
/// functions `excluded` names, and none at a call the body's generic leaves left at (`LaneExit::At`).
fn planned_targets<'p>(plans: &'p lane_region::bindings::CallPlans<'p>, function: u32, generic: &'p [LaneExit], excluded: &'p [u32])
    -> impl Fn(u32, u32) -> Vec<CallTarget> + 'p {
    move |caller, pc| {
        if caller == function && generic.contains(&LaneExit::At(pc as usize)) { return Vec::new(); }
        plans.targets(caller, pc).into_iter().filter(|target| !excluded.contains(&target.function)).collect()
    }
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

/// Whether a plan goes on to its lowering, from its unit, field proofs and direct callees
/// alone; one that does not records why where the gate's owner reads it.
type PlanGate<'a> = dyn Fn(&Vm, &lane_unit::Unit, &dyn std::fmt::Debug, &[DirectCallee]) -> bool + 'a;

/// What the VM did for it.
pub(crate) enum LaneBodyOutcome {
    Compiling(Box<dyn std::any::Any>),
    Published(Rc<PublishedLaneMethod>),
    /// A collection ran while its code compiled: its proofs no longer hold, and it warms again.
    Stale,
    /// Its function has not run long enough to pay for the compile, or callees it would call
    /// directly still warm toward their bodies: it counts on.
    Deferred,
    /// The predicted tier is the interpreter (`lane_facts::body_pick`): the interpreter runs the function.
    Unpaid,
    /// Its unit is too short to amortize an entry and no body calls it: declined until a caller's
    /// compile meets a call to it (`LaneView::demand_body`).
    Short,
    Declined,
}

#[cfg_attr(not(feature = "jit"), allow(unused_imports))]
use crate::compiled::tier_policy::LANE_BACKTRACKING_MAX_INSTS;
#[cfg(test)] #[path = "lane_compile/ahead.rs"] pub(crate) mod ahead;
use super::admission::{refuse, Refusal};
#[cfg_attr(not(feature = "jit"), allow(unused_imports))]
use lane_facts::{unit_closed, bind_callees, Census, plan_key, amortizes_entry, entry_pc, unit_direct_calls, unit_leaves};

/// The unit PCs a previous body left at, and whether it left at its entry. A leave at the first
/// instruction is a wrong entry speculation: the next body drops its entry contract
/// (`LaneExit::Entry`) and lowers the instruction as any other.
fn left_places(unit: &lane_unit::Unit, generic: &[LaneExit], first: usize) -> (Vec<usize>, bool) {
    let mut left_at = unit_leaves(unit, generic);
    let entry_left = left_at.contains(&first) || generic.contains(&LaneExit::Entry);
    left_at.retain(|pc| *pc != first);
    (left_at, entry_left)
}

/// A lane body's work (`body_work_in_lane`), the route `jit_routes` installs.
pub(crate) type BodyWork = fn(&mut Vm, &ResidualProgram, u32,
    &dyn Fn(Value) -> (usize, usize), &dyn Fn(&Vm) -> Vec<DirectCandidates>, LaneBodyWork<'_>, Option<u32>) -> LaneBodyOutcome;

#[quench_macros::ext(CompiledLaneCompileVm)]
impl Vm {
    /// The AOT facts of `program`, where it is the running program: facts of another program name no call of it.
    pub(crate) fn aot_facts_of(&self, program: &ResidualProgram) -> Option<std::sync::Arc<quench_runtime::compile::aot_facts::AotFacts>> {
        self.programs.residual(self.active_program).filter(|running| std::ptr::eq(*running, program))
            .and_then(|_| self.programs.aot_facts(self.active_program))
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
        let Some(body) = program.function_get(function as usize) else { refuse(self, Refusal::NoUnit); return None; };
        let unit = lane_unit::unit(program, graph, function, body, targets, &|caller, pc| plans.at(caller, pc),
            &|callee| program.function_get(callee as usize)).or_else(|| { refuse(self, Refusal::NoUnit); None })?;
        // Every planned callee binds by its function (`CalleeBinding::Function`). Any such frame is admitted, whatever
        // its body: a frame that leaves mid-body leaves through the frame-materialization exit (`leave_at`,
        // `materialize_inlined`), or restarts its call where its recovery says so (`CallRecovery`).
        unit.frames.iter().all(|frame| frame.target.callee == CalleeBinding::Function)
            .then_some(unit).or_else(|| { refuse(self, Refusal::FrameNotAdmitted); None })
    }

    /// Plans `function`'s lane body: its unit, proofs and lowering, and the function the code
    /// generator compiles. `None` where it does not lower or runs nothing natively.
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    fn plan_lane_method(&mut self, program: &ResidualProgram, function: u32,
        captures: &dyn Fn(Value) -> (usize, usize), direct: &[DirectCandidates],
        generic: &[LaneExit], entry: Option<u32>,
        gate: &PlanGate<'_>) -> Option<(LanePlan, ir::Function)> {
        let LanePrepared { unit, fields, global_reads, direct, direct_sites, closed, entry, entry_left, left_at } =
            self.prepare_lane_plan(program, function, direct, generic, &[], entry, None, gate)?;
        // The body folds Stable facts under its own invalidation word, which its publication
        // sets when one changes (`ProgramStore::publish_lane_method`).
        let invalidation = quench_runtime::code_dependencies::Invalidation::default();
        let job = self.lowering_job(LoweringProgram::Running(program), function, &unit, &fields, global_reads, direct_sites,
            captures, invalidation.address(), (entry, entry_left, &left_at));
        let mut lowered = job.run().or_else(|| { refuse(self, Refusal::NothingNative); None })?;
        if lowered.region.native == 0 { refuse(self, Refusal::NothingNative); return None; }
        let native = std::mem::replace(&mut lowered.region.function, ir::Function::new());
        Some((LanePlan { unit, lowered, fields, direct, closed, function, invalidation, tiers: self.programs.tiers() }, native))
    }

    /// What `function`'s lane body lowers from (`LanePrepared`): its unit, field proofs, global
    /// reads and direct calls, read from the VM. Its callees are the arms of its call plans
    /// (`planned_targets`), less the functions `excluded` names. `None` where it has no unit or
    /// entry, or `gate` decides against it.
    #[allow(clippy::too_many_arguments)]
    fn prepare_lane_plan(&mut self, program: &ResidualProgram, function: u32,
        direct: &[DirectCandidates], generic: &[LaneExit], excluded: &[u32], entry: Option<u32>,
        view: Option<&crate::dispatch_fast::LaneView>, gate: &PlanGate<'_>) -> Option<LanePrepared> {
        let aot = self.aot_facts_of(program);
        let graph = self.call_graph_of(program);
        let plans = lane_region::bindings::CallPlans::new(program, aot.as_deref(), graph.as_deref());
        let targets = planned_targets(&plans, function, generic, excluded);
        let unit = self.plan_unit_in_lane(program, function, graph.as_deref(), &plans, &targets)?;
        let entry = entry_pc(&unit, entry).or_else(|| { refuse(self, Refusal::NoEntry); None })?;
        let (left_at, entry_left) = left_places(&unit, generic, entry.unwrap_or(0));
        let mut fields = self.unit_fields_in_lane(program, &unit, &left_at, view);
        // Admitting the stores' writes once, here, widens what they widen; the fields then read the widened slots.
        if !self.admit_lane_stores(lane_region::store_domains(&unit.instructions, |site| lane_region::site_loads(&fields, site).map(Vec::as_slice))) { fields = self.unit_fields_in_lane(program, &unit, &left_at, view); }
        let global_reads = self.global_reads_in_lane(program, function, &unit);
        let (direct, direct_sites) = unit_direct_calls(&unit, direct);
        // What decides against a plan from its unit alone decides before its lowering, the most
        // of a plan's cost (`PlanGate`).
        if !gate(self, &unit, &fields, &direct) { return None; }
        let closed = unit_closed(&unit, function, |pc| direct_sites.contains_key(&pc), &targets);
        Some(LanePrepared { unit, fields, global_reads, direct, direct_sites, closed, entry, entry_left, left_at })
    }

    /// Everything the lowering of `unit` reads (`LoweringJob`), taken from the VM now: the
    /// captures of its inlined frames, the VM's facts it may ask (`FactTables`), the names
    /// method caches hold, the running program's constants, its leaves and the typed kinds of its element records.
    #[allow(clippy::too_many_arguments)]
    fn lowering_job<'p>(&self, program: LoweringProgram<'p>, function: u32, unit: &lane_unit::Unit, fields: &[Option<Vec<NativeFieldLoad>>],
        global_reads: quench_runtime::vm::global_facts::GlobalReads, direct_sites: lane_region::DirectSites, captures: &dyn Fn(Value) -> (usize, usize),
        invalidation: usize, (entry, entry_left, left_at): (Option<usize>, bool, &[usize])) -> LoweringJob<'p> {
        let frame_captures: Vec<(usize, usize)> = unit.frames.iter()
            .map(|frame| frame.target.identity().map_or((0, 0), captures)).collect();
        let active = self.active_program;
        let aot = self.aot_facts_of(&program);
        let graph = self.call_graph_of(&program);
        let constants = self.programs.residual(active).filter(|running| std::ptr::eq(*running, &*program))
            .map_or(Vec::new(), |_| self.programs.constants(active).to_vec());
        let tables = FactTables::of(self, function, unit, fields, &global_reads);
        let leaves: Vec<usize> = left_at.iter().copied().chain(entry_left.then(|| lane_region::entry_leave(unit))).chain(self.adding_stores(unit)).collect();
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
        captures: &dyn Fn(Value) -> (usize, usize), direct: Vec<DirectCandidates>, generic: &[LaneExit]) -> Option<LaneMethod> {
        // A test-only compile: its unit is minted here, and its lowering takes the body's gate
        // (`LanePlan` lowering mints `UnitKind::Body`).
        let _unit = crate::compiled::admission::test_unit();
        self.host.before_native_compilation();
        let method = self.plan_lane_method(program, function, captures, &direct, generic, None, &|_, _, _, _| true)
            .and_then(|(mut plan, native)| match image::compile(native, plan.lowered.scalar.take(), plan.lowered.proven.take(),
                super::jit_backend::CompileLevel::run_default()) {
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
        captures: &dyn Fn(Value) -> (usize, usize), direct: &[DirectCandidates], generic: &[LaneExit], entry: Option<u32>,
        looped: Option<(u64, u64)>, called: bool, caller: &crate::dispatch_fast::LaneView) -> Result<PendingLaneMethod, Unstarted> {
        #[cfg(feature = "jit")]
        { self.start_lane_method_compiled(program, function, captures, direct, generic, entry, looped, called, caller) }
        #[cfg(not(feature = "jit"))]
        {
            let _ = (program, function, captures, direct, generic, entry, looped, called, caller);
            Err(Unstarted::Declined)
        }
    }

    #[cfg(feature = "jit")]
    #[allow(clippy::too_many_arguments)]
    fn start_lane_method_compiled(&mut self, program: &ResidualProgram, function: u32,
        captures: &dyn Fn(Value) -> (usize, usize), direct: &[DirectCandidates], generic: &[LaneExit], entry: Option<u32>,
        looped: Option<(u64, u64)>, called: bool, caller: &crate::dispatch_fast::LaneView) -> Result<PendingLaneMethod, Unstarted> {
        // A plan costs the program's own time, as much as its compile and on its thread: what
        // decides against the compile after planning decides before it, charging the plan too.
        // A function whose compiles from this entry are spent plans nothing; one whose last plan
        // from it did not pay at its size is not planned again until its entries would pay for that size.
        let site = (self.active_program.raw(), function, entry);
        let tiers = self.programs.tiers();
        // Before its first plan, a body's size is at least its own bytecodes' (`ir_per_bytecode`
        // each, the loop region's measure): inlining only adds to it.
        let own_ir = |bytecodes: u64| super::lane_facts::unplanned_ir(&tiers.loops, bytecodes);
        let planned = self.programs.planned_lane_ir(&site);
        // A function the loaded image gave a body was paid for ahead of time: a plan after that body
        // left (a fact it took weakened) respecializes at once, as the image's install did.
        let ahead = super::admission::ahead_unit(function).is_some() && self.programs.ahead_images(self.active_program, program).is_some();
        // The predicted tier (`lane_facts::body_pick`) decides the pick; the level is the run's own.
        let level = super::jit_backend::CompileLevel::run_default();
        let mut pick = None;
        let Some(graph) = self.call_graph_of(program) else { refuse(self, Refusal::NoUnit); return Err(Unstarted::Declined) };
        // A function that calls itself back through other functions runs a call tree per entry,
        // as a loop does, before any of its calls is direct (`CallGraph::in_cycle`).
        let recursive = entry.is_none() && graph.in_cycle(function);
        if !ahead {
            pick = Some(super::lane_facts::body_pick(tiers, program, &graph, function, caller.called_by_regions()));
        }
        let looped_taken = ahead || looped.is_none_or(|(span, taken)| taken >= super::loop_model::body_taken(tiers, span, planned.unwrap_or_else(|| own_ir(span)), true));
        // Phase A: the compile bound, the pick and the looped iterations, in that order (`admission::LanePlan`).
        let compiles_left = self.programs.compiles_left_in_lane(&site, tiers.lane_body.compile_bound(entry.is_some()));
        let plan_unit = match super::admission::specialize(super::admission::UnitRequest { kind: super::admission::UnitKind::LanePlan,
            facts: super::admission::UnitFacts { compiles_left, pick, called_by_regions: caller.called_by_regions(), looped_taken, ..Default::default() } }) {
            super::admission::Decision::Compile(unit) => unit,
            super::admission::Decision::Declined => { refuse(self, Refusal::CompileBound); return Err(Unstarted::Declined); }
            super::admission::Decision::Unpaid => return Err(Unstarted::Unpaid),
            super::admission::Decision::Short => return Err(Unstarted::Short),
        };
        super::base_region::count(super::base_region::Event::BodyPlanned);
        let _mark = quench_runtime::trace::CompileMark::begin("u-jit", "plan", function, entry.unwrap_or(0), || program.function_get(function as usize).map_or(0, |body| body.instructions().count()));
        // A unit too short to amortize its entry, or whose key a published body holds, is known
        // before its lowering; the same tests after it stay, for the unit a smaller budget made.
        // A member of the cycle is called, not inlined: inlined, its call back to `function`
        // leaves the body (a direct call runs only from the body's own frame), while called it
        // is a direct call of its own body once one exists, as `function`'s self calls are.
        let others: Vec<u32> = graph.cycle_of(function).iter().copied().filter(|member| *member != function).collect();
        let stopped: std::cell::RefCell<Option<Unstarted>> = std::cell::RefCell::new(None);
        let gate = |vm: &Self, unit: &lane_unit::Unit, fields: &dyn std::fmt::Debug, direct: &[DirectCallee]| {
            // Phase B during planning: the entry gate, on the planned unit (`LaneEntry`). Its amortization is read only where the gate reads it.
            let entry_facts = super::admission::UnitFacts { entry_none: entry.is_none(), called_by_bodies: called, recursive,
                amortizes_entry: entry.is_some() || called || recursive || amortizes_entry(tiers, unit, function, direct), ..Default::default() };
            if super::admission::specialize(super::admission::UnitRequest { kind: super::admission::UnitKind::LaneEntry(plan_unit), facts: entry_facts }).compiled().is_none() {
                *stopped.borrow_mut() = Some(Unstarted::Short); return false;
            }
            let key: LaneKey = (vm.active_program.raw(), function, entry, plan_key(unit, fields, direct.len(), generic, entry));
            if let Some(published) = vm.programs.installed_lane(&key) { *stopped.borrow_mut() = Some(Unstarted::Installed(published)); return false; }
            true
        };
        // Planning reads the VM here; the lowering, the most of a plan's cost, reads only what the
        // plan took (`LoweringJob`) and runs with the code generation on the code generator's
        // thread, while the program runs on.
        self.host.before_native_compilation();
        let prepared = self.prepare_lane_plan(program, function, direct, generic, &others, entry, Some(caller), &gate);
        self.host.after_native_compilation();
        match stopped.into_inner() {
            Some(Unstarted::Short) => { refuse(self, Refusal::NotAmortized); return Err(Unstarted::Short); }
            Some(stop) => return Err(stop),
            None => {}
        }
        let LanePrepared { unit, fields, global_reads, direct, direct_sites, closed, entry: entry_at, entry_left, left_at } =
            prepared.ok_or(Unstarted::Declined)?;
        // A body that cannot amortize a lane entry would serve native callers only
        // (`LaneView::specialized`); the lane's warm-up is not one, so it declines here rather
        // than compile code no caller may ever run.
        // A body compiles only where one entry of its unit amortizes it (`amortizes_entry`, the same test as above,
        // inlined callees included).
        // Phase B after planning: the install gate on the plan's unit (`LaneInstall`). The callees bind, and the
        // installed and admission checks run, only with that unit.
        let install_facts = super::admission::UnitFacts { entry_none: entry.is_none(), called_by_regions: caller.called_by_regions(),
            looped_none: looped.is_none(), recursive, amortizes_entry: entry.is_some() || recursive || amortizes_entry(tiers, &unit, function, &direct), ..Default::default() };
        let Some(_install) = super::admission::specialize(super::admission::UnitRequest { kind: super::admission::UnitKind::LaneInstall(plan_unit), facts: install_facts }).compiled() else {
            refuse(self, Refusal::NotAmortized); return Err(Unstarted::Short);
        };
        bind_callees(&direct, caller);
        let key: LaneKey = (self.active_program.raw(), function, entry, plan_key(&unit, &fields, direct.len(), generic, entry));
        if let Some(published) = self.programs.installed_lane(&key) { return Err(Unstarted::Installed(published)); }
        if !self.programs.admit_lane_compile(&key, tiers.lane_body.compile_bound(entry.is_some())) { refuse(self, Refusal::CompileBound); return Err(Unstarted::Declined); }
        let invalidation = quench_runtime::code_dependencies::Invalidation::default();
        // The functions the lowering reads: its own, its inlined callees' and its direct callees'.
        let functions: Vec<u32> = std::iter::once(function).chain(unit.frames.iter().map(|frame| frame.target.function))
            .chain(direct.iter().map(|callee| callee.target.function)).collect();
        let job = self.lowering_job(LoweringProgram::Detached(program.detached(&functions)), function, &unit, &fields,
            global_reads.clone(), direct_sites, captures, invalidation.address(), (entry_at, entry_left, &left_at));
        let callee_summaries = direct.iter().map(|callee| callee.summary().is_some()).collect();
        let taken = TakenFacts { tables: job.tables.clone(), global_reads, frame_captures: job.frame_captures.clone(), left_at, callee_summaries };
        let census = crate::compiled::admission::diagnosing(self).then(|| Census {
            function, osr: entry.is_some(), unit: unit.instructions.len(), ir: 0, key: key.3,
            generic: generic.len(), direct: direct.len(), closed });
        let plan = LanePlan { unit, lowered: lane_region::LoweredLane::default(), fields, direct, closed, function, invalidation, tiers };
        let watch = Rc::new(quench_runtime::code_dependencies::PublishedCode::new(()));
        let code = super::jit_backend::OffThread::guarded((job, census, level, (function, entry.unwrap_or(0), plan.unit.instructions.len())), |(job, census, level, (function, pc, bytecodes))| {
            let _mark = quench_runtime::trace::CompileMark::begin("u-jit", "body", function, pc, || bytecodes);
            Lowered::of(&job, census, level)
        });
        self.programs.note_pending_native_calls(&watch, plan.callees());
        Ok(PendingLaneMethod { plan, watch, code, key, site, taken: Box::new(taken) })
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
        direct: &dyn Fn(&Self) -> Vec<DirectCandidates>, work: LaneBodyWork<'_>, entry: Option<u32>) -> LaneBodyOutcome {
        // Without the JIT's routes (`jit_routes`) the body declines and the interpreter runs.
        match crate::jit_routes::jit() {
            Some(routes) => (routes.body)(self, program, function, captures, direct, work, entry),
            None => LaneBodyOutcome::Declined,
        }
    }

    /// `body_work_in_lane`'s route, which `jit_routes` installs.
    fn compile_body_work(&mut self, program: &ResidualProgram, function: u32,
        captures: &dyn Fn(Value) -> (usize, usize),
        direct: &dyn Fn(&Self) -> Vec<DirectCandidates>, work: LaneBodyWork<'_>, entry: Option<u32>) -> LaneBodyOutcome {
        // Native code reads slot representations, which the runtime keeps from its first compile on.
        // A refusal noted while the body plans happened in this function (`quench_runtime::fallback`).
        let _focus = quench_runtime::fallback::focus(program, function, entry.unwrap_or(0));
        let (generic, looped, called, caller) = match work {
            LaneBodyWork::Start { generic, looped, called, caller } => (generic.to_vec(), looped, called, caller),
            LaneBodyWork::Poll(pending) => return self.body_poll_in_lane(program, pending),
        };
        // A function with an ahead-of-time body takes it where a first compile would.
        if let Some(unit) = super::admission::ahead_unit(function)
            && let Some(published) = self.install_ahead_body(unit, program, function, entry, &generic)
        {
            return LaneBodyOutcome::Published(published);
        }
        let direct = direct(self);
        let started = match self.start_lane_method(program, function, captures, &direct, &generic, entry, looped, called, caller) {
            Ok(started) => started,
            Err(Unstarted::Installed(published)) => return LaneBodyOutcome::Published(published),
            Err(Unstarted::Unpaid) => return LaneBodyOutcome::Unpaid,
            Err(Unstarted::Short) => return LaneBodyOutcome::Short,
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
        if !pending.watch.is_published() || pending.plan.direct.iter().any(|callee| callee.method.as_ref().is_some_and(|method| !method.is_published())) {
            return LaneBodyOutcome::Stale;
        }
        let key = pending.key;
        match pending.poll() {
            Err(pending) => LaneBodyOutcome::Compiling(pending),
            Ok(LaneCompile::Lowered(mut pending, lowered, ir, code)) => {
                self.programs.note_planned_lane_ir(pending.site, ir);
                // The facts it folds are subscribed from now on; what changed while it lowered
                // makes it stale here, as a change after this retires it.
                if !self.lowering_holds(program, &pending, &lowered) { return LaneBodyOutcome::Stale; }
                pending.plan.lowered = lowered;
                let watch = pending.watch.clone();
                let (mut facts, _) = self.fact_watch();
                facts.hold_roots(&watch, pending.plan.lowered.region.stable.roots.clone());
                let watcher: Rc<dyn quench_runtime::code_dependencies::DependentCode> = watch.clone();
                if !pending.plan.dependencies().subscribe(&pending.plan.lowered.region.stable, &watcher, &mut facts) {
                    refuse(self, Refusal::Dependencies);
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
            && FactTables::of(self, plan.function, &plan.unit, &plan.fields, &taken.global_reads) == taken.tables
            && self.global_reads_in_lane(program, plan.function, &plan.unit) == taken.global_reads
            && unformed(&self.unit_fields_in_lane(program, &plan.unit, &taken.left_at, None)) == unformed(&plan.fields)
            && plan.unit.frames.iter().map(|frame| frame.target.identity().map_or((0, 0), |callee| self.callee_capture_slots(callee)))
                .eq(taken.frame_captures.iter().copied())
            && lowered.region.stable.atoms.iter().all(|atom| !self.method_cache_atoms.bits().contains(*atom as usize))
    }

#[quench_macros::ext(CompiledLaneCompileVm2)]
impl Vm {
    /// Registers the writes a unit's raw stores may make (`lane_region::store_domains`) with
    /// the slots' observations; whether every slot already admitted them. A body's slot
    /// dependencies hold its slots at the admitted types, so its runs need not admit again.
    fn admit_lane_stores(&mut self, domains: Vec<(u32, u16, quench_runtime::typed_ir::Type)>) -> bool {
        domains.into_iter().fold(true, |admitted, (shape, slot, domain)| self.admit_slot_writes(shape, usize::from(slot), domain) && admitted)
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
            std::task::Poll::Ready(Some(Lowered::Compiled { lowered, ir, code: Ok(code) })) => LaneCompile::Lowered(self, lowered, ir, code),
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
            tiers: self.tiers, fault_poll: lowered.region.fault_poll,
            exits: std::mem::take(&mut lowered.region.exits), summary: lowered.region.summary.take(), clif: lowered.clif, proven_calls: false,
        };
        finish_lane_method(memory, scalar, proven, facts)
    }
}

#[quench_macros::ext(LaneMethodOps4)]
impl LaneMethod {
    /// A leave as the next body's feedback (`LaneLeaves`): one at an inlined call whose
    /// callee was none of the alternatives it inlined is the leave of the caller's call that
    /// inlined its frame, which the next body then makes itself (`planned_targets`), so its
    /// callees run in their own bodies instead of the whole caller's remainder in the lane.
    /// A leave at a construction inside an inlined frame is the same: a construction no plan
    /// makes is generic only at a caller record (`Lowering::construct`), so the callee that
    /// holds it keeps its own body.
    #[inline]
    pub(crate) fn feedback(&self, exit: LaneExit, program: &ResidualProgram) -> LaneExit {
        let LaneExit::Inlined { frame, pc } = exit else { return exit };
        let Some(inlined) = self.inlined.get(frame) else { return exit };
        let calls_here = self.inlined.iter().any(|call| call.parent == Some(frame) && call.call == pc);
        let constructs_here = program.function_get(inlined.target.function as usize)
            .and_then(|body| body.instructions().nth(pc as usize))
            .is_some_and(|instruction| quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::Construct));
        if !calls_here && !constructs_here {
            return exit;
        }
        let mut root = inlined;
        while let Some(parent) = root.parent { root = &self.inlined[parent]; }
        LaneExit::At(root.call as usize)
    }
}

#[path = "lane_compile/dependencies.rs"]
pub(crate) mod dependencies;
pub(crate) use dependencies::LaneDependencies;

#[cfg(test)] #[path = "lane_compile/tests.rs"] pub(crate) mod tests;

