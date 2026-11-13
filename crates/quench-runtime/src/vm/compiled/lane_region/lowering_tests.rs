use super::*;

#[test]
fn unavailable_fault_page_declines_before_changing_canonical_registers() {
    if !super::super::FAULT_POLL_SUPPORTED { return; }
    let program = crate::Engine::specialize(
        "function count(n){n=n|0;let sum=0;for(let i=0;i<n;i=(i+1)|0)sum=(sum+1)|0;return sum;}",
        "missing-poll.js").unwrap();
    let function = program.function(1);
    let unit = super::super::lane_unit::unit(&program, 1, function, &|_, _| Vec::new(), &|_| None).unwrap();
    let isa = super::super::super::jit_backend::target_isa().unwrap();
    let helpers = LaneHelpers::Process(LaneHelper::addresses::<crate::test_host::TestHost>());
    let region = lower(&program, 1, &unit, helpers, &[], &Default::default(), VmFacts::STATIC, &[], &BTreeMap::new(), &[], &*isa, &NO_ENTRY_FIELDS, &[], &[], &[], &[], Coverage::Full, Completion::Body, ElementViews::Dense).unwrap();
    assert!(region.fault_poll);
    let code = compile_native_function(region.function, &*isa, usize::MAX, CodeMemory::new).unwrap();
    let entry = unsafe { std::mem::transmute::<*mut u8, super::super::BodyEntry>(code.pointer()) };
    let mut frame = super::super::LaneFrame::new();
    let mut vm = Vm::new(crate::test_host::TestHost::default());
    frame.dispatch.refresh_object_bases(&vm);
    frame.dispatch.bind_native_storage(&mut vm);
    frame.context.adapter_payload = (&mut frame.dispatch as *mut JsStepDispatch).cast();
    assert_eq!(frame.context.poll_start, 0);
    let before = vec![Value::number(7.0); usize::from(unit.caller_registers)];
    let mut registers = before.clone();
    // No resumable page is lent, so this entry must leave before touching a
    // poll or executing any guest operation. The canonical inputs stay exact.
    let BodyReturn { status, value: word } = unsafe { entry(&mut frame.context, 0, 0, registers.as_mut_ptr().cast()) };
    assert_eq!(status, NATIVE_JS_STOPPED);
    assert_eq!(word, 0);
    assert_eq!(registers, before);
}

#[test]
fn admitted_integer_stores_do_not_repeat_heap_barrier_checks() {
    let program = crate::Engine::specialize(
        "function write(o, value) { o.value=value; return value; }", "store-proof.js").unwrap();
    let function = program.function(1);
    let mut fields = flat_table(function.instructions().filter(|instruction| instruction.op().is_set_field())
        .map(|instruction| (usize::from(instruction.cache_site_index()), vec![NativeFieldLoad {
            shape: 0, holder: None, slot: 0, storage_len: 1, storage: None,
            ty: Type::INT32, inherited: None, function_shape: false,
        }])));
    assert_eq!(fields.iter().flatten().count(), 1, "the test must exercise a profiled store");
    let isa = super::super::super::jit_backend::target_isa().unwrap();
    let unit = super::super::lane_unit::unit(&program, 1, function, &|_, _| Vec::new(), &|_| None).unwrap();
    for ty in [Type::INT32, Type::ANY] {
        fields.iter_mut().flatten().next().unwrap()[0].ty = ty;
        let region = lower(&program, 1, &unit, LaneHelpers::Process(&[std::ptr::null(); LaneHelper::COUNT]),
            &fields, &Default::default(), VmFacts::STATIC, &[], &BTreeMap::new(), &[], &*isa, &NO_ENTRY_FIELDS, &[], &[], &[], &[], Coverage::Full, Completion::Body, ElementViews::Dense).unwrap();
        let native = region.function;
        let barrier = native.layout.blocks().flat_map(|block| native.layout.block_insts(block))
            .any(|instruction| matches!(native.dfg.insts[instruction], ir::InstructionData::Load { offset, .. }
                if i32::from(offset) == std::mem::offset_of!(JsStepDispatch, object_mark_blocks) as i32));
        assert_eq!(barrier, ty == Type::ANY,
            "only the admitted integer excludes a heap edge:\n{native}");
    }
}

#[test]
fn object_shape_proofs_survive_verified_control_flow_edges() {
    let program = crate::Engine::specialize(
        "function subject(o, flag) { let value=o.left; if(flag) value+=1; return value+o.right; }",
        "shape-edges.js",
    ).unwrap();
    let function = program.function(1);
    let fields = flat_table(function.instructions().filter_map(|instruction| {
        if instruction.op().class().eq(Class::GetField)
            && let Some(FieldLookup::Atom { atom, cache_site, .. }) = instruction.field_lookup()
            && let Some(slot) = ["left", "right"].iter().position(|name| *name == &program.atoms[atom as usize]) {
            Some((usize::from(cache_site), vec![NativeFieldLoad { shape: 0, holder: None, slot: slot as u16, storage_len: 2, storage: None, ty: Type::ANY, inherited: None, function_shape: false }]))
        } else { None }
    }));
    let isa = super::super::super::jit_backend::target_isa().unwrap();
    let unit = super::super::lane_unit::unit(&program, 1, function, &|_, _| Vec::new(), &|_| None).unwrap();
    let region = lower(&program, 1, &unit, LaneHelpers::Process(&[std::ptr::null(); LaneHelper::COUNT]), &fields, &Default::default(),
        VmFacts::STATIC, &[], &BTreeMap::new(), &[], &*isa, &NO_ENTRY_FIELDS, &[], &[], &[], &[], Coverage::Prefix, Completion::Body, ElementViews::Dense).unwrap();
    let native = region.function;
    let shapes = native.layout.blocks().flat_map(|block| native.layout.block_insts(block))
        .filter(|&instruction| matches!(native.dfg.insts[instruction], ir::InstructionData::Load { offset, .. }
            if i32::from(offset) == crate::heap::NATIVE_OBJECT_LAYOUT.auxiliary_offset as i32
                && native.dfg.value_type(native.dfg.inst_results(instruction)[0]) == types::I32))
        .count();
    // The implicit receiver has its entry projection; o needs one shape
    // proof, shared by both paths to its second field read.
    assert_eq!(shapes, 2, "the CFG must carry the proven shape:\n{native}");
}

#[test]
fn own_field_reads_take_no_arena_bound() {
    let program = crate::Engine::specialize(
        include_str!("../../../../tests/fixtures/own-slot-proofs.js"), "own-slot-proofs.js",
    ).unwrap();
    let id = program.functions.iter().position(|body|
        body.header().name.is_some_and(|name| &program.atoms[name as usize] == "readPair")).unwrap();
    let fields = flat_table(program.function(id).instructions().filter_map(|instruction| {
        if instruction.op().class().eq(Class::GetField)
            && let Some(FieldLookup::Atom { atom, cache_site, .. }) = instruction.field_lookup()
            && let Some(slot) = ["left", "right"].iter().position(|name| *name == &program.atoms[atom as usize]) {
            Some((usize::from(cache_site), vec![NativeFieldLoad { shape: 0, holder: None, slot: slot as u16, storage_len: 2, storage: None,
                ty: Type::ANY, inherited: None, function_shape: false }]))
        } else { None }
    }));
    let isa = super::super::super::jit_backend::target_isa().unwrap();
    let unit = super::super::lane_unit::unit(&program, id as u32, program.function(id), &|_, _| Vec::new(), &|_| None).unwrap();
    let region = lower(&program, id as u32, &unit, LaneHelpers::Process(&[std::ptr::null(); LaneHelper::COUNT]), &fields, &Default::default(),
        VmFacts::STATIC, &[], &BTreeMap::new(), &[], &*isa, &NO_ENTRY_FIELDS, &[], &[], &[], &[], Coverage::Prefix, Completion::Body, ElementViews::Dense).unwrap();
    let native = region.function;
    let bounds = native.layout.blocks().flat_map(|block| native.layout.block_insts(block))
        .filter(|&instruction| {
            let ir::InstructionData::IntCompare { cond: IntCC::UnsignedLessThanOrEqual, args, .. }
                = native.dfg.insts[instruction] else { return false };
            let ir::ValueDef::Result(length, _) = native.dfg.value_def(native.dfg.resolve_aliases(args[1]))
                else { return false };
            matches!(native.dfg.insts[length], ir::InstructionData::Load { offset, .. }
                if i32::from(offset) == crate::value_vec::NATIVE_PROPERTY_BUFFER_LAYOUT.length as i32)
        }).count();
    // A live object's run lies inside the arena (`object_slots::arena_range`): neither read
    // compares it with the arena's length.
    assert_eq!(bounds, 0, "own field reads take no arena bound:\n{native}");
}

#[test]
fn number_loops_keep_f64_values_and_proofs_across_the_backedge() {
    let program = crate::Engine::specialize(
        include_str!("../../../../tests/fixtures/typed-lane-numbers.js"), "typed-lane-numbers.js",
    ).unwrap();
    let id = program.functions.iter().position(|body|
        body.header().name.is_some_and(|name| &program.atoms[name as usize] == "accumulate")).unwrap();
    let isa = super::super::super::jit_backend::target_isa().unwrap();
    let unit = super::super::lane_unit::unit(&program, id as u32, program.function(id), &|_, _| Vec::new(), &|_| None).unwrap();
    let region = lower(&program, id as u32, &unit, LaneHelpers::Process(&[std::ptr::null(); LaneHelper::COUNT]),
        &[], &Default::default(), VmFacts::STATIC, &[], &BTreeMap::new(), &[], &*isa, &NO_ENTRY_FIELDS, &[], &[], &[], &[], Coverage::Prefix, Completion::Body, ElementViews::Dense).expect("typed Number residual");
    assert!(region.native > 0);
    let native = region.function;
    let cfg = cranelift_codegen::flowgraph::ControlFlowGraph::with_function(&native);
    let mut dominators = cranelift_codegen::dominator_tree::DominatorTree::new();
    dominators.compute(&native, &cfg);
    let mut loops = cranelift_codegen::loop_analysis::LoopAnalysis::new();
    loops.compute(&native, &cfg, &dominators);
    assert_eq!(loops.loops().count(), 1, "{native}");
    let mut arithmetic = 0;
    for block in native.layout.blocks().filter(|block| loops.innermost_loop(*block).is_some()) {
        for instruction in native.layout.block_insts(block) {
            let opcode = native.dfg.insts[instruction].opcode();
            assert!(!matches!(opcode, ir::Opcode::Bitcast | ir::Opcode::Band),
                "no boxing/unboxing or repeated tag check in the Number loop: {instruction}: {native}");
            arithmetic += usize::from(matches!(opcode, ir::Opcode::Fadd | ir::Opcode::Fsub | ir::Opcode::Fmul));
        }
    }
    assert_eq!(arithmetic, 3, "{native}");
}

/// A plain unit over the function's own code, without inlining: the shape the loop
/// tier hands the region specializer.
fn plain_unit(function: &crate::bytecode::Function) -> super::super::lane_unit::Unit {
    let instructions: Vec<_> = function.instructions().collect();
    let count = instructions.len();
    super::super::lane_unit::Unit {
        instructions,
        origins: (0..count).map(|pc|
            super::super::lane_unit::Origin::Caller(pc as u32)).collect(),
        frames: Vec::new(), markers: Vec::new(),
        caller_registers: function.registers, registers: function.registers,
        end: count as u32,
    }
}

/// `Coverage::Loop` lowers only the loop, treats its backedge as a native branch, and
/// leaves at the exact PC of every edge out of it.
#[test]
fn loop_coverage_lowers_only_the_loop() {
    let program = crate::Engine::specialize(
        "function accumulate(x) { let s = 0; for (let i = 0; i < x; i++) { s = (s + i) | 0; } return s; } accumulate(4);",
        "loop-coverage.js").unwrap();
    let id = program.functions.iter().position(|body|
        body.header().name.is_some_and(|name| &program.atoms[name as usize] == "accumulate")).unwrap() as u32;
    let function = program.function(id as usize);
    let unit = plain_unit(function);
    let (header, backedge) = unit.instructions.iter().enumerate().find_map(|(pc, instruction)| {
        (instruction.op().immediate_role() == crate::bytecode::ImmediateRole::JumpTarget
            && (instruction.jump_target() as usize) <= pc)
            .then(|| (instruction.jump_target() as usize, pc))
    }).expect("the function has a loop");
    let isa = super::super::super::jit_backend::target_isa().unwrap();
    let region = RegionSpecializer { function_id: id, unit: &unit,
        helpers: LaneHelpers::Process(&[std::ptr::null(); LaneHelper::COUNT]),
        fields: &[], global_reads: &Default::default(), facts: VmFacts::STATIC,
        generic: &[], direct: &BTreeMap::new(), frame_captures: &[], isa: &*isa,
        entry_fields: &NO_ENTRY_FIELDS, number_reads: &[], int_entries: &[], typed_captures: &[], int_captures: &[],
        plan: RegionPlan { coverage: Coverage::Loop { header, backedge, poll: true },
            completion: Completion::Body, views: ElementViews::Dense } }
        .specialize(&program).expect("the loop lowers");
    assert!(region.native > 0, "the loop runs natively");
    let native = region.function;
    let cfg = cranelift_codegen::flowgraph::ControlFlowGraph::with_function(&native);
    let mut dominators = cranelift_codegen::dominator_tree::DominatorTree::new();
    dominators.compute(&native, &cfg);
    let mut loops = cranelift_codegen::loop_analysis::LoopAnalysis::new();
    loops.compute(&native, &cfg, &dominators);
    assert_eq!(loops.loops().count(), 1, "one native loop: {native}");
}
