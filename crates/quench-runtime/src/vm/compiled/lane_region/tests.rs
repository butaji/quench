#[test]
fn call_effect_retires_nullable_shapes_without_losing_null_alternatives() {
    use super::*;
    let null = Type::language(crate::ValueType::Null);
    let mut state = State::canonical(1);
    state.values[0] = RegisterFact::definition(Type::direct_object(3).join(null), Representation::Tagged);
    state.call_effect();
    assert_eq!(state.values[0].value.ty, Type::DIRECT_OBJECT_FAMILY.join(null));
    assert_eq!(state.values[0].value.ty.direct_object_part(), Some(Type::DIRECT_OBJECT_FAMILY));
}

use super::*;
use crate::value::EXACT_I64_TRUNCATION_LIMIT;

#[test]
fn callback_edges_retire_only_reachable_interrupt_ownership_proofs() {
    let isa = super::super::super::jit_backend::target_isa().unwrap();
    for callback_returns_to_loop in [false, true] {
        let mut function = ir::Function::new();
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut function, &mut context);
        let blocks = [(); 4].map(|_| builder.create_block());
        let mut polls = smallvec::SmallVec::<[ir::Inst; INLINE_EFFECT_EDGES]>::new();
        for (index, block) in blocks.into_iter().enumerate() {
            builder.switch_to_block(block);
            let value = builder.ins().iconst(types::I8, 1);
            let ir::ValueDef::Result(inst, _) = builder.func.dfg.value_def(value) else { unreachable!() };
            polls.push(inst);
            match index {
                0 => { builder.ins().jump(blocks[1], &[]); }
                1 => { builder.ins().brif(value, blocks[1], &[], blocks[2], &[]); }
                2 if callback_returns_to_loop => { builder.ins().brif(value, blocks[1], &[], blocks[3], &[]); }
                2 => { builder.ins().jump(blocks[3], &[]); }
                _ => { builder.ins().return_(&[]); }
            }
        }
        builder.seal_all_blocks();
        builder.finalize(isa.frontend_config());
        for (edge, kept) in [(blocks[0], 0), (blocks[2], if callback_returns_to_loop { 1 } else { 2 }), (blocks[3], 3)] {
            let mut selected = polls.clone();
            retain_closed_polls(&function, &[edge], &mut selected);
            assert_eq!(selected.as_slice(), &polls[..kept]);
        }
    }
}

#[test]
#[cfg(feature = "aot")]
fn inlined_capture_reads_require_an_admitted_invariant_callee() {
    let source = "let held=4,suspended=2;let prototype={};prototype.test=function(){return (this.state&held)!==0||this.state===suspended;};function other(){return this.state===held;}function run(task,count){const owner=task;let n=count|0,result=0;while(n>0){n=(n-1)|0;owner.state=(owner.state+1)|0;if(owner.test())result=(result+1)|0;}return result;}";
    let program = crate::Engine::specialize(source, "capture-entry.js").unwrap();
    let named = |name: &str| program.functions.iter().position(|body|
        body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == name)).unwrap();
    let caller = named("run");
    let other = named("other") as u32;
    let targets = static_call_targets(&program);
    let unit_for = |polymorphic| super::super::lane_unit::unit(&program, caller as u32,
        program.function(caller), &|function, pc| {
            let mut found: Vec<_> = targets[function as usize].get(pc as usize).into_iter().flatten().copied().next()
                .map(|function| super::super::lane_unit::CallTarget { function, callee: CalleeBinding::Function })
                .into_iter().collect();
            if polymorphic && !found.is_empty() {
                found.push(super::super::lane_unit::CallTarget { function: other, callee: CalleeBinding::Function });
            }
            found
        }, &|id| program.function_get(id as usize)).unwrap();
    for polymorphic in [false, true] {
        let unit = unit_for(polymorphic);
        assert_eq!(unit.frames.len(), if polymorphic { 2 } else { 1 });
        let header = unit.instructions.iter().enumerate().filter_map(|(pc, instruction)|
            instruction.op().control_flow_layout().jumps()
                .then(|| instruction.jump_target() as usize).filter(|target| *target <= pc)).min().unwrap();
        let loops = local_loops(&unit, &program.constants, header, &vec![None; unit.instructions.len()]);
        let reads = invariant_reads(&program, &unit, &loops);
        let captures: Vec<_> = reads.values().flatten().map(|read| read.pc).filter(|&pc|
            unit.instructions[pc].op().is_load_capture()
                && matches!(unit.origins[pc], super::super::lane_unit::Origin::Inlined { .. })).collect();
        assert_eq!(captures.len(), if polymorphic { 0 } else { 2 }, "{source}\n{}", program.disassemble());
        if polymorphic { continue; }
        let isa = super::super::super::jit_backend::target_isa().unwrap();
        let addresses = LaneHelper::addresses::<crate::test_host::TestHost>();
        let region = lower(&program, caller as u32, &unit, LaneHelpers::Process(addresses),
            &[], &Default::default(), VmFacts::STATIC, &[], &BTreeMap::new(), &[], &*isa, &NO_ENTRY_FIELDS, &[], &[], &[], &[], Coverage::PrefixFrom(header), Completion::Body, ElementViews::Dense).expect("capture loop region");
        let function = region.function;
        let cfg = cranelift_codegen::flowgraph::ControlFlowGraph::with_function(&function);
        let dom = cranelift_codegen::dominator_tree::DominatorTree::with_function(&function, &cfg);
        let mut loops = cranelift_codegen::loop_analysis::LoopAnalysis::new();
        loops.compute(&function, &cfg, &dom);
        let calls: Vec<_> = function.layout.blocks().flat_map(|block|
            function.layout.block_insts(block).map(move |inst| (block, inst))).filter(|&(_, inst)| {
                if function.dfg.insts[inst].opcode() != ir::Opcode::CallIndirect { return false; }
                let target = function.dfg.resolve_aliases(function.dfg.inst_args(inst)[0]);
                let ir::ValueDef::Result(target, _) = function.dfg.value_def(target) else { return false };
                matches!(function.dfg.insts[target], ir::InstructionData::UnaryImm { opcode: ir::Opcode::Iconst, imm }
                    if imm.bits() as usize == addresses[LaneHelper::FunctionCapture as usize] as usize)
            }).collect();
        assert_eq!(calls.len(), captures.len(), "one shared capture projection per entry read");
        for (block, _) in calls {
            assert!(loops.innermost_loop(block).is_none(), "capture admission must precede the loop");
        }
        let body: Vec<_> = function.layout.blocks().filter(|&block| loops.innermost_loop(block).is_some())
            .flat_map(|block| function.layout.block_insts(block)).collect();
        assert!(!body.iter().any(|&inst| matches!(function.dfg.insts[inst],
            ir::InstructionData::FloatCompare { cond: FloatCC::Equal, .. })),
            "the admitted equality capture must not decode or compare f64 in the loop: {function}");
        let entry_projection = |value| {
            let mut pending = vec![value];
            let mut seen = BTreeSet::new();
            let mut origin = None;
            while let Some(value) = pending.pop() {
                let value = function.dfg.resolve_aliases(value);
                if !seen.insert(value) { continue; }
                match function.dfg.value_def(value) {
                    ir::ValueDef::Param(block, index) => {
                        let before = pending.len();
                        for predecessor in cfg.pred_iter(block) {
                            for branch in function.dfg.insts[predecessor.inst]
                                .branch_destination(&function.dfg.jump_tables, &function.dfg.exception_tables) {
                                if branch.block(&function.dfg.value_lists) == block {
                                    let Some(value) = branch.args(&function.dfg.value_lists).nth(index)
                                        .and_then(|arg| arg.as_value()) else { return false };
                                    pending.push(value);
                                }
                            }
                        }
                        if pending.len() == before { return false; }
                    }
                    ir::ValueDef::Result(inst, _) if function.dfg.insts[inst].opcode() == ir::Opcode::Ireduce
                        && function.layout.inst_block(inst).is_some_and(|block| loops.innermost_loop(block).is_none()) => {
                            if origin.is_some_and(|other| other != value) { return false; }
                            origin = Some(value);
                        }
                    _ => return false,
                }
            }
            origin.is_some()
        };
        assert!(body.iter().any(|&inst| {
            let ir::InstructionData::IntCompare { cond: IntCC::Equal, args, .. } = function.dfg.insts[inst]
                else { return false };
            args.into_iter().any(|value| function.dfg.value_type(value) == types::I32 && entry_projection(value))
        }), "an entry-projected I32 capture must feed the native equality: {function}");
    }
}

#[test]
fn relation_capture_entry_preserves_noninteger_values_and_effects() {
    for (operator, strict) in [("==", false), ("===", true)] {
        for reversed in [false, true] {
            let comparison = if reversed { format!("limit{operator}this.bits") }
                else { format!("this.bits{operator}limit") };
            let source = format!(r#"
                (function(){{
                    function make(limit){{class Entry{{constructor(){{this.bits=0;}}probe(){{return {comparison};}}}}
                        return {{entry:new Entry(),set(value){{limit=value;}},read(){{return limit;}}}};}}
                    function run(entry,count){{const owner=entry;let n=count|0,result=0;
                        while(n>0){{n=(n-1)|0;owner.bits=(owner.bits+1)|0;if(owner.probe())result=(result+1)|0;}}return result;}}
                    function expected(value){{let total=0;for(let i=1;i<=7;i++)if(i{operator}value)total++;return total;}}
                    const bundle=make(2);
                    for(let warm=0;warm<96;warm++){{bundle.entry.bits=0;if(run(bundle.entry,7)!==1)throw 'warm';}}
                    for(const value of [0,-0,1,2,2.5,NaN,Infinity,-Infinity,2n,'2',true,null,undefined,Symbol()]){{
                        bundle.set(value);bundle.entry.bits=0;
                        if(run(bundle.entry,7)!==expected(value)||bundle.entry.bits!==7||!Object.is(bundle.read(),value))throw 'capture value';
                    }}
                    let effects=0;
                    bundle.set({{valueOf(){{effects++;return 2;}}}});bundle.entry.bits=0;
                    if(run(bundle.entry,7)!=={}||effects!=={}||bundle.entry.bits!==7)throw 'coercion';
                    effects=0;bundle.set({{valueOf(){{effects++;bundle.set(3);return 2;}}}});bundle.entry.bits=0;
                    if(run(bundle.entry,7)!=={}||effects!=={}||bundle.entry.bits!==7)throw 'coercion mutation';
                    bundle.set({{valueOf(){{throw 'zero-trip coercion';}}}});bundle.entry.bits=0;
                    if(run(bundle.entry,0)!==0||bundle.entry.bits!==0)throw 'zero-trip';
                    bundle.set(2);bundle.entry.bits=0;if(run(bundle.entry,7)!==1)throw 'integer reentry';
                    function cold(count){{class Entry{{constructor(){{this.bits=0;}}probe(){{return this.bits{operator}hidden;}}}}
                        const result=run(new Entry(),count);let hidden=2;return result;}}
                    if(cold(0)!==0)throw 'zero-trip tdz';let caught=false;
                    try{{cold(1);}}catch(error){{caught=error instanceof ReferenceError;}}if(!caught)throw 'tdz';
                    throw 42;
                }})();
            "#, usize::from(!strict), if strict { 0 } else { 7 }, usize::from(!strict), usize::from(!strict));
            for typescript in [false, true] {
                let program = if typescript { crate::Engine::specialize_typescript(&source, "relation-capture.cts") }
                    else { crate::Engine::specialize(&source, "relation-capture.cjs") }.unwrap();
                for mode in [crate::ExecutionMode::Interpreted, crate::ExecutionMode::Compiled] {
                    let mut vm = Vm::new(crate::test_host::TestHost::default());
                    vm.set_execution_mode(mode);
                    vm.programs.enable_js_lane_methods();
                    let error = vm.execute(&program).unwrap_err();
                    assert_eq!(error.thrown_value(), Some(Value::integer(42)),
                        "{operator}, reverse={reversed}, TS={typescript}, {mode:?}: {error}");
                }
            }
        }
    }
}

#[test]
#[cfg(feature = "aot")]
fn capture_entry_does_not_follow_a_copy_across_a_diamond() {
    let source = r#"
        let held=4;const prototype={};prototype.test=function(){return held|0;};
        function fieldChoice(a,b,flag,count){const first=a,second=b;let n=count|0,result=0;
            while(n>0){n=(n-1)|0;let selected=first;if(flag)selected=second;
                if(selected.test())result=(result+1)|0;}return result;}
        function calleeChoice(a,b,flag,count){const first=a,second=b;let n=count|0,result=0;
            while(n>0){n=(n-1)|0;let selected=first.test;if(flag)selected=second.test;
                if(selected())result=(result+1)|0;}return result;}
    "#;
    let program = crate::Engine::specialize(source, "capture-diamond.js").unwrap();
    let targets = static_call_targets(&program);
    for name in ["fieldChoice", "calleeChoice"] {
        let caller = program.functions.iter().position(|body|
            body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == name)).unwrap();
        let unit = super::super::lane_unit::unit(&program, caller as u32, program.function(caller),
            &|function, pc| targets[function as usize].get(pc as usize).into_iter().flatten().copied().next()
                .map(|function| super::super::lane_unit::CallTarget { function, callee: CalleeBinding::Function })
                .into_iter().collect(), &|id| program.function_get(id as usize)).unwrap();
        assert_eq!(unit.frames.len(), 1, "exercise an actual inline marker: {name}");
        let loops = local_loops(&unit, &program.constants, 0, &vec![None; unit.instructions.len()]);
        assert!(!loops.is_empty(), "exercise an eligible loop: {name}");
        let reads = invariant_reads(&program, &unit, &loops);
        assert!(!reads.values().flatten().any(|read|
            unit.instructions[read.pc].op().is_load_capture()
                && matches!(unit.origins[read.pc], super::super::lane_unit::Origin::Inlined { .. })),
            "a skipped copy cannot prove the closure environment: {name}\n{}", program.disassemble());
    }
}

#[test]
fn capture_entry_preserves_distinct_environments_and_mutations() {
    let source = r#"
        function make(held,suspended){
            class State {constructor(state){this.state=state;}
                test(){return (this.state&held)!==0||this.state===suspended;}}
            return {task:new State(5),set(a,b){held=a;suspended=b;}};
        }
        function run(task,count){let result=0;while(count>0){count=(count-1)|0;
            task.state=(task.state+1)|0;if(task.test())result=(result+1)|0;}return result;}
        function expected(state,count,held,suspended){let result=0;for(let i=0;i<count;i++){
            state=(state+1)|0;if((state&held)!==0||state===suspended)result++;}return result;}
        const a=make(4,2),b=make(8,3);
        for(let warm=0;warm<96;warm++){run(a.task,17);run(b.task,17);}
        for(let mask=0;mask<16;mask++){a.set(mask,7);a.task.state=0;b.task.state=0;
            if(run(a.task,257)!==expected(0,257,mask,7)||run(b.task,257)!==expected(0,257,8,3))throw 'environment';}
        let effects=0;a.set({valueOf(){effects++;return 4;}},2);a.task.state=0;
        if(run(a.task,17)!==expected(0,17,4,2)||effects!==17)throw 'coercion';
        function tdz(count){const task={state:0,test(){return (this.state&held)!==0;}};
            const result=run(task,count);let held=4;return result;}
        if(tdz(0)!==0)throw 'zero-trip';
        let caught=false;try{tdz(1);}catch(error){caught=error instanceof ReferenceError;}
        if(!caught)throw 'tdz';throw 42;
    "#;
    // CommonJS admits this source inside a function environment. Keep that
    // capture ownership/layout rather than testing only global bindings.
    let source = format!("(function(exports,require,module,__filename,__dirname){{{source}}})(undefined,undefined,undefined,undefined,undefined);");
    for typescript in [false, true] {
        let program = if typescript { crate::Engine::specialize_typescript(&source, "capture-entry.cts") }
            else { crate::Engine::specialize(&source, "capture-entry.cjs") }.unwrap();
        #[cfg(feature = "aot")]
        {
            // Exercise the linked unit/inliner path too: execution through
            // the dynamic VM alone need not select the same region entries.
            let module = crate::lower_native_js_module(&program, "capture_entry").unwrap();
            let isa = super::super::super::jit_backend::target_isa().unwrap();
            for function in module.functions {
                if let super::super::super::native_object::NativeFunction::Definition { function, .. } = function {
                    super::super::super::jit_backend::compile_function(function, &*isa, None, |_, _| Ok(())).unwrap();
                }
            }
        }
        for mode in [crate::ExecutionMode::Interpreted, crate::ExecutionMode::Compiled] {
            let mut vm = Vm::new(crate::test_host::TestHost::default());
            vm.set_execution_mode(mode);
            #[cfg(not(quench_profiled))]
            vm.programs.enable_js_jit_diagnostics();
            vm.programs.enable_js_lane_methods();
            let error = vm.execute(&program).unwrap_err();
            assert_eq!(error.thrown_value(), Some(Value::integer(42)), "{mode:?}: {error}");
        }
    }
}

#[test]
fn comparison_boolean_proofs_survive_branch_joins() {
    let program = crate::Engine::specialize(
        "function predicate(x){let result;if(x<0)result=x===-1;else result=x===1;if(result)return 7;return 9;}",
        "boolean-join.js").unwrap();
    let unit = super::super::lane_unit::unit(&program, 1, program.function(1), &|_, _| Vec::new(), &|_| None).unwrap();
    let isa = super::super::super::jit_backend::target_isa().unwrap();
    let region = lower(&program, 1, &unit, LaneHelpers::Process(&[std::ptr::null(); LaneHelper::COUNT]),
        &[], &Default::default(), VmFacts::STATIC, &[], &BTreeMap::new(), &[], &*isa, &NO_ENTRY_FIELDS, &[], &[], &[], &[], Coverage::Full, Completion::Body, ElementViews::Dense).unwrap();
    let code = compile_native_function(region.function, &*isa, usize::MAX, CodeMemory::new).unwrap();
    // SAFETY: the lowered function uses the shared lane-entry ABI and only live frame/register storage.
    let entry = unsafe { std::mem::transmute::<*mut u8, super::super::BodyEntry>(code.pointer()) };
    let mut frame = super::super::LaneFrame::new();
    let mut vm = Vm::new(crate::test_host::TestHost::default());
    frame.dispatch.refresh_object_bases(&vm);
    frame.dispatch.bind_native_storage(&mut vm);
    frame.dispatch.receiver = Value::UNDEFINED.to_bits();
    frame.context.adapter_payload = (&mut frame.dispatch as *mut JsStepDispatch).cast();
    for input in [-2.0, -1.0, 0.0, 1.0, 2.0] {
        let mut registers = vec![Value::UNDEFINED; usize::from(unit.caller_registers)];
        registers[0] = Value::number(input);
        let BodyReturn { status, value: word } = unsafe { entry(&mut frame.context, 0, 0, registers.as_mut_ptr().cast()) };
        assert_eq!(status, LANE_BODY_RETURNED, "comparison result must continue natively: {input}, stopped at {word}");
        assert_eq!(Value::from_bits(word), Value::number(if input == -1.0 || input == 1.0 { 7.0 } else { 9.0 }));
    }
    // This region selects an int32 entry view. Other Number representations
    // must recover before any guest operation, preserving their exact bits.
    for input in [-0.0, f64::NEG_INFINITY, f64::INFINITY, f64::NAN] {
        let mut registers = vec![Value::UNDEFINED; usize::from(unit.caller_registers)];
        registers[0] = Value::number(input);
        let before = registers.clone();
        let BodyReturn { status, value: word } = unsafe { entry(&mut frame.context, 0, 0, registers.as_mut_ptr().cast()) };
        // The entry's proof failed: the leave names it (`LaneExit::Entry`, which resumes
        // at the first record), not a PC the code would continue at.
        assert_eq!((status, word), (NATIVE_JS_STOPPED, super::super::LaneExit::Entry.word()));
        assert_eq!(registers, before);
    }
}

#[test]
fn native_to_int32_keeps_number_inputs_unboxed_and_misses_exactly() {
    let program = crate::Engine::specialize(
        "function words(x){x=x|0;return x|0;}", "unboxed-word-input.js").unwrap();
    let unit = super::super::lane_unit::unit(&program, 1, program.function(1), &|_, _| Vec::new(), &|_| None).unwrap();
    let isa = super::super::super::jit_backend::target_isa().unwrap();
    let region = lower(&program, 1, &unit, LaneHelpers::Process(&[std::ptr::null(); LaneHelper::COUNT]),
        &[], &Default::default(), VmFacts::STATIC, &[], &BTreeMap::new(), &[], &*isa, &NO_ENTRY_FIELDS, &[], &[], &[], &[], Coverage::Full, Completion::Number, ElementViews::Dense).unwrap();
    assert_eq!(region.number_result, Some(NativeNumberResult::Int32));
    for instruction in region.function.layout.blocks()
        .flat_map(|block| region.function.layout.block_insts(block)) {
        if region.function.dfg.insts[instruction].opcode() == ir::Opcode::Bitcast {
            let result = region.function.dfg.first_result(instruction);
            assert_ne!(region.function.dfg.value_type(result), types::I64, "int32 result must not round trip through f64");
        }
    }
    let code = compile_native_function(region.function, &*isa, usize::MAX, CodeMemory::new).unwrap();
    // SAFETY: the region has the lane body ABI and borrows one tagged Number.
    let entry = unsafe { std::mem::transmute::<*mut u8, super::super::BodyEntry>(code.pointer()) };
    let interrupt = crate::ExecutionInterrupt::default();
    let mut context = CallContext::interrupt_only(&interrupt);
    let boundary = [0.0, -0.0, 1.0, -1.0, f64::from(i32::MIN), f64::from(i32::MAX),
        f64::from(i32::MIN) - 1.0, f64::from(i32::MAX) + 1.0,
        1.5, -1.5, f64::MIN_POSITIVE, f64::from_bits(1), -f64::from_bits(1),
        f64::INFINITY, f64::NEG_INFINITY, f64::NAN, f64::MAX, -f64::MAX,
        EXACT_I64_TRUNCATION_LIMIT, -EXACT_I64_TRUNCATION_LIMIT,
        EXACT_I64_TRUNCATION_LIMIT.next_down(), -EXACT_I64_TRUNCATION_LIMIT.next_down()];
    let mut random = 0x517c_c1b7_2722_0a95_u64;
    let samples = boundary.into_iter().chain((0..256).map(|_| {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        f64::from_bits(random)
    }));
    for number in samples {
        let input = [Value::number(number).to_bits()];
        let BodyReturn { status, value: out } = unsafe { entry(&mut context, 0, 0, input.as_ptr()) };
        if number.abs() < EXACT_I64_TRUNCATION_LIMIT {
                assert_eq!(status, 0, "{number:?}");
                assert_eq!(out, u64::from(crate::value::number_to_u32(number)), "{number:?}");
        } else {
                assert_eq!(i64::from(status), NUMBER_ENTRY_MISS, "{number:?}");
                assert_eq!(out, 0, "miss may not publish a result");
        }
        assert_eq!(input[0], Value::number(number).to_bits(), "borrowed Number is never canonicalized in place");
    }
}

