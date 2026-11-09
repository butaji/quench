//! Loop-region code compiled beside the program: the loop runs in the lane while the code
//! generator's thread compiles, and the next look publishes the code.

use super::{
    compile_native_function, dump_native_code, native_dump_directory, lane_unit, osr_loop, poll_address,
    publish_entry, tier_policy, CompiledOsrRegion, LaneHelpers, LoopRegion,
    NativeCompileRejection, NativeFieldLoad, Observation, VmFacts, Widening,
};
use crate::bytecode::{FieldLookup, Function, ResidualProgram};
use super::super::specializer::Specializer;
use std::collections::BTreeMap;
use std::rc::Rc;

/// What a region's code relies on beyond its entry guards: the exits that widen a value,
/// and the facts its own-field descriptors embed, which retire it when they change.
pub(in crate::vm) struct OsrFacts {
    /// By exit PC, in order.
    widenings: Box<[(u32, Widening)]>,
    /// Shape IDs the code embeds, the receiver's included. Shape compaction retires this code
    /// through its lane view before execution can resume with remapped IDs.
    pub(in crate::vm) shape_dependencies: Vec<u32>,
    /// Holder shapes: their layout is read without the receiver-shape guard, so their
    /// departure discards the compile.
    departure_shapes: Vec<u32>,
    slot_dependencies: Vec<crate::code_dependencies::SlotDependency>,
}

impl OsrFacts {
    /// The value the code held as an `i32` that its exit at `pc` found none.
    pub(super) fn widening(&self, pc: u32) -> Option<Widening> {
        let found = self.widenings.binary_search_by_key(&pc, |&(exit, _)| exit).ok()?;
        Some(self.widenings[found].1)
    }

    /// Subscribes `code` to the facts; whether all hold. A receiver shape is guarded at each
    /// field use, so only a holder shape's departure discards it; a receiver shape ID still
    /// retires it at compaction (`shape_dependencies`).
    pub(in crate::vm) fn subscribe(&self, code: &Rc<dyn crate::code_dependencies::DependentCode>, heap: &mut crate::heap::Heap) -> bool {
        self.departure_shapes.iter().all(|shape| heap.subscribe_shape(*shape, code))
            && self.slot_dependencies.iter().all(|dependency| heap.subscribe_slot_observation(*dependency, code))
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
    /// Whether the code uses the lane-region entry ABI (`Completion::StatusAndPc`),
    /// rather than the loop region's own entry record.
    lane: bool,
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
fn loop_unit(function: &Function) -> lane_unit::Unit {
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

/// The loop's cached field reads, keyed by their field-cache site: the lane specializer's
/// field-site map, from the observation's descriptors.
fn loop_fields(region: &LoopRegion, observation: &Observation<'_>)
    -> BTreeMap<u16, Vec<NativeFieldLoad>> {
    let mut fields = BTreeMap::new();
    for (&offset, &index) in region.field_reads() {
        let Some(FieldLookup::Atom { cache_site, .. }) =
            region.instructions()[offset as usize].field_lookup() else { continue };
        if let Some(loads) = observation.native_field_loads.get(index) {
            fields.insert(cache_site, loads.clone());
        }
    }
    fields
}

/// Starts compiling `region`'s code from `observation`, or `None` where it lowers no code.
pub(in crate::vm) fn start_osr_region(
    region: &LoopRegion,
    observation: &Observation<'_>,
    interrupt: &crate::execution_interrupt::ExecutionInterrupt,
    policy: crate::MemoryPolicy,
    program: &ResidualProgram,
    function_id: u32,
    helpers: LaneHelpers,
) -> Option<PendingOsrRegion> {
    let isa = super::super::jit_backend::target_isa().filter(|_| tier_policy::by_policy(policy).loop_tier)?;
    let unit = loop_unit(program.function(function_id as usize));
    let fields = loop_fields(region, observation);
    // The lane unit models the loop's registers and locals; its captures, fields and
    // arrays still come from the loop entry record, so those regions keep the private
    // lowering until the lane unit carries them too.
    let simple = region.inlining.is_none()
        && region.captures.is_empty()
        && region.field_store_sites.is_empty()
        && !region.reads_heap_natively();
    let lane = simple.then(|| super::lane_region::RegionSpecializer {
        function_id, unit: &unit, helpers, fields: &fields,
        global_reads: &BTreeMap::new(), facts: VmFacts::STATIC, generic: &[],
        direct: &BTreeMap::new(), frame_captures: &[], isa: &*isa,
        plan: super::lane_region::RegionPlan {
            coverage: super::lane_region::Coverage::Loop {
                header: region.header as usize, backedge: region.backedge as usize, poll: false },
            completion: super::lane_region::Completion::StatusAndPc,
            views: super::lane_region::ElementViews::Dense,
        },
    }.specialize(program).ok()).flatten();
    let (function, widenings, lane) = match lane {
        Some(region) => (region.function, BTreeMap::new(), true),
        // The loop tier's private lowering remains for an inlining region and for a loop
        // whose captures, fields or arrays the lane unit does not carry yet.
        None if !simple => {
            let verified = osr_loop::verify_region(region, observation).ok()?;
            let (function, widenings) =
                osr_loop::lower_verified(verified, observation, &*isa, poll_address(interrupt))?;
            (function, widenings, false)
        }
        None => return None,
    };
    let clif = native_dump_directory().map(|_| function.display().to_string());
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
    let code = super::super::jit_backend::run_in_background(move || {
        compile_native_function(function, &*isa, osr_loop::MAX_CODE_BYTES, |code| Some(code.to_vec()))
    });
    let watch = Rc::new(crate::code_dependencies::PublishedCode::new(()));
    Some(PendingOsrRegion { code, watch, header: region.header, clif, lane,
        facts: OsrFacts { widenings: widenings.into_iter().collect(), shape_dependencies, departure_shapes, slot_dependencies } })
}

impl PendingOsrRegion {
    /// Whether the plan uses the shared lane-region entry ABI (tests).
    #[cfg(test)]
    pub(super) fn uses_lane(&self) -> bool { self.lane }

    /// Subscribes its watch to the facts its code names; whether they hold.
    pub(in crate::vm) fn watch(&self, heap: &mut crate::heap::Heap) -> bool {
        let watch: Rc<dyn crate::code_dependencies::DependentCode> = self.watch.clone();
        self.facts.subscribe(&watch, heap)
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
        let Self { header, clif, facts, lane, .. } = *self;
        let Some((memory, entry)) = publish_entry(&code) else { return Ok(OsrCompile::Failed) };
        if let (Some(directory), Some(clif)) = (native_dump_directory(), clif) {
            dump_native_code(directory, header, &memory, &clif);
        }
        Ok(OsrCompile::Done(CompiledOsrRegion { _code: memory, entry, facts, lane }))
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// A simple numeric loop plans through `RegionSpecializer` with `Coverage::Loop`, not
    /// through the loop tier's private lowering.
    #[test]
    fn a_simple_loop_is_planned_by_the_region_specializer() {
        let program = crate::Engine::specialize(
            "function f(n){let s=0;for(let i=0;i<n;i++){s=s+i;}return s;} f(4);",
            "loop-plan.js").unwrap();
        let id = program.functions.iter().position(|body|
            body.header().name.is_some_and(|name| &program.atoms[name as usize] == "f")).unwrap() as u32;
        let function = program.function(id as usize);
        let decoded: Vec<_> = function.instructions().collect();
        let (header, backedge) = decoded.iter().enumerate().find_map(|(pc, instruction)| {
            (instruction.op().immediate_role() == crate::bytecode::ImmediateRole::JumpTarget
                && (instruction.jump_target() as usize) <= pc)
                .then(|| (instruction.jump_target(), pc as u32))
        }).expect("the function has a loop");
        let region = osr_loop::region(function, &decoded, &[], &[], None, header, backedge)
            .expect("the loop is admitted");
        let observation = Observation {
            register: Some(&|_| crate::Value::UNDEFINED),
            int32_elements: &|_| false, only_int32: &|_| false,
            captures: &[], fields: &[], native_field_loads: &[],
            widened: &std::collections::BTreeSet::new(),
        };
        let interrupt = crate::ExecutionInterrupt::default();
        let pending = start_osr_region(&region, &observation, &interrupt,
            crate::MemoryPolicy::Throughput, &program, id,
            super::super::compiled::process_lane_helpers::<crate::test_host::TestHost>())
            .expect("the loop plans");
        assert!(pending.uses_lane(), "the loop goes through RegionSpecializer");
    }

    /// The lane-ABI code the region specializer planned executes the loop correctly.
    #[test]
    fn a_loop_compiled_by_the_region_specializer_runs() {
        super::super::LANE_OSR_RUNS.with(|count| count.set(0));
        let program = crate::Engine::specialize(
            "function run(){ let s=0; for(let k=0;k<2000;k++){ for(let i=0;i<100;i++){s=s+i;} } return s; } throw run();",
            "lane-osr.js").unwrap();
        let mut vm = crate::vm::Vm::new(crate::test_host::TestHost::default());
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        vm.programs.enable_js_jit_diagnostics();
        vm.programs.enable_js_lane_methods();
        vm.programs.disable_js_entry_specializations();
        let error = vm.execute(&program).unwrap_err();
        assert_eq!(error.thrown_value(), Some(crate::Value::integer(9_900_000)));
        assert!(super::super::LANE_OSR_RUNS.with(|count| count.get()) > 0,
            "the lane OSR path ran");
    }

    /// A loop reading a static own field lowers through the lane unit's field sites.
    #[test]
    fn a_field_loop_compiled_by_the_region_specializer_runs() {
        super::super::LANE_OSR_RUNS.with(|count| count.set(0));
        let program = crate::Engine::specialize(
            "function helper(o){ let s=0; for(let k=0;k<2000;k++){ for(let i=0;i<100;i++){ s=(s+i+o.x)|0; } } return s; } throw helper({x:1});",
            "lane-osr-field.js").unwrap();
        let mut vm = crate::vm::Vm::new(crate::test_host::TestHost::default());
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        vm.programs.enable_js_jit_diagnostics();
        vm.programs.enable_js_lane_methods();
        vm.programs.disable_js_entry_specializations();
        let error = vm.execute(&program).unwrap_err();
        assert_eq!(error.thrown_value(), Some(crate::Value::integer(10_100_000)));
        assert!(super::super::LANE_OSR_RUNS.with(|count| count.get()) > 0,
            "the lane OSR path ran");
    }
    /// A loop indexing a dense record array lowers through the lane unit's array views.
    #[test]
    fn an_array_loop_compiled_by_the_region_specializer_runs() {
        super::super::LANE_OSR_RUNS.with(|count| count.set(0));
        let program = crate::Engine::specialize(
            "function helper(arr){ let s=0; for(let k=0;k<2000;k++){ for(let i=0;i<100;i++){ s=(s+i+arr[i&3])|0; } } return s; } throw helper([1,2,3,4]);",
            "lane-osr-array.js").unwrap();
        let mut vm = crate::vm::Vm::new(crate::test_host::TestHost::default());
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        vm.programs.enable_js_jit_diagnostics();
        vm.programs.enable_js_lane_methods();
        vm.programs.disable_js_entry_specializations();
        let error = vm.execute(&program).unwrap_err();
        assert_eq!(error.thrown_value(), Some(crate::Value::integer(10_400_000)));
        assert!(super::super::LANE_OSR_RUNS.with(|count| count.get()) > 0,
            "the lane OSR path ran");
    }
}
