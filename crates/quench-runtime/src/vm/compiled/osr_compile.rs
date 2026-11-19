//! Loop-region code compiled beside the program: the loop runs in the lane while the code
//! generator's thread compiles, and the next look publishes the code.

use super::{
    compile_native_function, dump_native_bytes, dump_native_code, DumpSite, names_length, native_dump_directory, lane_unit, osr_loop,
    publish_body, tier_policy, CompiledOsrRegion, LaneHelpers, LoopRegion,
    NativeCompileRejection, NativeFieldLoad, Observation, VmFacts,
};
use super::base_region::function::{build_region, Body, BodySpec};
use super::lane_compile::LaneDependencies;
use super::tier_policy::Tiers;
use super::lane_region::StableFacts;
use crate::code_dependencies::Invalidation;
mod calls;
mod inlined_unit;
use inlined_unit::*;
pub(in crate::vm) use calls::{CallCode, RegionCalls};
use crate::bytecode::{FieldLookup, Function, Op, ResidualProgram, WideInstruction};
use crate::vm::global_facts::GlobalTable;
use super::super::specializer::Specializer;
use super::{DirectCallee, DirectCandidates};
use std::rc::Rc;

/// What a region's code relies on beyond its entry guards: the exits that widen a value,
/// and the facts its own-field descriptors embed, which retire it when they change.
pub(in crate::vm) struct OsrFacts {
    /// Shape IDs the code embeds, the receiver's included. Shape compaction retires this code
    /// through its lane view before execution can resume with remapped IDs.
    pub(in crate::vm) shape_dependencies: Vec<u32>,
    /// Holder shapes: their layout is read without the receiver-shape guard, so their
    /// departure discards the compile.
    departure_shapes: Vec<u32>,
    slot_dependencies: Vec<crate::code_dependencies::SlotDependency>,
    /// The heap identities the code names (a call's callee), which its publication keeps alive.
    pub(in crate::vm) roots: crate::folded_roots::FoldedRoots,
    /// The Stable facts a BaseJIT region folded: atoms its stores run in place for, which no method
    /// cache names (a cache naming one retires it, `MethodCacheAtoms::name`), and the global cells
    /// that hold the callees it inlines.
    stable: StableFacts,
    /// What the callees its calls name depend on (`LaneDependencies::of`).
    calls: LaneDependencies,
}

impl OsrFacts {

    /// Subscribes `code` to the facts; whether all hold. A receiver shape is guarded at each
    /// field use, so only a holder shape's departure discards it; a receiver shape ID still
    /// retires it at compaction (`shape_dependencies`).
    pub(in crate::vm) fn subscribe(&self, code: &Rc<dyn crate::code_dependencies::DependentCode>, heap: &mut crate::heap::Heap,
        method_atoms: &mut crate::code_dependencies::CodeReaders) -> bool {
        self.departure_shapes.iter().all(|shape| heap.subscribe_shape(*shape, code))
            && self.slot_dependencies.iter().all(|dependency| heap.subscribe_slot_observation(*dependency, code))
            && self.calls.subscribe(&self.stable, code, heap, method_atoms)
    }
}

/// A loop region whose code compiles on the code generator's thread, as V8 compiles OSR code
/// concurrently: compiling on the loop's own thread stalled it 2 ms a region (Box2D --jit:
/// 34 ms of a 733 ms run). Its facts are the observation's at the start, to which `watch`
/// subscribes: a shape compaction or a slot widening meanwhile discards the code.
pub(in crate::vm) struct PendingOsrRegion {
    code: super::super::jit_backend::Pending<Result<Vec<u8>, NativeCompileRejection>>,
    watch: Rc<crate::code_dependencies::PublishedCode<()>>,
    header: u32,
    clif: Option<String>,
    facts: OsrFacts,
    /// What a run of the code builds around it.
    crossing: super::Crossing,
    /// Where a run may re-enter the code mid-region.
    resumes: super::base_region::ResumeTable,
    /// What its code owns of its calls (`CallCode`).
    calls: CallCode,
}

/// What a background loop-region compile came to.
pub(in crate::vm) enum OsrCompile {
    Done(CompiledOsrRegion),
    /// A fact it took changed meanwhile: the next entry may compile from fresh facts.
    Stale,
    Failed,
}

/// The function's full decoded code as a plain lane unit: the loop coverage selects
/// `header..=backedge`, so every other PC is an exact-PC exit and the loop's PCs stay the
/// function's.
pub(in crate::vm) fn loop_unit(function: &Function) -> lane_unit::Unit {
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
    let mut shapes: std::collections::BTreeMap<crate::bytecode::Register, Option<u32>> = std::collections::BTreeMap::new();
    for (&offset, &index) in region.field_reads() {
        let Some(FieldLookup::Atom { base, .. }) = code[offset as usize].field_lookup() else { continue };
        let (Some(register), Some(loads)) = (base.register_index(), observation.native_field_loads.get(index)) else { continue };
        let Some(shape) = loads.first().map(|load| load.shape).filter(|shape| loads.iter().all(|load| load.shape == *shape)) else { continue };
        let merged = shapes.entry(register).or_insert(Some(shape));
        if *merged != Some(shape) { *merged = None; }
    }
    shapes.into_iter()
        .filter_map(|(register, shape)| Some(super::base_region::function::ShapeProto { register, shape: shape? }))
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

/// A store site the observation names no load for, whose receiver register the loop just read the
/// same field of (`o.f = o.f + x`), takes that read's loads: the store meets the shapes the read
/// met, whose facts the region already names, and its row guards the shape it holds.
fn stored_after_reads(instructions: &[crate::bytecode::WideInstruction], fields: &mut Vec<Option<Vec<NativeFieldLoad>>>) {
    for (at, store) in instructions.iter().enumerate() {
        let Some(receiver) = store.field_store_register() else { continue };
        let site = usize::from(store.cache_site_index());
        if fields.get(site).is_some_and(|loads| loads.as_ref().is_some_and(|loads| !loads.is_empty())) { continue }
        let atom = store.atom_index();
        let read = instructions[..at].iter().rev().take_while(|earlier| earlier.defined_register() != Some(receiver))
            .find_map(|earlier| match earlier.op().is_get_field().then(|| earlier.field_lookup()).flatten() {
                Some(FieldLookup::Atom { atom: read, cache_site, .. }) if read == atom && earlier.register_b() == receiver => Some(cache_site),
                _ => None,
            });
        let Some(loads) = read.and_then(|read| fields.get(usize::from(read)).cloned().flatten()) else { continue };
        if fields.len() <= site { fields.resize(site + 1, None) }
        fields[site] = Some(loads);
    }
}

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

/// The element reads through an array observed to hold int32s or Numbers (`number_arrays`), where
/// no earlier body left (`generic`) and none was `abandoned`: typed, each leaving at an element
/// that is not one. A lane body's observation is its entry activation's array parameters.
pub(super) fn observed_reads(program: &ResidualProgram, unit: &lane_unit::Unit, number_arrays: &[(crate::bytecode::Register, crate::typed_ir::Type)],
    abandoned: &[usize], generic: &[usize]) -> Vec<super::facts::ObservedRead> {
    let mut reads: Vec<super::facts::ObservedRead> = unit.instructions.iter().enumerate()
        .filter(|(pc, instruction)| instruction.op().is_get_index() && !abandoned.contains(pc))
        .filter_map(|(pc, instruction)| {
            let register = instruction.operand_b().register_index()?;
            let ty = number_arrays.iter().find(|(array, _)| *array == register)?.1;
            // An inlined callee's reads are Numbers whatever the entry showed: its registers
            // hold nothing at entry that the sample could have seen. So is a read that left
            // once: an int32 observation it outgrew widens to Numbers before it is dropped.
            let ty = if unit.frames.is_empty() && !generic.contains(&pc) { ty } else { crate::typed_ir::Type::NUMBER };
            Some(super::facts::ObservedRead { pc, ty })
        }).collect();
    // A register's `length` read defines an int32: the region reads it as one
    // (`Lowering::array_length`), leaving where it is not.
    reads.extend(unit.instructions.iter().enumerate().filter(|(_, instruction)| super::reads_length(program, **instruction)
        && instruction.result_flags() == 0
        && matches!(instruction.field_lookup(), Some(crate::bytecode::FieldLookup::Atom { base, .. }) if base.register_index().is_some()))
        .map(|(pc, _)| super::facts::ObservedRead { pc, ty: crate::typed_ir::Type::INT32 }));
    reads.sort_by_key(|read| read.pc);
    reads
}

/// What lowers a loop's unit: the region specializer, the same plan a lane body and a static
/// typed region use. A region it declines stays in the interpreter; there is no second
/// lowering with its own facts, widenings and entry record.
struct LoopPlanner<'a> {
    observation: &'a Observation<'a>,
    fields: &'a super::lane_region::FieldSites,
    isa: &'a dyn cranelift_codegen::isa::TargetIsa,
    program: &'a ResidualProgram,
    function_id: u32,
    helpers: LaneHelpers,
    views: super::ElementViews,
    tiers: &'static Tiers,
    /// The names method caches hold now, in a specialized run (`VmFacts::held_atoms`).
    stores: Option<super::HeldAtoms<'a>>,
    /// By function PC, the callees each call site reached: the lowered unit calls them by their
    /// entries (`unit_direct_calls`).
    unit_calls: &'a [DirectCandidates],
}

impl LoopPlanner<'_> {
    /// The function's own unit run over the loop is the general plan: a call inside it leaves
    /// at the call's exact PC, with no callee inlined. The inlined unit is an optimization over
    /// that plan, used only where the general plan declines; an inlined callee the region
    /// cannot prove exactly then costs the region nothing but the inlining.
    fn plan(&self, global_reads: &mut dyn FnMut(&lane_unit::Unit, GlobalTable) -> crate::vm::global_facts::GlobalReads,
        unit: &lane_unit::Unit, entry_fields: &super::lane_region::EntryWords, (header, backedge): (usize, usize),
        generic: &[usize], direct: &super::lane_region::DirectSites) -> Option<super::lane_region::LoweredRegion> {
        let number_reads = observed_reads(self.program, unit, self.observation.number_arrays, self.observation.abandoned, generic);
        super::lane_region::RegionSpecializer {
            number_reads: &number_reads, int_entries: self.observation.int_entries, number_entries: self.observation.double_registers, typed_captures: self.observation.typed_captures, int_captures: self.observation.int_captures,
            function_id: self.function_id, unit, helpers: self.helpers, fields: self.fields,
            global_reads: &global_reads(unit, GlobalTable::Lane), facts: VmFacts { tiers: self.tiers, held_atoms: self.stores, construct_plans: self.observation.construct_plans, ..VmFacts::STATIC }, generic,
            direct, frame_captures: self.observation.frame_captures, isa: self.isa,
            entry_fields,
            plan: super::lane_region::RegionPlan { entry_bounds: self.observation.entry_bounds,
                coverage: super::lane_region::Coverage::Loop { header, backedge, poll: true },
                completion: super::lane_region::Completion::Body, views: self.views,
            },
        }.specialize(self.program).ok().filter(|lowered| self.views == super::ElementViews::Dense || lowered.hoists_arrays)
    }
}

/// A lane region carries each inlined callee's full body and its guards, so a loop
/// that inlines two calls may legitimately exceed one function's code budget. The code
/// is published once per hot loop and retires with its site, so the larger per-site
/// bound stays bounded by the lane plan's own admission.
const REGION_CODE_BUDGET: usize = 2 * osr_loop::MAX_CODE_BYTES;

/// The BaseJIT tier's answer for `region`: `Some(code)` where it builds or (under `--b-jit`) declines the
/// region, `None` where Cranelift compiles it. A region BaseJIT builds skips Cranelift: every instruction has a lowering (`base_region::steps`).
/// Its small monomorphic calls run as their callees' rows (`calls`, the function's and the realm's they were found in).
fn fast_pending(region: &LoopRegion, views: super::ElementViews, known: super::base_region::Known, widened: &[usize], observation: &Observation<'_>, helpers: LaneHelpers,
    (function, calls, invalidation): (u32, RegionCalls, Invalidation)) -> Option<Option<PendingOsrRegion>> {
    let base_only = crate::jit_mode::base_only();
    if !crate::jit_mode::base_regions() || region.inlining.is_some() {
        return base_only.then_some(None);
    }
    let registers = known.program.function(function as usize).registers;
    let seen = super::base_region::observed_types(registers, observation.int_registers, observation.double_registers);
    let spec = BodySpec { function, registers, known, realm: calls.realm, widened, seen: &seen, kinds: observation.operand_kinds };
    // SAFETY: `calls` was made of the call sites' records this compile (`Vm::region_calls`), whose entry cells are live.
    let built = unsafe { build_region(region, &spec, views, &calls.callees, helpers) };
    let Ok(Body { code, bare, frames, records, stable, atoms, holders, layouts, slot_types, resumes, .. }) = built else { return base_only.then_some(None) };
    if let Some(directory) = native_dump_directory() {
        dump_native_bytes(directory, DumpSite::Loop { header: region.header }, &code, None, "");
    }
    let calls = CallCode::of((frames, calls.callees.owned, records), registers, invalidation);
    let facts = OsrFacts { stable: StableFacts { globals: stable, holders, atoms, layouts, slots: slot_types, ..Default::default() },
        calls: LaneDependencies::of(std::iter::empty(), &calls.direct), ..OsrFacts::observed(observation) };
    let watch = Rc::new(crate::code_dependencies::PublishedCode::new(()));
    let code = super::super::jit_backend::run_in_background(move || Ok(code));
    Some(Some(PendingOsrRegion { code, watch, header: region.header, clif: None, crossing: if bare { super::Crossing::Bare } else { super::Crossing::Lean }, resumes, facts, calls }))
}

/// Lowers the loop: the inlined unit where the region has one, else (or where that plan
/// declines) the function's own unit over the loop. One plan lowers a loop, the region
/// specializer, the same plan a lane body and a static typed region use; a region it declines
/// stays in the interpreter, with no second lowering with its own facts, widenings and entry
/// record. The plain unit calls the region's direct callees by their entries (`planner.direct`);
/// the second value says it lowered, so that its code owns them.
fn lower_loop(planner: &LoopPlanner<'_>, region: &LoopRegion,
    global_reads: &mut dyn FnMut(&lane_unit::Unit, GlobalTable) -> crate::vm::global_facts::GlobalReads, widened: &[usize],
) -> Option<(super::lane_region::LoweredRegion, Box<[DirectCallee]>)> {
    let program = planner.program;
    let mut plan = |unit: &lane_unit::Unit, entry_fields: &super::lane_region::EntryWords, header: usize, backedge: usize, generic: &[usize], direct: &super::lane_region::DirectSites|
        planner.plan(&mut *global_reads, unit, entry_fields, (header, backedge), generic, direct);
    let inlined = region.inlining.is_some().then(|| inlining_unit(region, program)).flatten();
    inlined
        .as_ref()
        .and_then(|(unit, fields)| {
            // A unit that inlined no frame calls its callees by their entries as the plain unit does:
            // its leaves write no inlined registers, so the lane's scratch window is its callees' alone.
            let (direct, sites) = if unit.frames.is_empty() { super::lane_facts::unit_direct_calls(unit, planner.unit_calls) } else { Default::default() };
            plan(unit, fields.as_slice(), 0, unit.instructions.len().saturating_sub(1), &[], &sites).map(|lowered| (lowered, direct))
        })
        .or_else(|| {
            let plain = loop_unit(program.function(planner.function_id as usize));
            let (direct, sites) = super::lane_facts::unit_direct_calls(&plain, planner.unit_calls);
            // The plain unit's PCs are the function's, which the site's leaves name.
            plan(&plain, plain_entry_fields(region, program).as_slice(), region.header as usize, region.backedge as usize, widened, &sites)
                .map(|lowered| (lowered, direct))
        })
}

/// Starts compiling `region`'s code over its entry's element `views` from `observation`, or
/// `None` where it lowers no code (a typed plan hoisting no view would leave at an access).
pub(in crate::vm) fn start_osr_region(
    region: &LoopRegion, views: super::ElementViews,
    observation: &Observation<'_>,
    (policy, tiers): (crate::MemoryPolicy, &'static Tiers),
    program: &ResidualProgram,
    function_id: u32,
    helpers: LaneHelpers,
    global_reads: &mut dyn FnMut(&lane_unit::Unit, GlobalTable) -> crate::vm::global_facts::GlobalReads,
    (widened, stores): (&[usize], Option<&crate::vm::method_cache::AtomBits>),
    mut calls: RegionCalls,
) -> Option<PendingOsrRegion> {
    let isa = super::super::jit_backend::target_isa().filter(|_| tier_policy::by_policy(policy).loop_tier)?;
    // The word the code reads where a call may have changed a Stable fact it folded (`Known::invalidation`).
    let invalidation = Invalidation::default();
    // The b-jit's rows hold global slots where the VM and the program run specialized (`stores` is that fact).
    let globals = (crate::jit_mode::base_regions() && region.inlining.is_none() && stores.is_some()).then(|| global_reads(&loop_unit(program.function(function_id as usize)), GlobalTable::Base));
    // The field sites' loads the observation recorded, which its facts name (`OsrFacts::observed`): a
    // shape compaction retires the region, so its inline-cache rows may patch these shapes in.
    let fields = loop_fields(region, observation);
    let mut row_fields = fields.clone();
    stored_after_reads(region.instructions(), &mut row_fields);
    let reads = |site: u16| row_fields.get(usize::from(site)).cloned().flatten().unwrap_or_default();
    let reads = stores.is_some().then_some(&reads as &dyn Fn(u16) -> Vec<NativeFieldLoad>);
    let shapes = loop_shapes(region, observation);
    let known =super::base_region::Known { program, stores, reads, adds: None, names: None, invalidation: Some(invalidation.address()), constants: &[], typeof_names: &[], instanceofs: None, presences: None, globals: globals.as_ref(), stitch: &tiers.stitch, classes: &[], shapes: &shapes };
    let unit_calls = std::mem::take(&mut calls.direct);
    if let Some(decided) = fast_pending(region, views, known, widened, observation, helpers, (function_id, calls, invalidation.clone())) {
        return decided;
    }
    // The calls' callees, by the sites that reached them: the lowered unit calls them by their entries.
    let caller = program.function(function_id as usize);
    let held = stores.map(|names| super::HeldAtoms { names, invalidation: invalidation.address() });
    let planner = LoopPlanner { observation, fields: &fields, isa: &*isa, program, function_id, helpers, views, tiers, stores: held, unit_calls: &unit_calls };
    let (lowered, direct) = lower_loop(&planner, region, global_reads, widened)?;
    let function = lowered.function;
    super::residuals::report_clif(region.header, &function);
    let clif = native_dump_directory().map(|_| function.display().to_string());
    let stable = StableFacts { atoms: lowered.stable.atoms, holders: lowered.stable.holders, ..StableFacts::default() };
    let calls = if !direct.is_empty() { CallCode::of((Vec::new(), direct, Vec::new()), caller.registers, invalidation) } else { CallCode::holding(invalidation) };
    let facts = OsrFacts { roots: lowered.stable.roots, stable, calls: LaneDependencies::of(std::iter::empty(), &calls.direct), ..OsrFacts::observed(observation) };
    let code = super::super::jit_backend::run_in_background(move || {
        compile_native_function(function, &*isa, REGION_CODE_BUDGET, |code| Some(code.to_vec()))
    });
    let watch = Rc::new(crate::code_dependencies::PublishedCode::new(()));
    Some(PendingOsrRegion { code, watch, header: region.header, clif, facts, crossing: super::Crossing::General, resumes: super::base_region::ResumeTable::empty(), calls })
}

impl PendingOsrRegion {

    /// Subscribes its watch to the facts its code names; whether they hold.
    pub(in crate::vm) fn watch(&self, heap: &mut crate::heap::Heap, method_atoms: &mut crate::code_dependencies::CodeReaders) -> bool {
        self.watch.hold_roots(self.facts.roots.clone(), heap);
        let watch: Rc<dyn crate::code_dependencies::DependentCode> = self.watch.clone();
        self.facts.subscribe(&watch, heap, method_atoms)
    }

    /// The watch a collection that frees any of the callees its code names retires, and those callees
    /// (`ProgramStore::note_pending_native_calls`).
    pub(in crate::vm) fn native_calls(&self) -> (Rc<crate::code_dependencies::PublishedCode<()>>, Box<[crate::value::Value]>) {
        (self.watch.clone(), self.calls.identities())
    }

    /// Makes it stale where its code names shape ids, which a compaction renumbers.
    pub(in crate::vm) fn retire_named_shapes(&self) {
        if !self.facts.shape_dependencies.is_empty() { self.watch.retire(); }
    }

    /// What its compile came to, or itself while its code compiles.
    pub(in crate::vm) fn poll(self: Box<Self>) -> Result<OsrCompile, Box<Self>> {
        let code = match self.code.poll() {
            std::task::Poll::Pending => return Err(self),
            std::task::Poll::Ready(_) if !self.watch.is_published() => return Ok(OsrCompile::Stale),
            std::task::Poll::Ready(Some(Ok(code))) => code,
            std::task::Poll::Ready(_) => return Ok(OsrCompile::Failed),
        };
        let Self { header, clif, facts, crossing, resumes, calls, .. } = *self;
        let Some((memory, entry)) = publish_body(&code) else { return Ok(OsrCompile::Failed) };
        super::entry_cell::reserve_lane_scratch(calls.window());
        if let (Some(directory), Some(clif)) = (native_dump_directory(), clif) {
            dump_native_code(directory, DumpSite::Loop { header }, &memory, &clif);
        }
        Ok(OsrCompile::Done(CompiledOsrRegion { code: memory, entry, facts, crossing, resumes, calls, parked: std::cell::Cell::new(None), made: std::cell::Cell::new(false) }))
    }
}


#[cfg(test)]
#[path = "osr_compile/tests.rs"]
mod tests;
