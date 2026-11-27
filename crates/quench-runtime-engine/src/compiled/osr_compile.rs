//! Loop-region code compiled beside the program: the loop runs in the lane while the code
//! generator's thread compiles, and the next look publishes the code.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::{
    compile_native_function, dump_native_bytes, dump_native_code, DumpSite, names_length, native_dump_directory, lane_unit, osr_loop,
    publish_body, CompiledOsrRegion, LaneHelpers, LoopRegion,
    NativeCompileRejection, NativeFieldLoad, Observation, VmFacts,
};
use super::base_region::function::{build_region, Body, BodySpec};
use super::lane_compile::LaneDependencies;
use super::tier_policy::{Tiers, REGION_CODE_BUDGET};
use super::lane_region::StableFacts;
use quench_runtime::code_dependencies::Invalidation;
pub(crate) mod calls;
pub(crate) mod inlined_unit;
use inlined_unit::*;
pub(crate) use calls::{CallCode, RegionCalls};
use quench_runtime::bytecode::{FieldLookup, Function, Register, ResidualProgram, WideInstruction};
use quench_runtime::vm::global_facts::GlobalTable;
use quench_runtime::vm::specializer::Specializer;
use super::{DirectCallee, DirectCandidates};
use std::rc::Rc;

pub(crate) use quench_runtime::vm::engine_types::OsrFacts;



/// A loop region whose code compiles on the code generator's thread, as V8 compiles OSR code
/// concurrently: compiling on the loop's own thread stalled it 2 ms a region (Box2D --jit:
/// 34 ms of a 733 ms run). Its facts are the observation's at the start, to which `watch`
/// subscribes: a shape compaction or a slot widening meanwhile discards the code.
pub(crate) struct PendingOsrRegion {
    /// Its lowering and code on the code generator's thread; a drop waits for the lowering,
    /// which reads `directs` by pointer. First, so it drops before them.
    pub code: quench_runtime::vm::jit_backend::OffThread<Option<LoopLowered>>,
    pub watch: Rc<quench_runtime::code_dependencies::PublishedCode<()>>,
    pub header: u32,
    pub facts: OsrFacts,
    /// What a run of the code builds around it.
    pub crossing: super::Crossing,
    /// Where a run may re-enter the code mid-region.
    pub resumes: super::base_region::ResumeTable,
    /// What its code owns of its calls (`CallCode`): known at the start for a b-jit region, made
    /// from the plan the lowering took (`directs`) for a Cranelift one.
    pub calls: Option<CallCode>,
    /// Per plan the lowering may take (`LoopInput`), its direct callees, which its code names.
    pub directs: Vec<Box<[DirectCallee]>>,
    /// The caller's registers and invalidation word, which a Cranelift region's `CallCode` takes.
    pub call_frame: (u16, Invalidation),
    /// Per plan, where its lowering folded Stable facts (`OsrOnline`): its unit, global reads and
    /// the VM's answers it took, which must still hold where the code publishes (`poll`).
    pub taken: Vec<Option<TakenPlan>>,
    pub function: u32,
}

/// What a plan whose lowering folded Stable facts took from the VM (`PendingOsrRegion::taken`).
struct TakenPlan {
    unit: lane_unit::Unit,
    fields: Vec<Option<Vec<NativeFieldLoad>>>,
    global_reads: quench_runtime::vm::global_facts::GlobalReads,
    tables: super::lane_compile::FactTables,
}

impl TakenPlan {
    /// Whether the VM answers now what the plan took: its global reads and its fact tables.
    fn holds(&self, vm: &mut quench_runtime::vm::Vm, program: &ResidualProgram, function: u32) -> bool {
        vm.global_table(program, function, &self.unit, GlobalTable::Lane) == self.global_reads
            && super::lane_compile::FactTables::of(vm, function, &self.unit, &self.fields, &self.global_reads, &|_, _| None) == self.tables
    }
}

/// What a loop region's lowering and code generation came to: the plan it took (an index of
/// `PendingOsrRegion::directs`), the Stable facts and roots its code folded, its CLIF text where
/// dumps are on, and its code.
pub(crate) struct LoopLowered {
    plan: usize,
    stable: StableFacts,
    roots: quench_runtime::folded_roots::FoldedRoots,
    clif: Option<String>,
    code: Result<Vec<u8>, NativeCompileRejection>,
}

/// One plan a loop region's lowering may take, owned (`loop_inputs`): its unit and entry
/// words, the span it covers, its earlier leaves, the global reads and observed reads taken for
/// it, and its direct calls by unit PC (pointers into `PendingOsrRegion::directs`).
struct LoopInput {
    unit: lane_unit::Unit,
    /// What the VM answered the lowering's questions of this unit when the plan was taken
    /// (`FactTables`), where the region folds Stable facts (`OsrOnline`).
    tables: Option<super::lane_compile::FactTables>,
    entry_fields: Vec<Option<u32>>,
    span: (usize, usize),
    generic: Vec<usize>,
    global_reads: quench_runtime::vm::global_facts::GlobalReads,
    sites: super::lane_region::DirectSites,
}

/// What the observation tells a loop region's lowering, owned (`Observation`'s borrowed parts).
struct LoopFacts {
    int_entries: Vec<(Register, quench_runtime::typed_ir::IntRange)>,
    double_registers: Vec<Register>,
    string_registers: Vec<Register>,
    typed_captures: Vec<u16>,
    int_captures: Vec<u16>,
    /// The kind each loop array's register held at entry (`Observation::typed_kinds`).
    typed_kinds: Vec<(Register, quench_runtime::tier_surface::TypedArrayKind)>,
    frame_captures: Vec<(usize, usize)>,
    entry_bounds: bool,
    /// By (function, PC) of each construction of the plans' units, the scalar plan its callee
    /// builds by (`Observation::construct_plans`).
    construct_plans: std::collections::BTreeMap<(u32, u32), super::ScalarConstructPlan>,
}

/// Everything a loop region's lowering reads, owned: it reads nothing of the VM, so it runs on
/// the code generator's thread (`ResidualProgram::detached` for the program).
struct LoopJob {
    /// The gate's proof that this loop's region may compile (minted by the caller of `start_osr_region`).
    admitted: super::admission::CompiledUnit,
    program: quench_runtime::bytecode::DetachedProgram,
    plans: Vec<LoopInput>,
    fields: Vec<Option<Vec<NativeFieldLoad>>>,
    facts: LoopFacts,
    isa: std::sync::Arc<dyn cranelift_codegen::isa::TargetIsa>,
    function_id: u32,
    helpers: LaneHelpers,
    views: super::ElementViews,
    tiers: &'static Tiers,
    /// The names method caches held at the start, and the code's invalidation word, in a
    /// specialized run (`VmFacts::held_atoms`).
    held: Option<(quench_runtime::vm::method_cache::AtomBits, usize)>,
    /// The names method caches held and the program's constants at the start, and the code's
    /// invalidation word, where the region folds Stable facts (`Division::Online`).
    online: Option<(quench_runtime::vm::method_cache::AtomBits, Vec<quench_runtime::value::Value>, usize)>,
}

/// What a loop region's lowering takes from the VM to fold Stable facts under its code's
/// dependencies (`Division::Online`), as a lane body's does (`FactTables`): the names method caches
/// hold, the running program's constants, and the VM's answers for each plan's unit.
pub(crate) struct OsrOnline<'a> {
    pub(crate) atoms: quench_runtime::vm::method_cache::AtomBits,
    pub(crate) constants: Vec<quench_runtime::value::Value>,
    pub(crate) tables: Box<dyn FnMut(&lane_unit::Unit, &[Option<Vec<NativeFieldLoad>>], &quench_runtime::vm::global_facts::GlobalReads) -> super::lane_compile::FactTables + 'a>,
}

impl LoopJob {
    /// Lowers the first plan that lowers (the inlined unit before the function's own) and
    /// compiles its code; `None` where none lowers.
    fn run(&self) -> Option<LoopLowered> {
        let held = self.held.as_ref().map(|(names, invalidation)| super::HeldAtoms { names, invalidation: *invalidation });
        let construct_plans = |function: u32, pc: u32| self.facts.construct_plans.get(&(function, pc)).cloned();
        let (index, lowered) = self.plans.iter().enumerate().find_map(|(index, plan)| {
            let tables = plan.tables.as_ref();
            let instanceofs = |constructor: quench_runtime::value::Value| tables.and_then(|tables| tables.instanceof(constructor));
            let names = |site: u16, atom| tables.and_then(|tables| tables.name(site, atom));
            let shape_stable = |shape: u32| tables.is_some_and(|tables| tables.shape_stable(shape));
            let own_constant = |atom| tables.and_then(|tables| tables.own_constant(atom));
            let root_constant = |pc: usize| tables.and_then(|tables| tables.root_constant(pc));
            let online = self.online.as_ref().filter(|_| tables.is_some()).map(|(atoms, constants, invalidation)| super::OnlineFacts {
                method_cache_atoms: atoms, invalidation: *invalidation, constants, instanceofs: &instanceofs, shape_stable: &shape_stable,
                names: Some(&names), own_constant: &own_constant, root_constant: &root_constant });
            super::lane_region::RegionSpecializer {
                admitted: self.admitted,
                kinds: &[], int_entries: &self.facts.int_entries, number_entries: &self.facts.double_registers, string_entries: &self.facts.string_registers,
                typed_captures: &self.facts.typed_captures, int_captures: &self.facts.int_captures, typed_kinds: &self.facts.typed_kinds,
                function_id: self.function_id, unit: &plan.unit, helpers: self.helpers, fields: &self.fields,
                global_reads: &plan.global_reads, facts: VmFacts { tiers: self.tiers, held_atoms: held, construct_plans: Some(&construct_plans), online, ..VmFacts::STATIC }, generic: &plan.generic,
                direct: &plan.sites, frame_captures: &self.facts.frame_captures, isa: &*self.isa,
                entry_fields: &plan.entry_fields,
                plan: super::lane_region::RegionPlan { entry_bounds: self.facts.entry_bounds,
                    coverage: super::lane_region::Coverage::Loop { header: plan.span.0, backedge: plan.span.1, poll: true },
                    completion: super::lane_region::Completion::Body, views: self.views,
                },
            }.specialize(&self.program).ok().filter(|lowered| self.views == super::ElementViews::Dense || lowered.hoists_arrays)
                .map(|lowered| (index, lowered))
        })?;
        let header = self.plans[index].span.0 as u32;
        let (first, last) = self.plans[index].span;
        super::lane_facts::note_lane_compile(super::lane_facts::Lowers::Region, (last as usize).saturating_sub(first as usize) + 1, lowered.function.dfg.num_insts());
        super::residuals::report_clif(header, &lowered.function);
        let clif = native_dump_directory().map(|_| lowered.function.display().to_string());
        let code = compile_native_function(lowered.function, &*self.isa, REGION_CODE_BUDGET, |code| Some(code.to_vec()));
        Some(LoopLowered { plan: index, stable: StableFacts { roots: Default::default(), ..lowered.stable },
            roots: lowered.stable.roots, clif, code })
    }
}

/// What a background loop-region compile came to.
pub(crate) enum OsrCompile {
    Done(CompiledOsrRegion),
    /// A fact it took changed meanwhile: the next entry may compile from fresh facts.
    Stale,
    Failed,
}

/// The function's full decoded code as a plain lane unit: the loop coverage selects
/// `header..=backedge`, so every other PC is an exact-PC exit and the loop's PCs stay the
/// function's.
pub(crate) fn loop_unit(function: &Function) -> lane_unit::Unit {
    let instructions: Vec<_> = function.instructions().collect();
    let count = instructions.len();
    lane_unit::Unit {
        instructions,
        origins: (0..count).map(|pc| lane_unit::Origin::Caller(pc as u32)).collect(),
        frames: Vec::new(), markers: Vec::new(),
        caller_registers: function.registers, registers: function.registers,
        end: count as u32,
    }
}

/// Whether the read of `region`'s field `index` answers from the entry record: a static read
/// of a field, never a `length`, whatever its receiver (a register, `this`, a field or a
/// capture): the loop's element stores may change an array's length word, so native code
/// reads it at the read.
fn entry_read(region: &LoopRegion, index: usize, program: &ResidualProgram) -> bool {
    !region.is_dynamic_field(index) && !names_length(program, region.fields[index].atom)
}

/// A plain loop unit's static field reads, by the unit PC each instruction keeps, answered
/// from the entry record.
fn plain_entry_fields(region: &LoopRegion, program: &ResidualProgram) -> Vec<Option<u32>> {
    let header = region.header as usize;
    super::lane_region::flat_table(region.field_reads().iter()
        .filter(|(_, index)| entry_read(region, **index, program))
        .filter_map(|(offset, index)| Some((header + *offset as usize,
            u32::try_from(osr_loop::field_word(region, *index)).ok()?))))
}

/// The loop-invariant receivers whose field reads all saw one exact shape at entry: each register a field read of the loop
/// names and the loop never writes, when every observed load of its reads (`native_field_loads`) holds the same shape. The
/// entry checks that shape once (`Heap::Shape`), so the reads of the receiver keep their known-shape rows.
fn loop_shapes(region: &LoopRegion, observation: &Observation<'_>) -> Vec<super::base_region::function::ShapeProto> {
    let code = region.instructions();
    // Each register's shape, and whether every load of its reads is flat (`Form::Arena`): a later read that disagrees on the
    // shape drops the register; one that is not flat clears the flat flag only.
    let mut shapes: std::collections::BTreeMap<quench_runtime::bytecode::Register, Option<(u32, bool)>> = std::collections::BTreeMap::new();
    for (&offset, &index) in region.field_reads() {
        let Some(FieldLookup::Atom { base, .. }) = code[offset as usize].field_lookup() else { continue };
        let (Some(register), Some(loads)) = (base.register_index(), observation.native_field_loads.get(index)) else { continue };
        // A string's reads are its prototype's: the register holds no object of any shape, and a shape guard would refuse every entry.
        if observation.string_entries.contains(&register) { continue; }
        let Some(shape) = loads.first().map(|load| load.shape).filter(|shape| loads.iter().all(|load| load.shape == *shape)) else { continue };
        let flat = loads.iter().all(|load| quench_runtime::staged::object::Form::of(load.storage, load.storage_len) == quench_runtime::staged::object::Form::Arena);
        let merged = shapes.entry(register).or_insert(Some((shape, flat)));
        *merged = match *merged {
            Some((known, both)) if known == shape => Some((known, both && flat)),
            _ => None,
        };
    }
    shapes.into_iter()
        .filter_map(|(register, known)| {
            let (shape, flat) = known?;
            Some(super::base_region::function::ShapeProto { register, shape, flat })
        })
        .filter(|proto| !code.iter().any(|instruction| instruction.written_registers().any(|written| written == proto.register)))
        .collect()
}

/// The loop's cached field reads, keyed by their field-cache site: the lane specializer's
/// field-site map, from the observation's descriptors.
fn loop_fields(region: &LoopRegion, observation: &Observation<'_>)
    -> Vec<Option<Vec<NativeFieldLoad>>> {
    let reads = region.field_reads().iter().filter_map(|(&offset, &index)| {
        let Some(FieldLookup::Atom { cache_site, .. }) =
            region.instructions()[offset as usize].field_lookup() else { return None };
        Some((usize::from(cache_site), observation.native_field_loads.get(index)?.clone()))
    });
    let stores = observation.store_loads.iter().map(|(site, loads)| (usize::from(*site), loads.clone()));
    super::lane_region::flat_table(reads.chain(stores))
}

#[quench_macros::ext(OsrFactsOps2)]
impl OsrFacts {
    /// The facts the observation's field descriptors embed in code compiled from it.
    fn observed(observation: &Observation<'_>) -> Self {
        let loads = || observation.native_field_loads.iter().flatten();
        let mut shape_dependencies: Vec<_> = loads().flat_map(|field| std::iter::once(field.shape).chain(field.holder)).collect();
        shape_dependencies.sort_unstable();
        shape_dependencies.dedup();
        let mut departure_shapes: Vec<_> = loads().filter_map(|field| field.holder).collect();
        departure_shapes.sort_unstable();
        departure_shapes.dedup();
        let mut slot_dependencies: Vec<_> = loads().filter_map(|field| field.slot_dependency()).collect();
        slot_dependencies.sort_unstable_by_key(|dependency| (dependency.shape, dependency.slot));
        slot_dependencies.dedup();
        Self { shape_dependencies, departure_shapes, slot_dependencies, roots: Default::default(), stable: StableFacts::default(), calls: LaneDependencies::default() }
    }
}

}

/// The plans a loop's lowering may take, in order: the inlined unit where the region has one,
/// then the function's own unit over the loop. One plan lowers a loop, the region specializer,
/// the same plan a lane body and a static typed region use; a region it declines stays in the
/// interpreter, with no second lowering with its own facts, widenings and entry record. Each
/// plan calls the region's direct callees by their entries (the second value, per plan: the code
/// of the plan taken owns them).
#[allow(clippy::too_many_arguments)]
fn loop_inputs(program: &ResidualProgram, region: &LoopRegion, function_id: u32, observation: &Observation<'_>,
    global_reads: &mut dyn FnMut(&lane_unit::Unit, GlobalTable) -> quench_runtime::vm::global_facts::GlobalReads, widened: &[usize],
    unit_calls: &[DirectCandidates], (fields, mut online): (&[Option<Vec<NativeFieldLoad>>], Option<&mut OsrOnline<'_>>)) -> (Vec<LoopInput>, Vec<Box<[DirectCallee]>>) {
    let mut input = |unit: lane_unit::Unit, entry_fields: Vec<Option<u32>>, span: (usize, usize), generic: Vec<usize>, sites| {
        let global_reads = global_reads(&unit, GlobalTable::Lane);
        let tables = online.as_mut().map(|online| (online.tables)(&unit, fields, &global_reads));
        LoopInput { unit, tables, entry_fields, span, generic, global_reads, sites }
    };
    let (mut plans, mut directs) = (Vec::new(), Vec::new());
    if let Some((unit, fields)) = region.inlining.is_some().then(|| inlining_unit(region, program)).flatten() {
        // A unit that inlined no frame calls its callees by their entries as the plain unit does:
        // its leaves write no inlined registers, so the lane's scratch window is its callees' alone.
        let (direct, sites) = if unit.frames.is_empty() { super::lane_facts::unit_direct_calls(&unit, unit_calls) } else { Default::default() };
        let span = (0, unit.instructions.len().saturating_sub(1));
        plans.push(input(unit, fields, span, Vec::new(), sites));
        directs.push(direct);
    }
    let plain = loop_unit(program.function(function_id as usize));
    let (direct, sites) = super::lane_facts::unit_direct_calls(&plain, unit_calls);
    // The plain unit's PCs are the function's, which the site's leaves name.
    plans.push(input(plain, plain_entry_fields(region, program), (region.header as usize, region.backedge as usize), widened.to_vec(), sites));
    directs.push(direct);
    (plans, directs)
}

#[quench_macros::ext(PendingOsrRegionOps1)]
impl PendingOsrRegion {

    /// Subscribes its watch to the facts its code names; whether they hold.
    pub(crate) fn watch(&self, facts: &mut quench_runtime::vm::tier_queries::FactWatch<'_>,
        programs: &quench_runtime::vm::program_store::ProgramStore) -> bool {
        facts.hold_roots(&self.watch, self.facts.roots.clone());
        let watch: Rc<dyn quench_runtime::code_dependencies::DependentCode> = self.watch.clone();
        self.facts.subscribe(&watch, facts) && programs.subscribe_root_constants(&self.facts.root_programs(), &watch)
    }

    /// The watch a collection that frees any of the callees its code names retires, and those callees
    /// (`ProgramStore::note_pending_native_calls`): while it lowers, those of every plan it may take.
    pub(crate) fn native_calls(&self) -> (Rc<quench_runtime::code_dependencies::PublishedCode<()>>, Box<[quench_runtime::value::Value]>) {
        let identities = match &self.calls {
            Some(calls) => calls.identities(),
            None => self.directs.iter().flat_map(|direct| direct.iter().map(|callee| callee.callee)).collect(),
        };
        (self.watch.clone(), identities)
    }

    /// Makes it stale where its code names shape ids, which a compaction renumbers.
    pub(crate) fn retire_named_shapes(&self) {
        if !self.facts.shape_dependencies.is_empty() { self.watch.retire(); }
    }

    /// What its compile came to, or itself while its code lowers and compiles. A Cranelift
    /// region's Stable facts and roots are its lowering's, which the watch takes here: a fact
    /// that changed while it lowered (a method cache naming an atom it stores in place) fails
    /// the subscription and the code is stale.
    /// A plan that folded Stable facts publishes where the VM still answers what it took (`TakenPlan::holds`).
    pub(crate) fn poll(mut self: Box<Self>, vm: &mut quench_runtime::vm::Vm, program: Option<&ResidualProgram>) -> Result<OsrCompile, Box<Self>> {
        let lowered = match self.code.poll() {
            std::task::Poll::Pending => return Err(self),
            std::task::Poll::Ready(_) if !self.watch.is_published() => return Ok(OsrCompile::Stale),
            std::task::Poll::Ready(Some(Some(lowered))) => lowered,
            std::task::Poll::Ready(_) => return Ok(OsrCompile::Failed),
        };
        let LoopLowered { plan, stable, roots, clif, code } = lowered;
        if self.taken.get(plan).and_then(Option::as_ref).is_some_and(|taken| !program.is_some_and(|program| taken.holds(vm, program, self.function))) {
            return Ok(OsrCompile::Stale);
        }
        let (mut facts, programs) = vm.fact_watch();
        let Ok(code) = code else { return Ok(OsrCompile::Failed) };
        let calls = match self.calls.take() {
            Some(calls) => calls,
            None => {
                let (registers, invalidation) = self.call_frame.clone();
                let direct = self.directs.get_mut(plan).map(std::mem::take).unwrap_or_default();
                let calls = if direct.is_empty() { CallCode::holding(invalidation) } else { CallCode::of((Vec::new(), direct, Vec::new()), registers, invalidation) };
                self.facts.roots = roots;
                self.facts.stable = stable;
                self.facts.calls = LaneDependencies::of(std::iter::empty(), &calls.direct);
                facts.hold_roots(&self.watch, self.facts.roots.clone());
                let watch: Rc<dyn quench_runtime::code_dependencies::DependentCode> = self.watch.clone();
                if !self.facts.calls.subscribe(&self.facts.stable, &watch, &mut facts)
                    || !programs.subscribe_root_constants(&self.facts.root_programs(), &watch) {
                    return Ok(OsrCompile::Stale);
                }
                calls
            }
        };
        let Self { header, facts, crossing, resumes, .. } = *self;
        let Some((memory, entry)) = publish_body(&code) else { return Ok(OsrCompile::Failed) };
        super::entry_cell::reserve_lane_scratch(calls.window());
        if let (Some(directory), Some(clif)) = (native_dump_directory(), clif) {
            dump_native_code(directory, DumpSite::Loop { header }, &memory, &clif);
        }
        Ok(OsrCompile::Done(CompiledOsrRegion { code: memory, entry, facts, crossing, resumes, calls, parked: std::cell::Cell::new(None), made: std::cell::Cell::new(false),
            region_calls: std::cell::OnceCell::new(), dense: std::cell::Cell::new(Vec::new()),
            stamp: std::cell::Cell::new(None) }))
    }
}


#[cfg(test)]
#[path = "osr_compile/tests.rs"]
pub(crate) mod tests;
#[cfg(test)]
#[path = "osr_compile/method_tests.rs"]
pub(crate) mod method_tests;
#[cfg(test)]
#[path = "osr_compile/math_replacement_tests.rs"]
pub(crate) mod math_replacement_tests;
