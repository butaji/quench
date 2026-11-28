use super::*;

/// Everything a lane body's lowering reads, owned: it reads nothing of the VM, so it runs where
/// its program does (`LoweringProgram`), on the program's thread or the code generator's.
pub(crate) struct LoweringJob<'p> {
    pub(super) program: LoweringProgram<'p>,
    pub(super) function: u32,
    pub(super) unit: lane_unit::Unit,
    pub(super) fields: Vec<Option<Vec<NativeFieldLoad>>>,
    pub(super) global_reads: quench_runtime::vm::global_facts::GlobalReads,
    /// Its direct calls, by unit PC: pointers into the plan's `direct`, which its code names
    /// as descriptors, so the plan outlives the job (`PendingLaneMethod`'s drop waits for it).
    pub(super) direct_sites: lane_region::DirectSites,
    pub(super) frame_captures: Vec<(usize, usize)>,
    pub(super) helpers: LaneHelpers,
    /// The VM's static facts (`Vm::vm_facts`); its online ones are the fields below.
    pub(super) facts: VmFacts<'static>,
    /// The active program's AOT facts (`ProgramStore::aot_facts`), shared with every lowering of it.
    pub(super) aot: Option<std::sync::Arc<quench_runtime::compile::aot_facts::AotFacts>>,
    /// The active program's call graph (`ProgramStore::call_graph`), which its call plans' inlining reads.
    pub(super) graph: Option<std::sync::Arc<quench_runtime::compile::call_graph::CallGraph>>,
    pub(super) method_cache_atoms: quench_runtime::vm::method_cache::AtomBits,
    pub(super) invalidation: usize,
    pub(super) constants: Vec<Value>,
    pub(super) tables: FactTables,
    pub(super) leaves: Vec<usize>,
    pub(super) kinds: Vec<super::super::body_views::ObservedKind>,
    pub(super) entry: Option<usize>,
    pub(super) views: quench_runtime::vm::engine_types::ElementViews, // Its loop arrays' view (`body_views`).
}

impl LoweringJob<'_> {
    /// The lowered body, `None` where it does not lower.
    pub(crate) fn run(&self) -> Option<lane_region::LoweredLane> {
        let instanceofs = |constructor: Value| self.tables.instanceof(constructor);
        let names = |site: u16, atom| self.tables.name(site, atom);
        let shape_stable = |shape: u32| self.tables.shape_stable(shape);
        let root_constant = |pc: usize| self.tables.root_constant(pc);
        let facts = VmFacts { online: Some(OnlineFacts { method_cache_atoms: &self.method_cache_atoms,
            invalidation: self.invalidation, constants: &self.constants, instanceofs: &instanceofs, shape_stable: &shape_stable,
            names: Some(&names), root_constant: &root_constant }),
            facts: self.aot.as_deref(), graph: self.graph.as_deref(), ..self.facts };
        let admitted = super::super::admission::specialize(super::super::admission::UnitRequest { kind: super::super::admission::UnitKind::Body, facts: Default::default() })?;
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
    pub code: super::super::jit_backend::OffThread<Lowered>,
    pub plan: LanePlan,
    pub watch: Rc<quench_runtime::code_dependencies::PublishedCode<()>>,
    /// What it compiles from (`LanePlan::key`).
    pub key: LaneKey,
    /// What the plan took from the VM, which must still hold when its code arrives.
    pub taken: Box<TakenFacts>,
}


/// What a lowering and its code generation came to.
pub(crate) enum Lowered {
    /// Nothing of the unit lowers natively.
    Nothing,
    Compiled { lowered: lane_region::LoweredLane, code: LaneCode },
}

impl Lowered {
    /// Lowers `job` and compiles its code, reporting it under `census`.
    pub(super) fn of(job: &LoweringJob<'_>, census: Option<Census>, level: super::super::jit_backend::CompileLevel) -> Self {
        let Some(mut lowered) = job.run().filter(|lowered| lowered.region.native != 0) else { return Self::Nothing };
        let native = std::mem::replace(&mut lowered.region.function, ir::Function::new());
        let ir = native.dfg.num_insts();
        let code = image::compile(native, lowered.scalar.take(), lowered.proven.take(), level);
        if let Some(census) = census {
            eprintln!("{}", Census { ir, ..census }.report(code.as_ref().map_or(0, |image| image.code.len())));
        }
        Self::Compiled { lowered, code }
    }
}

/// What `prepare_lane_plan` read from the VM for a plan.
pub(super) struct LanePrepared {
    pub(super) unit: lane_unit::Unit,
    pub(super) fields: Vec<Option<Vec<NativeFieldLoad>>>,
    pub(super) global_reads: quench_runtime::vm::global_facts::GlobalReads,
    pub(super) direct: Box<[DirectCallee]>,
    pub(super) direct_sites: lane_region::DirectSites,
    pub(super) closed: bool,
    pub(super) entry: Option<usize>,
}

/// The VM state a plan lowered under, which its code is published under only where the VM
/// answers the same when the code arrives (`Vm::lowering_holds`): the facts it asked, its global
/// reads and its inlined frames' captures.
pub(super) struct TakenFacts {
    pub(super) tables: FactTables,
    pub(super) global_reads: quench_runtime::vm::global_facts::GlobalReads,
    pub(super) frame_captures: Vec<(usize, usize)>,
    /// Which direct callees had a summary to type their calls' results by (`DirectCallee::summary`), in `LanePlan::direct`'s order.
    pub(super) callee_summaries: Vec<bool>,
}

/// What a pending body's compile came to.
pub(super) enum LaneCompile {
    /// Its body lowered and its code compiled; it publishes where
    /// what it took from the VM still holds (`Vm::lowering_holds`).
    Lowered(Box<PendingLaneMethod>, lane_region::LoweredLane, image::LaneImage),
    Done(Option<LaneMethod>),
}

/// What a function's lane body asks of the VM at an entry past its warm-up.
pub(crate) enum LaneBodyWork<'a> {
    /// Plan the body of the function and start compiling its code, for `caller`, the view whose body it is.
    Start { caller: &'a crate::dispatch_fast::LaneView },
    /// Look for the code of the body compiling.
    Poll(Box<dyn std::any::Any>),
}

/// The callees a lane body's plan may inline at each call: the arms of the call's plan (`CallPlans::targets`), less the
/// functions `excluded` names.
pub(super) fn planned_targets<'p>(plans: &'p lane_region::bindings::CallPlans<'p>, excluded: &'p [u32])
    -> impl Fn(u32, u32) -> Vec<CallTarget> + 'p {
    move |caller, pc| plans.targets(caller, pc).into_iter().filter(|target| !excluded.contains(&target.function)).collect()
}

/// Why a body did not start compiling.
pub(super) enum Unstarted {
    /// A body published under the same key still holds.
    Installed(Rc<PublishedLaneMethod>),
    /// Not yet worth compiling.
    Unpaid,
    Declined,
}

/// Whether a plan goes on to its lowering, from its unit, field proofs and direct callees
/// alone; one that does not records why where the gate's owner reads it.
pub(super) type PlanGate<'a> = dyn Fn(&Vm, &lane_unit::Unit, &dyn std::fmt::Debug, &[DirectCallee]) -> bool + 'a;

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
    Declined,
}

