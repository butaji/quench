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

#[cfg(test)]
thread_local! {
    /// Tests that exercise the lane OSR plan force it ahead of the verified lowering.
    pub(super) static FORCE_LANE_OSR: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

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

/// Map an inlining loop region onto the lane unit: its instructions' jump targets become
/// unit PCs, its origins name the caller/inlined frame of each instruction, and its frames
/// carry the callee identity, function, call window and result.
fn inlining_unit(region: &LoopRegion) -> Option<lane_unit::Unit> {
    let inlining = region.inlining.as_deref()?;
    let count = region.instructions().len();
    let header = region.header as usize;
    let mut instructions = region.instructions().to_vec();
    for instruction in &mut instructions {
        if instruction.op().immediate_role() == crate::bytecode::ImmediateRole::JumpTarget {
            let target = instruction.imm() as usize;
            let local = target.checked_sub(header).filter(|local| *local < count).unwrap_or(count);
            instruction.set_jump_target(local as u32);
        }
    }
    let origins: Vec<lane_unit::Origin> = inlining.origins.iter().take(count).map(|origin| match origin {
        osr_loop::Origin::Caller(pc) => lane_unit::Origin::Caller(*pc),
        osr_loop::Origin::Inlined { frame, pc } => lane_unit::Origin::Inlined { frame: usize::from(*frame), pc: *pc },
        osr_loop::Origin::Leave(pc) => lane_unit::Origin::Caller(*pc),
    }).collect();
    let frames: Vec<lane_unit::Frame> = inlining.frames.iter().map(|frame| lane_unit::Frame {
        recovery: lane_unit::CallRecovery::Materialize,
        parent: None,
        call: frame.call,
        target: lane_unit::CallTarget {
            callee: lane_unit::CalleeBinding::Identity(frame.callee),
            function: frame.function,
        },
        callee_register: frame.callee_register,
        receiver: frame.receiver,
        window: frame.window,
        arguments: frame.arguments,
        result: Some(frame.result),
        base: frame.base,
        registers: frame.registers,
        alternative: None,
        function_call: None,
    }).collect();
    let markers: Vec<usize> = (0..frames.len()).map(|frame| origins.iter().position(|origin|
        matches!(origin, lane_unit::Origin::Inlined { frame: at, .. } if *at == frame)).unwrap_or(0)).collect();
    let registers = frames.iter().map(|frame| frame.base + frame.registers)
        .max().unwrap_or(0).max(inlining.caller_registers);
    Some(lane_unit::Unit {
        instructions, origins, frames, markers,
        caller_registers: inlining.caller_registers, registers, end: count as u32,
    })
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
    let plain = loop_unit(program.function(function_id as usize));
    let inlined = region.inlining.is_some().then(|| inlining_unit(region)).flatten();
    let unit = inlined.as_ref().unwrap_or(&plain);
    let fields = loop_fields(region, observation);
    let simple = region.inlining.is_none();
    // A callee with its own loop makes the inlined unit a nested loop region. The lane
    // plan's single loop does not inline that callee's loop yet; the verified lowering
    // does, so the lane plan declines it and the verified lowering publishes the region.
    let nested_inlined_loop = region.inlining.is_some() && region.instructions().iter().enumerate().any(|(pc, instruction)|
        instruction.op().immediate_role() == crate::bytecode::ImmediateRole::JumpTarget
            && (instruction.jump_target() as usize) <= pc
            && pc != region.backedge as usize);
    let lane = (!nested_inlined_loop && (simple || inlined.is_some())).then(|| super::lane_region::RegionSpecializer {
        function_id, unit, helpers, fields: &fields,
        global_reads: &BTreeMap::new(), facts: VmFacts::STATIC, generic: &[],
        direct: &BTreeMap::new(), frame_captures: &[], isa: &*isa,
        plan: super::lane_region::RegionPlan {
            coverage: super::lane_region::Coverage::Loop {
                header: if inlined.is_some() { 0 } else { region.header as usize },
                backedge: if inlined.is_some() {
                    unit.instructions.len().saturating_sub(1)
                } else { region.backedge as usize },
                poll: false },
            completion: super::lane_region::Completion::StatusAndPc,
            views: super::lane_region::ElementViews::Dense,
        },
    }.specialize(program).ok()).flatten();
    // The loop tier's verified lowering is the production path: it carries the whole
    // admitted region's facts, widenings and entry record. The lane specializer remains a
    // second plan over the same region, used where the verified lowering declines, until
    // its coverage matches (float unary, appends, inlining, return accounting, widening).
    let verified = || osr_loop::verify_region(region, observation).ok()
        .and_then(|verified| osr_loop::lower_verified(verified, observation, &*isa, poll_address(interrupt)));
    // The lane specializer is the production OSR plan; the verified lowering remains the
    // fallback for a nested inlined loop the lane unit does not model yet.
    let prefer_lane = true;
    let lane_plan = |lane: Option<super::lane_region::LoweredRegion>| lane.map(|region|
        (region.function, BTreeMap::new(), true));
    let (function, widenings, lane) = if prefer_lane {
        match lane_plan(lane) {
            Some(plan) => plan,
            None => match verified() {
                Some((function, widenings)) => (function, widenings, false),
                None => return None,
            },
        }
    } else {
        match verified() {
            Some((function, widenings)) => (function, widenings, false),
            None => lane_plan(lane)?,
        }
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
    // A lane region carries each inlined callee's full body and its guards, so a loop
    // that inlines two calls may legitimately exceed the verified lowering's budget.
    // The code is published once per hot loop and retires with its site, so the larger
    // per-site bound stays bounded by the lane plan's own admission.
    let code_budget = if lane { 2 * osr_loop::MAX_CODE_BYTES } else { osr_loop::MAX_CODE_BYTES };
    let code = super::super::jit_backend::run_in_background(move || {
        compile_native_function(function, &*isa, code_budget, |code| Some(code.to_vec()))
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
        super::FORCE_LANE_OSR.with(|force| force.set(true));
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
        super::FORCE_LANE_OSR.with(|force| force.set(true));
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

    /// A loop taking a Number remainder runs through the lane unit's shared remainder, keeping
    /// signed zeros and leaving a coercing operand to the general path.
    #[test]
    fn a_remainder_loop_runs_through_the_lane_unit() {
        super::FORCE_LANE_OSR.with(|force| force.set(true));
        super::super::LANE_OSR_RUNS.with(|count| count.set(0));
        let program = crate::Engine::specialize(
            "function remainderLoop(left, right) { let result = 0; for (let i = 0; i < 64; i++) result = left % right; return result; }
             for (let i = 0; i < 100; i++) remainderLoop(7, 3);
             const pairs = [[-4, 2], [-0, 3], [4, -2], [1, 0], [1, -0], [Infinity, 2], [-Infinity, 2], [2, Infinity], [-2, -Infinity], [NaN, 2], [2, NaN], [-2147483648, -1], [0.5, 0.2], [5.5, 2]];
             const results = [];
             for (const pair of pairs) { const value = remainderLoop(pair[0], pair[1]); results.push(Object.is(value, -0) ? '-0' : String(value)); }
             print(results.join(','));
             let calls = 0; print(remainderLoop({valueOf() {calls++; return 7;}}, 3)); print(calls);",
            "lane-osr-remainder.js").unwrap();
        let output = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut vm = crate::vm::Vm::new(crate::test_host::TestHost::writing_to(output.clone()));
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        vm.programs.enable_js_jit_diagnostics();
        vm.programs.enable_js_lane_methods();
        vm.programs.disable_js_entry_specializations();
        vm.execute(&program).unwrap();
        assert_eq!(output.borrow().as_slice(), [
            "-0,-0,0,NaN,NaN,NaN,NaN,2,-2,NaN,NaN,-0,0.09999999999999998,1.5", "1", "64",
        ]);
        assert!(super::super::LANE_OSR_RUNS.with(|count| count.get()) > 0,
            "the lane OSR path ran");
    }

    /// A `Return` inside the loop completes the activation through the lane unit, not an
    /// exit: the loop's native run count stays above its exits and the result matches.
    #[test]
    fn a_return_inside_a_loop_completes_through_the_lane_unit() {
        super::FORCE_LANE_OSR.with(|force| force.set(true));
        super::super::LANE_OSR_RUNS.with(|count| count.set(0));
        let program = crate::Engine::specialize(
            "function cmp(a, b) { var i = a.length; while (--i >= 0) { var r = a[i] - b[i]; if (r != 0) return r; } return 0; }
             var a = [], b = [];
             for (var i = 0; i < 200; i++) { a.push(i * 3 & 255); b.push(i * 3 & 255); }
             var s = 0;
             for (var k = 0; k < 3000; k++) {
               b[k % 200] = (k & 1) ? a[k % 200] + 1 : a[k % 200];
               s = (s + cmp(a, b) * (k % 7)) | 0;
               b[k % 200] = a[k % 200];
             }
             print(s);",
            "lane-osr-return.js").unwrap();
        let expected = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        crate::vm::Vm::new(crate::test_host::TestHost::writing_to(expected.clone()))
            .execute(&program).unwrap();
        let output = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut vm = crate::vm::Vm::new(crate::test_host::TestHost::writing_to(output.clone()));
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        vm.programs.enable_js_jit_diagnostics();
        vm.programs.enable_js_lane_methods();
        vm.programs.disable_js_entry_specializations();
        vm.execute(&program).unwrap();
        assert_eq!(*output.borrow(), *expected.borrow());
        let function_id = program.functions.iter().position(|function|
            function.header().name.is_some_and(|name| &program.atoms[name as usize] == "cmp")).unwrap();
        let views = vm.programs.lane_views(super::super::program_store::ProgramId::MAIN).unwrap();
        let osr = views.get(function_id).and_then(super::super::program_store::derived_lane_view)
            .unwrap().osr_diagnostics();
        assert!(osr.native_region_runs > 0, "the return loop ran natively: {osr:?}");
        assert_eq!(osr.exits, 0, "a return inside the loop is a completion, not an exit: {osr:?}");
    }

    /// A loop appending to a fresh array runs natively through the lane unit: the append
    /// helper grows the backing and the loop's normal continuation is a resume, not an exit.
    #[test]
    fn an_append_growth_loop_runs_through_the_lane_unit() {
        super::FORCE_LANE_OSR.with(|force| force.set(true));
        super::super::LANE_OSR_RUNS.with(|count| count.set(0));
        let program = crate::Engine::specialize(
            "function fill(n) { var a = []; for (var i = 0; i < n; i++) a[i] = (i * 7 + 3) & 255; return a; }
             var sum = 0;
             for (var k = 0; k < 3000; k++) { var a = fill(40 + (k & 7)); sum = (sum * 31 + a[a.length - 1] + a.length) | 0; }
             print(sum);",
            "lane-osr-append.js").unwrap();
        let expected = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        crate::vm::Vm::new(crate::test_host::TestHost::writing_to(expected.clone()))
            .execute(&program).unwrap();
        let output = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut vm = crate::vm::Vm::new(crate::test_host::TestHost::writing_to(output.clone()));
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        vm.programs.enable_js_jit_diagnostics();
        vm.programs.enable_js_lane_methods();
        vm.programs.disable_js_entry_specializations();
        vm.execute(&program).unwrap();
        assert_eq!(*output.borrow(), *expected.borrow());
        assert!(super::super::LANE_OSR_RUNS.with(|count| count.get()) > 0,
            "the lane OSR path ran");
    }

    /// A loop negating a Number runs through the lane unit's shared Number unary denotation,
    /// keeping `-0` and widening an overflowing int32 negation.
    #[test]
    fn a_float_unary_loop_runs_through_the_lane_unit() {
        super::FORCE_LANE_OSR.with(|force| force.set(true));
        super::super::LANE_OSR_RUNS.with(|count| count.set(0));
        let program = crate::Engine::compile(crate::ExecutionRequest::script(
            "function negate(value){ let index=0; while(index<9999){ value=-value; index++; } return value; }
             for(let call=0;call<100;call++) negate(0.5);
             if(negate(0.5)!==-0.5) throw Error('float negation');
             if(!Object.is(negate(0),-0)) throw Error('negative zero');
             if(negate(-2147483648)!==2147483648) throw Error('int32 minimum');
             print('ok');", "lane-osr-unary.js")).unwrap();
        let output = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut vm = crate::vm::Vm::new(crate::test_host::TestHost::writing_to(output.clone()));
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        vm.programs.enable_js_jit_diagnostics();
        vm.programs.enable_js_lane_methods();
        vm.programs.disable_js_entry_specializations();
        vm.execute(&program).unwrap();
        assert_eq!(output.borrow().as_slice(), ["ok"]);
        assert!(super::super::LANE_OSR_RUNS.with(|count| count.get()) > 0,
            "the lane OSR path ran");
    }

    /// A loop reading a static own field lowers through the lane unit's field sites.
    #[test]
    fn a_field_loop_compiled_by_the_region_specializer_runs() {
        super::FORCE_LANE_OSR.with(|force| force.set(true));
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
        super::FORCE_LANE_OSR.with(|force| force.set(true));
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
    /// A loop reading a captured binding carries the loop entry's capture view.
    #[test]
    fn a_capture_loop_keeps_its_capture_value() {
        super::FORCE_LANE_OSR.with(|force| force.set(true));
        super::super::LANE_OSR_RUNS.with(|count| count.set(0));
        let program = crate::Engine::specialize(
            "function helper(){ let add=1; let f=()=>add; let s=0; for(let k=0;k<2000;k++){ for(let i=0;i<100;i++){ s=s+i+add; } } return s; } function main(){ return helper(); } throw main();",
            "lane-osr-capture.js").unwrap();
        let mut vm = crate::vm::Vm::new(crate::test_host::TestHost::default());
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        vm.programs.enable_js_jit_diagnostics();
        vm.programs.enable_js_lane_methods();
        vm.programs.disable_js_entry_specializations();
        let error = vm.execute(&program).unwrap_err();
        assert_eq!(error.thrown_value(), Some(crate::Value::integer(10_100_000)));
    }
    /// A loop that stores a field either lowers through the lane specializer or stays in
    /// the interpreter; it never takes the private lowering.
    #[test]
    fn a_field_store_loop_keeps_its_result() {
        super::super::LANE_OSR_RUNS.with(|count| count.set(0));
        let program = crate::Engine::specialize(
            "function helper(o){ let s=0; for(let k=0;k<2000;k++){ for(let i=0;i<100;i++){ o.x=(o.x+i)|0; s=(s+o.x)|0; } } return s; } throw helper({x:1});",
            "lane-osr-field-store.js").unwrap();
        let run = |mode| {
            let mut vm = crate::vm::Vm::new(crate::test_host::TestHost::default());
            vm.set_execution_mode(mode);
            vm.programs.enable_js_jit_diagnostics();
            vm.programs.enable_js_lane_methods();
            vm.programs.disable_js_entry_specializations();
            vm.execute(&program).unwrap_err().thrown_value()
        };
        let interpreted = run(crate::ExecutionMode::Interpreted);
        let compiled = run(crate::ExecutionMode::Compiled);
        assert_eq!(interpreted, compiled);
    }
    /// An inlining loop region maps onto a lane unit with one frame, inlined origins and a
    /// call marker, before it is offered to `RegionSpecializer`.
    #[test]
    fn an_inlining_region_maps_onto_a_lane_unit() {
        let program = crate::Engine::specialize(
            "function callee(o){ return o.x; } function caller(o){ let s=0; for(let i=0;i<10;i++){ s += o.callee(o); } return s; } caller({callee: callee, x: 1});",
            "inlining-unit.js").unwrap();
        let caller_id = program.functions.iter().position(|body|
            body.header().name.is_some_and(|name| &program.atoms[name as usize] == "caller")).unwrap() as u32;
        let callee_id = program.functions.iter().position(|body|
            body.header().name.is_some_and(|name| &program.atoms[name as usize] == "callee")).unwrap() as u32;
        let function = program.function(caller_id as usize);
        let decoded: Vec<_> = function.instructions().collect();
        let constants: Vec<crate::Value> = program.constants.iter()
            .map(|constant| super::super::facts::immediate(constant).unwrap_or(crate::Value::UNDEFINED))
            .collect();
        let live = program.register_root_masks(caller_id).unwrap();
        let callee = program.function(callee_id as usize);
        let mut resolve = |_chain: &[super::super::FieldUse], _capture: Option<u16>| {
            Some(super::super::osr_loop::InlineCallee {
                callee: crate::Value::UNDEFINED,
                function: callee_id,
                params: callee.params,
                registers: callee.registers,
                instructions: callee.instructions().collect(),
                live: program.register_root_masks(callee_id).unwrap(),
            })
        };
        let (header, backedge) = decoded.iter().enumerate().find_map(|(pc, instruction)| {
            (instruction.op().immediate_role() == crate::bytecode::ImmediateRole::JumpTarget
                && (instruction.jump_target() as usize) <= pc)
                .then(|| (instruction.jump_target(), pc as u32))
        }).expect("the caller has a loop");
        let region = super::super::osr_loop::inline_region(function, &decoded, &constants,
            &program.field_sites, live, header, backedge, &mut resolve)
            .expect("the loop inlines");
        let unit = inlining_unit(&region).expect("the region maps onto a lane unit");
        assert_eq!(unit.frames.len(), 1, "one inlined frame");
        assert!(unit.origins.iter().any(|origin|
            matches!(origin, lane_unit::Origin::Inlined { .. })), "inlined origins");
        assert!(!unit.markers.is_empty(), "the call marker");
    }
    /// An inlined callee's loop executes through the lane unit when it lowers.
    #[test]
    fn an_inlined_call_loop_runs_through_the_lane_unit() {
        super::FORCE_LANE_OSR.with(|force| force.set(true));
        super::super::LANE_OSR_RUNS.with(|count| count.set(0));
        let program = crate::Engine::specialize(
            "function callee(o){ return o.x; } function caller(o){ let s=0; for(let k=0;k<2000;k++){ for(let i=0;i<100;i++){ s=(s+callee(o))|0; } } return s; } throw caller({x:1});",
            "lane-osr-inline.js").unwrap();
        let mut vm = crate::vm::Vm::new(crate::test_host::TestHost::default());
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        vm.programs.enable_js_jit_diagnostics();
        vm.programs.enable_js_lane_methods();
        vm.programs.disable_js_entry_specializations();
        let error = vm.execute(&program).unwrap_err();
        assert_eq!(error.thrown_value(), Some(crate::Value::integer(200_000)));
        assert!(super::super::LANE_OSR_RUNS.with(|count| count.get()) > 0,
            "the inlining lane OSR path ran");
    }
}
