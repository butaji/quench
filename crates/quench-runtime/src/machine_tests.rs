#[test]
fn call_frame_suspend_and_resume_restores_caller_state() {
    let function = super::FunctionCode::from_ops(vec![
        super::Op::Move { dst: 0, src: 0 },
        super::Op::Move { dst: 1, src: 1 },
    ]);
    let mut machine = super::Machine::with_function(
        &function,
        super::EnvironmentRef(7),
        2,
    );
    machine.set_program_counter(1);
    machine.registers_mut().write(0, super::Value::Number(11.0));
    machine.suspend_call(
        super::Value::Undefined,
        super::Value::Undefined,
        vec![super::Value::Number(3.0)],
        1,
        crate::completion::ContinuationGuards::new(9),
    )
    .expect("call continuation allocation");
    assert_eq!(machine.call_frames.len(), 1);
    assert!(machine.registers_mut().is_empty());

    let continuation = machine
        .resume_call(super::Value::Number(42.0))
        .expect("suspended caller");
    assert_eq!(continuation.caller_code, function.code_id());
    assert_eq!(continuation.caller_pc, 1);
    assert_eq!(continuation.destination, 1);
    assert_eq!(continuation.guards.flags, 9);
    assert_eq!(machine.program_counter(), 1);
    assert_eq!(
        machine.registers_mut().read(0),
        Some(super::Value::Number(11.0))
    );
    assert_eq!(
        machine.registers_mut().read(1),
        Some(super::Value::Number(42.0))
    );
    assert!(machine.call_frames.is_empty());
}

#[test]
fn code_store_drops_after_nested_function_owner_is_released() {
    let nested = super::FunctionCode::pending(vec![
        super::Op::Const {
            dst: 0,
            value: crate::ops::Constant::Number(1.0),
        },
        super::Op::Return { src: 0 },
    ]);
    let owner = super::FunctionCode::from_ops(vec![super::Op::MakeFunctionWithKind {
        dst: 0,
        body: nested,
        params: 0,
        length: 0,
        captures: 0,
        kind: crate::ops::FunctionKind::Ordinary,
        strictness: crate::ops::FunctionStrictness::Sloppy,
        is_async: false,
        mapped_arguments: false,
        source: None,
    }]);
    let code = owner.code().expect("owner code");
    let crate::ops::Op::MakeFunctionWithKind { body, .. } = code.cold_at(0).expect("nested op")
    else {
        panic!("expected nested function op");
    };
    assert!(
        body.has_internal_store_link(),
        "nested body retained its owner"
    );
    let weak = std::rc::Rc::downgrade(&owner.store().expect("nested code store"));
    drop(owner);
    assert!(
        weak.upgrade().is_none(),
        "nested code store retained a cycle"
    );
}

#[test]
fn escaped_nested_function_clone_retains_code_store() {
    let nested = super::FunctionCode::pending(vec![super::Op::Return { src: 0 }]);
    let owner = super::FunctionCode::from_ops(vec![super::Op::MakeFunctionWithKind {
        dst: 0,
        body: nested,
        params: 0,
        length: 0,
        captures: 0,
        kind: crate::ops::FunctionKind::Ordinary,
        strictness: crate::ops::FunctionStrictness::Sloppy,
        is_async: false,
        mapped_arguments: false,
        source: None,
    }]);
    let code = owner.code().expect("owner code");
    let crate::ops::Op::MakeFunctionWithKind { body, .. } = code.cold_at(0).expect("nested op")
    else {
        panic!("expected nested function op");
    };
    let escaped = body.clone();
    let weak = std::rc::Rc::downgrade(&owner.store().expect("nested code store"));
    drop(owner);
    assert!(
        escaped.store().is_some(),
        "escaped body lost its code owner"
    );
    assert!(
        weak.upgrade().is_some(),
        "escaped body did not retain its store"
    );
    drop(escaped);
    assert!(
        weak.upgrade().is_none(),
        "escaped store was not reclaimable"
    );
}

#[test]
fn machine_rejects_call_continuation_from_unknown_code_source() {
    let function = super::FunctionCode::from_ops(vec![super::Op::ParameterEnd]);
    let mut machine = Machine::with_function(&function, EnvironmentRef(0), 1);
    machine
        .try_push_call_frame(crate::completion::CallContinuation {
        callee: super::Value::Undefined,
        receiver: super::Value::Undefined,
        arguments: Vec::new().into(),
        caller_code: super::CodeId(99),
        caller_pc: 0,
        caller_registers: crate::register_file::RegisterFile::new(),
        caller_environment: EnvironmentRef(0),
        destination: 0,
        guards: crate::completion::ContinuationGuards::default(),
        })
        .expect("valid continuation source");
    assert!(machine.resume_call(super::Value::Number(1.0)).is_none());
    assert_eq!(machine.program_counter(), 0);
}

#[test]
fn machine_resolves_frame_ranges_from_its_function_store() {
    let function = super::FunctionCode::from_ops(vec![super::Op::ParameterEnd]);
    let machine = Machine::with_function(&function, EnvironmentRef(0), 1);
    assert_eq!(
        machine
            .store
            .as_ref()
            .and_then(|store| store.code(function.range))
            .map(|code| code.len()),
        Some(0)
    );
}

#[test]
fn code_view_register_width_comes_from_lowered_operands() {
    let function = super::FunctionCode::from_ops(vec![
        // A long instruction stream must not imply a 32-slot frame when the
        // lowered data flow only names three registers.
        super::Op::Move { dst: 2, src: 1 },
        super::Op::Return { src: 2 },
    ]);
    assert_eq!(function.code().expect("compact code").register_count(), 3);
}

fn lowered_named_call(args: Vec<u16>) -> super::FunctionCode {
    super::FunctionCode::from_ops(vec![
        super::Op::GetProperty {
            dst: 4,
            object: 2,
            key: "method".into(),
        },
        super::Op::CallMethod {
            dst: 3,
            object: 2,
            key: "method".into(),
            callee: Some(4),
            spreads: vec![false; args.len()],
            args,
        },
        super::Op::Return { src: 3 },
    ])
}

#[test]
fn lowered_named_call_width_uses_real_argument_operands() {
    let zero = lowered_named_call(Vec::new());
    let one = lowered_named_call(vec![9]);
    let many = lowered_named_call(vec![5, 7, 11]);
    assert_eq!(
        zero.code().unwrap().instruction(0).unwrap().opcode,
        crate::ir::Opcode::CallN
    );
    assert_eq!(zero.required_register_count(), 4);
    assert_eq!(one.required_register_count(), 10);
    assert_eq!(many.required_register_count(), 12);
}

fn source_call_frame(source: &str, opcode: crate::ir::Opcode, argc: u8) -> (u16, Option<Vec<u16>>) {
    let program = crate::reduce::reduce_source(source).expect("call source lowers");
    let mut found = None;
    crate::stencil_test_support::visit_code_views(program.code(), &mut |code| {
        for pc in 0..code.len() {
            let Some(instruction) = code.instruction(pc) else {
                continue;
            };
            if instruction.opcode == opcode && instruction.flags == argc {
                found = Some((
                    code.frame_register_count(),
                    code.operand_window_at(pc).map(<[u16]>::to_vec),
                ));
            }
        }
    });
    found.expect("named call instruction")
}

#[test]
fn ordinary_source_call_frames_use_declared_argument_windows() {
    let zero_source = "function f(o){return o.m()} if(f({m(){return 1}})!==1)throw 0";
    let many_source = concat!(
        "function f(o){return o.m(1,2,3)} ",
        "if(f({m(a,b,c){return a+b+c}})!==6)throw 0"
    );
    let direct_source = concat!(
        "function g(a,b,c){return a+b+c} function f(){return g(1,2,3)} ",
        "if(f()!==6)throw 0"
    );
    let zero = source_call_frame(zero_source, crate::ir::Opcode::CallN, 0);
    let many = source_call_frame(many_source, crate::ir::Opcode::CallN, 3);
    let direct = source_call_frame(direct_source, crate::ir::Opcode::Call, 3);
    assert!(zero.0 < 32, "zero-argument sentinel widened frame");
    assert_eq!(zero.1, None);
    assert_eq!(many.1.as_deref(), Some([2, 3, 4].as_slice()));
    assert!(many.0 < 32, "fixed argument window widened frame");
    assert_eq!(direct.1.as_ref().map(Vec::len), Some(3));
    assert!(direct.0 < 32, "direct argument window widened frame");
    for source in [zero_source, many_source, direct_source] {
        let program = crate::reduce::reduce_source(source).expect("call source reduces");
        crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
            .expect("call source executes");
    }
}

#[test]
fn ordinary_counted_numeric_loop_lowers_to_one_cfg_backedge() {
    let source = concat!(
        "function f(s){var x=17;for(var i=0;i<s.n;i++)x=(x*33+7)|0;return x}",
        "if(f({n:4})!==20420077)throw new Error('bad loop')"
    );
    let program = crate::reduce::reduce_source(source).expect("counted loop lowers");
    let mut matched = false;
    crate::stencil_test_support::visit_code_views(program.code(), &mut |code| {
        let instructions = (0..code.len())
            .filter_map(|pc| code.instruction(pc))
            .collect::<Vec<_>>();
        let has_body = instructions
            .iter()
            .any(|instruction| instruction.opcode == crate::ir::Opcode::Mul)
            && instructions
                .iter()
                .any(|instruction| instruction.opcode == crate::ir::Opcode::StoreLocal);
        let has_backedge = instructions.iter().enumerate().any(|(pc, instruction)| {
            instruction.opcode == crate::ir::Opcode::Jump && usize::from(instruction.a) <= pc
        });
        if has_body && has_backedge {
            matched = true;
            assert!(
                (0..code.len()).all(|pc| !matches!(code.cold_at(pc), Some(super::Op::Loop { .. }))),
                "eligible loop retained the structured gateway"
            );
        }
    });
    assert!(matched, "ordinary loop did not expose its canonical CFG");
}

#[test]
fn current_local_integer_loop_selector_matches_cfg_shape() {
    let source = concat!(
        "function f(s){var x=17;for(var i=0;i<s.n;i++)x=(x*33+7)|0;return x}",
        "if(f({n:4})!==20420077)throw new Error('bad loop')"
    );
    let program = crate::reduce::reduce_source(source).expect("counted loop lowers");
    let mut found = false;
    crate::stencil_test_support::visit_code_views(program.code(), &mut |code| {
        let entries = super::baseline_entries(code);
        let operand_windows = (0..entries.len())
            .map(|pc| code.operand_window_at(pc))
            .collect::<Vec<_>>();
        let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
        let Some(selection) = crate::stencil_numeric_integer_selection::select_integer_loop(
            code, &entries, &cfg, 0,
        ) else {
            return;
        };
        assert!(matches!(
            selection.recurrence,
            crate::stencil_numeric_integer_loop::IntegerRecurrence::LocalConstant(7)
        ));
        assert_eq!(selection.value_slot, 11);
        assert_eq!(selection.index_slot, 12);
        assert_eq!(selection.state_slot, 6);
        found = true;
    });
    assert!(found, "current local integer loop shape was not admitted");
}

#[test]
fn local_integer_loop_selector_admits_osr_loop_header() {
    let source = "function f(s){var x=17;for(var i=0;i<s.n;i++)x=(x*33+7)|0;return x}";
    let program = crate::reduce::reduce_source(source).expect("counted loop lowers");
    let mut found = false;
    crate::stencil_test_support::visit_code_views(program.code(), &mut |code| {
        let entries = super::baseline_entries(code);
        let operand_windows = (0..entries.len())
            .map(|pc| code.operand_window_at(pc))
            .collect::<Vec<_>>();
        let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
        let Some(selection) = crate::stencil_numeric_integer_selection::select_integer_loop(
            code, &entries, &cfg, 6,
        ) else {
            return;
        };
        assert!(matches!(
            selection.recurrence,
            crate::stencil_numeric_integer_loop::IntegerRecurrence::LocalConstantBody(7)
        ));
        assert_eq!(selection.loop_header_pc, 6);
        assert_eq!(selection.backedge_pc, 24);
        assert_eq!(selection.region_end_pc, 29);
        assert_eq!(selection.value_slot, 11);
        assert_eq!(selection.index_slot, 12);
        assert_eq!(selection.state_slot, 6);
        found = true;
    });
    assert!(found, "local integer loop OSR header was not admitted");
}

#[test]
fn cfg_derived_loop_body_admission_reuses_one_region_plan() {
    let function = super::FunctionCode::from_ops(vec![
        super::Op::CheckInitialized {
            slot: 0,
            name: "left".into(),
        },
        super::Op::LoadLocal { dst: 1, slot: 0 },
        super::Op::CheckInitialized {
            slot: 1,
            name: "right".into(),
        },
        super::Op::LoadLocal { dst: 2, slot: 1 },
        super::Op::Binary {
            dst: 3,
            operator: crate::ops::BinaryOp::Add,
            lhs: 1,
            rhs: 2,
        },
        super::Op::StoreLocal { slot: 3, src: 3 },
        super::Op::Move { dst: 4, src: 3 },
        super::Op::LoadLocal { dst: 5, slot: 0 },
        super::Op::Const {
            dst: 6,
            value: crate::ops::Constant::Number(1.0),
        },
        super::Op::Binary {
            dst: 7,
            operator: crate::ops::BinaryOp::NumericAdd,
            lhs: 5,
            rhs: 6,
        },
        super::Op::CheckInitialized {
            slot: 0,
            name: "left".into(),
        },
        super::Op::StoreLocal { slot: 0, src: 7 },
        super::Op::Return { src: 4 },
    ]);
    let code = function.code().expect("loop body lowers");
    let plan = super::BaselinePlan::compile_for_test(
        code,
        crate::stencil_policy::ExecutionPolicy::bridge_opt_in_for_test(),
    );
    let region = plan
        .native_region_at(0)
        .expect("CFG-derived loop body admission");
    let region = region.borrow();
    assert_eq!(
        region.key_for_test(),
        crate::stencil_select::loop_body_region_key()
    );
    assert_eq!(region.trace_operations().len(), 7);
    assert!(region.admitted_control_for_test().is_some());
}

#[test]
fn ordinary_call_loop_lowers_to_one_cfg_and_preserves_throw() {
    let source = concat!(
        "var calls=0,caught=0;function step(x){calls++;if(calls===3)throw 9;return x+1}",
        "function run(n){var x=0;try{for(var i=0;i<n;i++)x=step(x)}catch(e){caught=e}return x}",
        "if(run(5)!==2||calls!==3||caught!==9)throw new Error([calls,caught])"
    );
    let program = crate::reduce::reduce_source(source).expect("call loop lowers");
    let mut matched = false;
    crate::stencil_test_support::visit_code_views(program.code(), &mut |code| {
        let has_call = (0..code.len()).any(|pc| {
            code.instruction(pc).is_some_and(|instruction| {
                matches!(
                    instruction.opcode,
                    crate::ir::Opcode::Call | crate::ir::Opcode::CallN
                )
            })
        });
        let has_backedge = (0..code.len()).any(|pc| {
            code.instruction(pc).is_some_and(|instruction| {
                instruction.opcode == crate::ir::Opcode::Jump && usize::from(instruction.a) <= pc
            })
        });
        matched |= has_call && has_backedge;
    });
    assert!(matched, "ordinary call loop retained fragment gateway");
    crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
        .expect("flattened call loop executes");
}

#[cfg(feature = "execution-trace")]
#[test]
fn trace_metadata_does_not_change_counted_loop_lowering() {
    let source = concat!(
        "function f(s){var x=17;for(var i=0;i<s.n;i++)x=(x*33+7)|0;return x}",
        "if(f({n:4})!==20420077)throw new Error('bad loop')"
    );
    let program = crate::reduce::reduce_source(source).expect("counted loop lowers");
    let mut matched = false;
    crate::stencil_test_support::visit_code_views(program.code(), &mut |code| {
        let has_mul = (0..code.len()).any(|pc| {
            code.instruction(pc)
                .is_some_and(|instruction| instruction.opcode == crate::ir::Opcode::Mul)
        });
        let has_backedge = (0..code.len()).any(|pc| {
            code.instruction(pc).is_some_and(|instruction| {
                instruction.opcode == crate::ir::Opcode::Jump && usize::from(instruction.a) <= pc
            })
        });
        matched |= has_mul && has_backedge;
    });
    assert!(matched, "trace metadata changed canonical loop shape");
}

#[test]
fn ordinary_source_freezes_lowering_frame_width() {
    let mut source = String::from("function f(){");
    for _ in 0..80 {
        source.push_str("try{}finally{}");
    }
    source.push_str("return 1}if(f()!==1)throw 0");
    let program = crate::reduce::reduce_source(&source).expect("source lowers");
    let mut widths = Vec::new();
    crate::stencil_test_support::visit_code_views(program.code(), &mut |code| {
        for (_, op) in code.cold_ops() {
            if let super::Op::MakeFunctionWithKind { body, .. } = op {
                if let Some(declared) = body.declared_frame_register_count() {
                    widths.push((declared, body.required_register_count()));
                }
            }
        }
    });
    assert!(
        !widths.is_empty(),
        "source produced no declared function frame"
    );
    assert!(widths.iter().all(|(declared, linked)| declared == linked));
    crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
        .expect("source executes");
}

#[test]
fn nested_function_registers_do_not_widen_the_caller_frame() {
    let nested = super::FunctionCode::pending(vec![
        super::Op::Move { dst: 200, src: 199 },
        super::Op::Return { src: 200 },
    ]);
    let owner = super::FunctionCode::from_ops(vec![super::Op::MakeFunctionWithKind {
        dst: 0,
        body: nested,
        params: 0,
        length: 0,
        captures: 0,
        kind: crate::ops::FunctionKind::Ordinary,
        strictness: crate::ops::FunctionStrictness::Sloppy,
        is_async: false,
        mapped_arguments: false,
        source: None,
    }]);
    let code = owner.code().expect("owner code");
    let crate::ops::Op::MakeFunctionWithKind { body, .. } = code.cold_at(0).expect("function op")
    else {
        panic!("expected nested function")
    };
    let store = owner.store().expect("shared code store");
    assert_eq!(store.frame_register_count(owner.code_id()), Some(1));
    assert_eq!(store.frame_register_count(body.code_id()), Some(201));
    assert_eq!(owner.required_register_count(), 1);
}

#[test]
fn structured_fragment_registers_are_frozen_into_the_shared_frame() {
    let body = super::FunctionCode::pending(vec![
        super::Op::Move { dst: 50, src: 49 },
        super::Op::Return { src: 50 },
    ]);
    let owner = super::FunctionCode::from_ops(vec![super::Op::Label {
        name: "shared".into(),
        body,
    }]);
    assert_eq!(owner.required_register_count(), 51);
    assert_eq!(owner.code().expect("owner code").register_count(), 1);
}

#[test]
fn static_block_callable_does_not_widen_the_enclosing_frame() {
    let body = super::FunctionCode::pending(vec![
        super::Op::Move { dst: 80, src: 79 },
        super::Op::Return { src: 80 },
    ]);
    let owner = super::FunctionCode::from_ops(vec![super::Op::StaticBlock {
        constructor: 0,
        captures: 0,
        body,
    }]);
    assert_eq!(owner.required_register_count(), 1);
    let code = owner.code().expect("owner code");
    let crate::ops::Op::StaticBlock { body, .. } = code.cold_at(0).expect("static block") else {
        panic!("expected static block")
    };
    let store = owner.store().expect("shared code store");
    assert_eq!(store.frame_register_count(body.code_id()), Some(81));
}

#[test]
fn baseline_region_admission_respects_declared_abi() {
    let function = super::FunctionCode::from_ops(vec![
        super::Op::Binary {
            dst: 0,
            operator: crate::ops::BinaryOp::Add,
            lhs: 1,
            rhs: 2,
        },
        super::Op::Return { src: 0 },
    ]);
    let code = function.code().expect("compact code");
    let plan = super::BaselinePlan::compile_for_test(
        code,
        crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
    );
    // The Add/Return row is a scalar f64 entry, not a NativeRegionContext
    // bridge.  It must be handled by NativeBinaryPlan/canonical dispatch;
    // opcode adjacency alone cannot construct a region plan with the wrong
    // ABI.
    assert!(plan.native_region_at(0).is_none());
}

#[test]
fn generic_region_admission_excludes_typed_only_abis() {
    use crate::stencil_select::RegionAbi;

    for abi in [
        RegionAbi::Bridge,
        RegionAbi::ArrayKernel,
        RegionAbi::ArrayNumericLoop,
        RegionAbi::AffineI32Loop,
    ] {
        assert!(
            super::generic_region_context_abi_for_test(abi),
            "generic region ABI unexpectedly rejected: {abi:?}"
        );
    }
    for abi in [
        RegionAbi::ArrayCopyLoop,
        RegionAbi::ArrayReductionLoop,
        RegionAbi::I32CounterLoop,
        RegionAbi::BooleanReductionLoop,
        RegionAbi::BranchRecurrenceLoop,
        RegionAbi::NestedXorLoop,
        RegionAbi::SwitchReductionLoop,
        RegionAbi::MatrixReductionLoop,
        RegionAbi::TypedLaneLoop,
        RegionAbi::TwoStateI32Loop,
        RegionAbi::NumericF64Loop,
        RegionAbi::NumericI32BitwiseLoop,
        RegionAbi::NumericI32PairLoop,
        RegionAbi::NumericF64MixedLoop,
        RegionAbi::CompareBranch,
    ] {
        assert!(
            !super::generic_region_context_abi_for_test(abi),
            "typed-only ABI leaked into generic region admission: {abi:?}"
        );
    }
}

#[test]
fn eager_straight_line_admission_has_no_numeric_dag_window() {
    let mut operations = (0..40)
        .map(|slot| super::Op::Move {
            dst: slot as u16,
            src: slot as u16,
        })
        .collect::<Vec<_>>();
    operations.push(super::Op::Return { src: 0 });
    let function = super::FunctionCode::from_ops(operations);
    let code = function.code().expect("compact code");

    // This body is longer than the numeric DAG's fixed storage window.  It
    // is still a valid straight-line candidate; the DAG's own bounded
    // selector must not silently narrow admission for unrelated operations.
    assert!(super::eager_straight_line_candidate(code));
}

#[test]
fn eager_straight_line_admission_rejects_allocating_operations() {
    let function = super::FunctionCode::from_ops(vec![
        super::Op::MakeObject {
            dst: 0,
            properties: Vec::new(),
        },
        super::Op::Return { src: 0 },
    ]);
    let code = function.code().expect("allocating compact code");
    assert!(!super::eager_straight_line_candidate(code));
}

#[test]
fn eager_admission_uses_effect_facts_for_unshaped_heap_reads() {
    let function = super::FunctionCode::from_ops(vec![
        super::Op::GetProperty {
            dst: 0,
            object: 1,
            key: "value".into(),
        },
        super::Op::Return { src: 0 },
    ]);
    let code = function.code().expect("property-read compact code");
    // No recipe-shaped call/property sequence is required merely to build the
    // baseline facts. The read/throw/observable boundary remains canonical at
    // execution time when no specialized admission proves it.
    assert!(super::eager_straight_line_candidate(code));
}

#[test]
fn baseline_admissions_use_sparse_indexed_storage() {
    let function = numeric_admission_function(1);
    let code = function.code().expect("compact code");
    let plan = super::BaselinePlan::compile_for_test(
        code,
        crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
    );
    let admission = plan.admission.as_ref().expect("numeric admission");
    assert_eq!(admission.spans_len(), plan.entries.len());
    assert!(admission.entries_len() <= plan.entries.len() * 13);
    assert!(admission.charged_bytes() > 0);
    assert!(
        admission.charged_bytes() <= crate::stencil_admission_budget::MAX_OWNER_ADMISSION_BYTES
    );
    assert!(
        crate::stencil_admission_budget::global_admission_bytes()
            <= crate::stencil_admission_budget::MAX_GLOBAL_ADMISSION_BYTES
    );
    assert!(std::mem::size_of::<crate::stencil_admission::AdmissionSpan>() <= 8);
}

#[test]
fn disabled_native_policy_keeps_admission_and_executable_storage_empty() {
    let function = super::FunctionCode::from_ops(vec![
        super::Op::Binary {
            dst: 0,
            operator: crate::ops::BinaryOp::Add,
            lhs: 1,
            rhs: 2,
        },
        super::Op::Return { src: 0 },
    ]);
    let disabled = crate::stencil_policy::ExecutionPolicy {
        native_leaves: false,
        local_fusions: crate::stencil_policy::LocalFusionPolicy::NONE,
        native_dispatch: false,
        fused_regions: false,
        array_kernels: false,
        array_numeric_loops: false,
        affine_i32_loops: false,
        numeric_i32_bitwise_loops: false,
        numeric_i32_pair_loops: false,
        numeric_f64_loops: false,
        numeric_f64_mixed_loops: false,
        optimizing_view: false,
    };
    let plan =
        super::BaselinePlan::compile_for_test(function.code().expect("compact code"), disabled);
    assert!(plan.admission.is_none());
    assert!(!plan.has_admission_at(0));
    assert_eq!(plan.shared_region_arena.borrow().slab_count(), 0);
    assert_eq!(plan.shared_region_arena.borrow().capacity(), 0);
}

#[test]
fn optimizing_entries_reuse_sparse_admission_storage() {
    let function = super::FunctionCode::from_ops(vec![
        super::Op::Binary {
            dst: 0,
            operator: crate::ops::BinaryOp::Add,
            lhs: 1,
            rhs: 2,
        },
        super::Op::Return { src: 0 },
    ]);
    let code = function.code().expect("compact code");
    let mut policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    policy.fused_regions = true;
    let baseline = super::BaselinePlan::compile_for_test(code, policy);
    let optimizing = super::OptimizingPlan::compile(&baseline, policy);
    assert_eq!(optimizing.entries.len(), baseline.entries.len());
    assert!(baseline.has_admission_at(0));
    assert!(std::rc::Rc::ptr_eq(&optimizing.entries, &baseline.entries));
    let baseline_admission = baseline.admission.as_ref().expect("baseline admission");
    let optimizing_admission = optimizing.admission.as_ref().expect("optimizing admission");
    assert!(std::rc::Rc::ptr_eq(
        optimizing_admission,
        baseline_admission
    ));
    assert!((0..optimizing.len()).all(|pc| {
        optimizing
            .entry(pc)
            .is_some_and(|entry| entry.admissions.len() <= 13)
    }));
}

fn numeric_admission_function(count: usize) -> super::FunctionCode {
    let mut operations = Vec::with_capacity(count + 1);
    operations.extend((0..count).map(|_| super::Op::Binary {
        dst: 0,
        operator: crate::ops::BinaryOp::Add,
        lhs: 1,
        rhs: 2,
    }));
    operations.push(super::Op::Return { src: 0 });
    super::FunctionCode::from_ops(operations)
}

#[test]
fn admission_metadata_stays_within_owner_budget_and_falls_back() {
    let function = numeric_admission_function(2048);
    let code = function.code().expect("compact code");
    let mut policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    policy.fused_regions = true;
    let plan = super::BaselinePlan::compile_for_test(code, policy);
    assert!(
        plan.admission
            .as_ref()
            .expect("bounded prefix")
            .charged_bytes()
            <= crate::stencil_admission_budget::MAX_OWNER_ADMISSION_BYTES
    );
    let cold = (0..2048)
        .find(|pc| !plan.has_admission_at(*pc))
        .expect("owner budget must leave a canonical suffix");
    let mut registers = crate::register_file::RegisterFile::with_undefined(3);
    registers.write_number(1, 1.0);
    registers.write_number(2, 2.0);
    let result = crate::vm::execute_baseline_code_from(
        code,
        &plan,
        cold,
        &mut registers,
        &crate::vm::current_context_or_default(),
        crate::environment::Environment::new(),
    );
    assert!(matches!(
        result,
        Ok((
            crate::completion::Completion::Return(crate::value::Value::Number(3.0)),
            _
        ))
    ));
}

#[test]
fn final_plan_view_owns_one_admission_charge() {
    let baseline = super::BaselinePlan::compile_for_test(
        numeric_admission_function(1).code().unwrap(),
        crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
    );
    let weak = std::rc::Rc::downgrade(baseline.admission.as_ref().expect("populated admission"));
    let clone = baseline.clone();
    let optimizing = super::OptimizingPlan::compile(
        &baseline,
        crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
    );
    drop(baseline);
    drop(clone);
    assert!(weak.upgrade().is_some());
    drop(optimizing);
    assert!(weak.upgrade().is_none());
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn baseline_scalar_entry_rebuilds_after_shared_owner_eviction() {
    let function = super::FunctionCode::from_ops(vec![
        super::Op::Binary {
            dst: 0,
            operator: crate::ops::BinaryOp::Add,
            lhs: 1,
            rhs: 2,
        },
        super::Op::Return { src: 0 },
    ]);
    let code = function.code().expect("compact code");
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut plan = super::BaselinePlan::compile_for_test(code, policy);
    let native = plan.native_binary_at(0).expect("scalar entry");
    let context = crate::vm::current_context_or_default();
    let environment = crate::environment::Environment::new();
    let mut registers = crate::register_file::RegisterFile::from_values(vec![
        crate::value::Value::Undefined,
        crate::value::Value::Number(2.0),
        crate::value::Value::Number(3.0),
    ]);
    crate::vm::execute_baseline_code_from(
        code,
        &plan,
        0,
        &mut registers,
        &context,
        std::rc::Rc::clone(&environment),
    )
    .expect("first scalar execution");
    assert_eq!(native.borrow().native_entry_count, 1);
    assert_eq!(registers.read(0), Some(crate::value::Value::Number(5.0)));
    assert_eq!(plan.shared_region_arena.borrow_mut().evict_idle(0), 1);
    registers.write(0, crate::value::Value::Undefined);
    crate::vm::execute_baseline_code_from(code, &plan, 0, &mut registers, &context, environment)
        .expect("scalar execution after owner eviction");
    assert_eq!(native.borrow().native_entry_count, 2);
    assert_eq!(registers.read(0), Some(crate::value::Value::Number(5.0)));
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn warm_numeric_region_reuses_one_publication() {
    let function = numeric_add_function();
    let code = function.code().expect("numeric code");
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let plan = super::BaselinePlan::compile_for_test(code, policy);
    let context = crate::vm::current_context_or_default();
    let mut registers = crate::register_file::RegisterFile::with_undefined(3);
    execute_numeric_add(code, &plan, &mut registers, &context);
    let published = {
        let slab = plan.shared_region_arena.borrow();
        (slab.slab_count(), slab.used(), slab.capacity())
    };
    for _ in 0..8 {
        execute_numeric_add(code, &plan, &mut registers, &context);
    }
    assert_eq!(registers.read_number(0), Some(5.0));
    assert_eq!(
        plan.native_binary_at(0)
            .unwrap()
            .borrow()
            .native_entry_count(),
        9
    );
    let slab = plan.shared_region_arena.borrow();
    assert_eq!((slab.slab_count(), slab.used(), slab.capacity()), published);
    assert_eq!(slab.slab_count(), 1, "stable hits share one slab");
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn broken_numeric_fact_does_not_render_native_add() {
    let function = numeric_add_function();
    let code = function.code().expect("numeric code");
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let plan = super::BaselinePlan::compile_for_test(code, policy);
    let mut registers = crate::register_file::RegisterFile::from_values(vec![
        crate::value::Value::Undefined,
        crate::value::Value::String("a".into()),
        crate::value::Value::Number(1.0),
    ]);
    let context = crate::vm::current_context_or_default();
    crate::vm::execute_baseline_code_from(
        code,
        &plan,
        0,
        &mut registers,
        &context,
        crate::environment::Environment::new(),
    )
    .expect("generic Add fallback executes");
    assert_eq!(
        registers.read(0),
        Some(crate::value::Value::String("a1".into()))
    );
    assert_eq!(
        plan.native_binary_at(0)
            .unwrap()
            .borrow()
            .native_entry_count(),
        0
    );
    assert_eq!(plan.shared_region_arena.borrow().capacity(), 0);
}

fn numeric_add_function() -> super::FunctionCode {
    super::FunctionCode::from_ops(vec![
        super::Op::Binary {
            dst: 0,
            operator: crate::ops::BinaryOp::Add,
            lhs: 1,
            rhs: 2,
        },
        super::Op::Return { src: 0 },
    ])
}

fn execute_numeric_add(
    code: super::CodeView<'_>,
    plan: &super::BaselinePlan,
    registers: &mut crate::register_file::RegisterFile,
    context: &crate::vm::VmContext,
) {
    registers.write_number(1, 2.0);
    registers.write_number(2, 3.0);
    crate::vm::execute_baseline_code_from(
        code,
        plan,
        0,
        registers,
        context,
        crate::environment::Environment::new(),
    )
    .expect("numeric region executes");
}

#[cfg(target_arch = "x86_64")]
#[test]
fn native_dispatch_rebuilds_evicted_typed_entry_in_normal_driver() {
    let executable = super::ExecutableCode::from_ops(vec![
        super::Op::Move { dst: 0, src: 1 },
        super::Op::Return { src: 0 },
    ]);
    let policy = crate::stencil_policy::ExecutionPolicy {
        native_leaves: false,
        local_fusions: crate::stencil_policy::LocalFusionPolicy::NONE,
        native_dispatch: true,
        fused_regions: false,
        array_kernels: false,
        array_numeric_loops: false,
        affine_i32_loops: false,
        numeric_i32_bitwise_loops: false,
        numeric_i32_pair_loops: false,
        numeric_f64_loops: false,
        numeric_f64_mixed_loops: false,
        optimizing_view: false,
    };
    let plan = super::BaselinePlan::compile_for_test(executable.code(), policy);
    assert!(plan.native_dispatch_at(0).is_some());
    let pool = plan.shared_stencil_pool_for_test();
    for expected in [7.0, 11.0] {
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            crate::value::Value::Undefined,
            crate::value::Value::Number(expected),
        ]);
        let result = crate::vm::execute_baseline_code_from(
            executable.code(),
            &plan,
            0,
            &mut registers,
            &crate::vm::VmContext::default(),
            crate::environment::Environment::new(),
        )
        .expect("dispatch execution");
        assert_eq!(
            result.0,
            crate::completion::Completion::Return(crate::value::Value::Number(expected))
        );
        assert!(pool.borrow().used() > 0);
        if expected == 7.0 {
            assert!(pool.borrow_mut().evict_idle(0) > 0);
        }
    }
}

#[test]
fn composed_plan_retires_cached_bytes_after_committed_failure() {
    let mut plan =
        super::NativeRegionPlan::new_for_test(crate::stencil_select::array_loop_body_region_key())
            .expect("array region plan");
    plan.physical
        .state
        .cache
        .insert(crate::stencil_fact::RegionKey(900), 0, 0x1000);
    let committed: Result<crate::vm::DispatchTransition, super::NativeDispatchError> =
        Err(super::NativeDispatchError::committed(0, "post-entry"));
    plan.physical.apply_dispatch_outcome(
        &committed,
        None,
        super::InstalledRegionEntry::Unpublished,
    );
    assert_eq!(
        plan.physical.state.cache.len(),
        0,
        "committed bytes must not remain callable"
    );
    assert_eq!(
        plan.physical.state.lifecycle.state(),
        crate::stencil_lifecycle::StencilState::Retired,
        "committed failure must not reset admission history"
    );

    plan.physical
        .state
        .cache
        .insert(crate::stencil_fact::RegionKey(901), 0, 0x2000);
    let semantic: Result<crate::vm::DispatchTransition, super::NativeDispatchError> =
        Err(super::NativeDispatchError::SemanticAt {
            pc: 0,
            error: crate::vm::VmError::EvalError("ordinary throw".into()),
        });
    plan.physical
        .apply_dispatch_outcome(&semantic, None, super::InstalledRegionEntry::Unpublished);
    assert_eq!(
        plan.physical.state.cache.len(),
        1,
        "semantic errors do not invalidate physical code"
    );
}

#[test]
fn bridge_region_follows_verified_forward_join_while_using_binary_leaf() {
    let executable = super::ExecutableCode::from_ops(vec![
        super::Op::LoadLocal { dst: 1, slot: 0 },
        super::Op::LoadLocal { dst: 2, slot: 1 },
        super::Op::Binary {
            dst: 3,
            operator: crate::ops::BinaryOp::LessThan,
            lhs: 1,
            rhs: 2,
        },
        super::Op::Branch {
            condition: 3,
            then_ops: super::FunctionCode::pending(vec![
                super::Op::Const {
                    dst: 4,
                    value: crate::ops::Constant::Number(10.0),
                },
                super::Op::Move { dst: 5, src: 4 },
            ]),
            else_ops: super::FunctionCode::pending(vec![
                super::Op::Const {
                    dst: 6,
                    value: crate::ops::Constant::Number(20.0),
                },
                super::Op::Move { dst: 5, src: 6 },
            ]),
        },
        super::Op::Return { src: 5 },
    ]);
    let code = executable.code();
    let key = crate::stencil_select::binary_branch_glue_region_key();
    let record = crate::stencil_select::select_region(key).expect("binary branch row");
    assert_eq!(code.len(), record.operations.len());
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let control = cfg
        .region_plan(&entries, 0, record.operations)
        .expect("verified branch CFG");
    let context = crate::vm::current_context_or_default();
    for (left, right, expected, expected_retired) in [
        (1.0, 2.0, 10.0, 8),
        (3.0, 2.0, 20.0, 7),
    ] {
        let (transition, native, branch_entries, constant_entries, move_entries, load_entries, retired) = crate::stencil_policy::with_policy_for_test(
            crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
            || {
                let environment = crate::environment::Environment::new();
                environment.set(0, crate::value::Value::Number(left));
                environment.set(1, crate::value::Value::Number(right));
                let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
                let arena = std::rc::Rc::new(std::cell::RefCell::new(
                    crate::stencil_arena::SharedStencilSlab::new(4096).expect("region arena"),
                ));
                let mut region = super::NativeRegionPlan::new_inner(
                    key,
                    true,
                    arena,
                    Some(control.clone()),
                )
                .expect("binary branch region plan");
                let mut registers = crate::register_file::RegisterFile::with_undefined(8);
                let transition = region
                    .execute(code, 0, &mut registers, &context, Some(&environment))
                    .expect("branch region execution");
                (
                    transition,
                    region.last_native_execution(),
                    region.branch_entry_count_for_test(),
                    region.constant_entry_count_for_test(),
                    region.move_entry_count_for_test(),
                    region.load_local_entry_count_for_test(),
                    region.retired_operations(),
                )
            },
        );
        assert_eq!(
            transition.completion,
            Some(crate::completion::Completion::Return(
                crate::value::Value::Number(expected),
            ))
        );
        assert!(native);
        assert_eq!(branch_entries, 1);
        assert_eq!(constant_entries, 1);
        assert_eq!(move_entries, 1);
        assert_eq!(load_entries, 2);
        assert_eq!(retired, expected_retired);
    }
}

#[test]
fn control_only_bridge_follows_canonical_join_after_boolean_guard_miss() {
    let executable = super::ExecutableCode::from_ops(vec![
        super::Op::Branch {
            condition: 0,
            then_ops: super::FunctionCode::pending(vec![super::Op::Move { dst: 3, src: 1 }]),
            else_ops: super::FunctionCode::pending(vec![super::Op::Move { dst: 3, src: 2 }]),
        },
        super::Op::Return { src: 3 },
    ]);
    let code = executable.code();
    let key = crate::stencil_select::branch_glue_region_key();
    let record = crate::stencil_select::select_region(key).expect("branch row");
    assert_eq!(code.len(), record.operations.len());
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let control = cfg
        .region_plan(&entries, 0, record.operations)
        .expect("verified branch CFG");
    let context = crate::vm::current_context_or_default();
    for (condition, expected, retired, native, jumps) in [
        (crate::value::Value::Number(0.0), 22.0, 3, false, 0),
        (crate::value::Value::Number(3.0), 11.0, 4, true, 1),
    ] {
        let arena = std::rc::Rc::new(std::cell::RefCell::new(
            crate::stencil_arena::SharedStencilSlab::new(4096).expect("region arena"),
        ));
        let mut region = super::NativeRegionPlan::new_inner(key, true, arena, Some(control.clone()))
            .expect("branch region plan");
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            condition,
            crate::value::Value::Number(11.0),
            crate::value::Value::Number(22.0),
            crate::value::Value::Undefined,
        ]);
        let transition = crate::stencil_policy::with_policy_for_test(
            crate::stencil_policy::ExecutionPolicy::bridge_opt_in_for_test(),
            || {
                region
                    .execute(code, 0, &mut registers, &context, None)
                    .expect("branch region execution")
            },
        );
        assert_eq!(
            transition.completion,
            Some(crate::completion::Completion::Return(
                crate::value::Value::Number(expected),
            ))
        );
        assert_eq!(region.last_native_execution(), native);
        assert_eq!(region.jump_entry_count_for_test(), jumps);
        assert_eq!(region.retired_operations(), retired);
    }
}

#[test]
fn nested_branch_bridge_follows_two_cfg_conditions_into_one_join() {
    let instructions = vec![
        crate::ir::Instruction::load_local(0, 0),
        crate::ir::Instruction::jump_if_false(0, 8),
        crate::ir::Instruction::load_local(1, 1),
        crate::ir::Instruction::jump_if_false(1, 6),
        crate::ir::Instruction::load_const(2, 0),
        crate::ir::Instruction::jump(9),
        crate::ir::Instruction::load_const(2, 1),
        crate::ir::Instruction::jump(9),
        crate::ir::Instruction::load_const(2, 2),
        crate::ir::Instruction::ret(2),
    ];
    let store = std::rc::Rc::new(super::CodeStore {
        instructions: instructions.into(),
        cold: Vec::<super::Op>::new().into(),
        ranges: vec![(0, 10)].into(),
        constants: vec![super::ConstantPool::new(vec![
            super::Constant::Number(1.0),
            super::Constant::Number(2.0),
            super::Constant::Number(3.0),
        ])]
        .into(),
        metadata: vec![vec![super::InstructionMeta::empty(); 10]].into(),
        layouts: vec![super::FunctionLayout {
            register_count: 3,
            frame_register_count: 3,
            parameter_end: None,
        }]
        .into(),
        quickening_sites: vec![
            Vec::<std::cell::RefCell<crate::quickening::QuickeningSite<4>>>::new()
                .into_boxed_slice(),
        ]
        .into(),
        operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
        catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
    });
    let range = super::CodeRange::new(super::CodeId(0), 0, 10).expect("nested branch range");
    let code = store.code(range).expect("nested branch code");
    let key = crate::stencil_select::nested_branch_glue_region_key();
    let record = crate::stencil_select::select_region(key).expect("nested branch row");
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let control = cfg
        .region_plan(&entries, 0, record.operations)
        .expect("verified nested branch CFG");
    assert_eq!(control.join_blocks(), [9]);
    assert_eq!(control.internal_backedges(), []);
    let context = crate::vm::current_context_or_default();
    crate::stencil_policy::with_policy_for_test(
        crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
        || {
            for (outer, inner, expected, branches, jumps, loads, retired) in [
                (true, true, 1.0, 2, 1, 2, 7),
                (true, false, 2.0, 2, 1, 2, 7),
                (false, true, 3.0, 1, 0, 1, 4),
            ] {
                let environment = crate::environment::Environment::new();
                environment.set(0, crate::value::Value::Boolean(outer));
                environment.set(1, crate::value::Value::Boolean(inner));
                let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
                let arena = std::rc::Rc::new(std::cell::RefCell::new(
                    crate::stencil_arena::SharedStencilSlab::new(4096).expect("region arena"),
                ));
                let mut region = super::NativeRegionPlan::new_inner(
                    key,
                    true,
                    arena,
                    Some(control.clone()),
                )
                .expect("nested branch region plan");
                let mut registers = crate::register_file::RegisterFile::with_undefined(3);
                let transition = region
                    .execute(code, 0, &mut registers, &context, Some(&environment))
                    .expect("nested branch region execution");
                assert_eq!(
                    transition.completion,
                    Some(crate::completion::Completion::Return(
                        crate::value::Value::Number(expected),
                    ))
                );
                assert!(region.last_native_execution());
                assert_eq!(region.branch_entry_count_for_test(), branches);
                assert_eq!(region.jump_entry_count_for_test(), jumps);
                assert_eq!(region.load_local_entry_count_for_test(), loads);
                assert_eq!(region.constant_entry_count_for_test(), 1);
                assert_eq!(region.retired_operations(), retired);
            }
        },
    );
}

#[test]
fn generic_bridge_admission_uses_operator_facts_not_opcode_names() {
    let generic_binary = |operator| crate::ir::Instruction {
        opcode: crate::ir::Opcode::Binary,
        flags: crate::ir::compact_binary_id(operator),
        a: 0,
        b: 1,
        c: 2,
    };
    assert!(super::generic_bridge_candidate(generic_binary(
        crate::ops::BinaryOp::StrictEqual
    )));
    for operator in [
        crate::ops::BinaryOp::Add,
        crate::ops::BinaryOp::Subtract,
        crate::ops::BinaryOp::Multiply,
        crate::ops::BinaryOp::Divide,
    ] {
        assert!(
            super::generic_bridge_candidate(generic_binary(operator)),
            "generic {operator:?} should use its dedicated numeric leaf when guarded"
        );
    }
    for (operator, expected) in [
        (crate::ops::BinaryOp::Add, 5.0),
        (crate::ops::BinaryOp::Subtract, -1.0),
        (crate::ops::BinaryOp::Multiply, 6.0),
        (crate::ops::BinaryOp::Divide, 2.0 / 3.0),
    ] {
        let instruction = crate::ir::Instruction {
            opcode: crate::ir::Opcode::Binary,
            flags: crate::ir::compact_binary_id(operator),
            a: 0,
            b: 1,
            c: 2,
        };
        let mut plan = super::NativeBinaryPlan::new(
            instruction,
            crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
        )
        .unwrap_or_else(|| panic!("generic {operator:?} must resolve its scalar artifact"));
        assert_eq!(
            plan.execute(2.0, 3.0)
                .unwrap_or_else(|error| panic!("generic {operator:?} leaf failed: {error:?}")),
            expected,
            "generic {operator:?} should use the dedicated scalar semantics"
        );
    }
    for operator in [
        crate::ops::BinaryOp::NumericAdd,
        crate::ops::BinaryOp::NumericSubtract,
    ] {
        assert!(
            super::generic_bridge_candidate(generic_binary(operator)),
            "generic {operator:?} must use the generated update family"
        );
    }
    for opcode in [crate::ir::Opcode::NumericAdd, crate::ir::Opcode::NumericSubtract] {
        assert!(
            super::generic_bridge_candidate(crate::ir::Instruction {
                opcode,
                flags: 0,
                a: 0,
                b: 1,
                c: 2,
            }),
            "{opcode:?} should use the generated ±1 update family"
        );
        let mut plan = super::NativeBinaryPlan::new(
                crate::ir::Instruction {
                    opcode,
                    flags: 0,
                    a: 0,
                    b: 1,
                    c: 2,
                },
                crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
            )
            .unwrap_or_else(|| panic!("{opcode:?} must resolve its generated update artifact"));
        let actual = plan
            .execute(2.0, 99.0)
            .unwrap_or_else(|error| panic!("{opcode:?} update leaf failed: {error:?}"));
        let expected = if opcode == crate::ir::Opcode::NumericAdd {
            3.0
        } else {
            1.0
        };
        assert_eq!(actual, expected, "{opcode:?} must ignore the encoded RHS");
    }
    assert!(super::generic_bridge_candidate(crate::ir::Instruction::inc_i(
        0, 1, false
    )));
    assert!(!super::generic_bridge_candidate(crate::ir::Instruction {
        opcode: crate::ir::Opcode::Remainder,
        flags: 0,
        a: 0,
        b: 1,
        c: 2,
    }));
    for opcode in [crate::ir::Opcode::Exponentiate, crate::ir::Opcode::Instanceof] {
        assert!(
            !super::generic_bridge_candidate(crate::ir::Instruction {
                opcode,
                flags: 0,
                a: 0,
                b: 1,
                c: 2,
            }),
            "{opcode:?} has no generated leaf and must remain canonical"
        );
    }
    assert!(super::generic_bridge_candidate(crate::ir::Instruction::unary_operator(
        0,
        crate::ops::UnaryOp::Not,
        1,
    )));
    assert!(super::generic_bridge_candidate(crate::ir::Instruction::unary_operator(
        0,
        crate::ops::UnaryOp::Plus,
        1,
    )));
    assert!(super::generic_bridge_candidate(crate::ir::Instruction::unary_operator(
        0,
        crate::ops::UnaryOp::Void,
        1,
    )));
    assert!(super::generic_bridge_candidate(crate::ir::Instruction::unary_operator(
        0,
        crate::ops::UnaryOp::Delete,
        1,
    )));
    for operator in [
        crate::ops::UnaryOp::Typeof,
        crate::ops::UnaryOp::ToString,
        crate::ops::UnaryOp::ToNumeric,
    ] {
        assert!(
            !super::generic_bridge_candidate(crate::ir::Instruction::unary_operator(
                0, operator, 1,
            )),
            "unsupported unary {operator:?} must retain canonical fallback"
        );
    }
    assert!(!super::generic_bridge_candidate(crate::ir::Instruction {
        opcode: crate::ir::Opcode::AddConst,
        flags: crate::ir::ADD_CONST_LEFT_FLAG,
        a: 0,
        b: 1,
        c: 0,
    }));
    assert!(super::generic_bridge_candidate(
        crate::ir::Instruction::initialize_local(0)
    ));
    assert!(!super::generic_bridge_candidate(crate::ir::Instruction {
        opcode: crate::ir::Opcode::InitLocal,
        flags: 0,
        a: 0,
        b: 1,
        c: 0,
    }));
    assert!(super::generic_bridge_candidate(crate::ir::Instruction {
        opcode: crate::ir::Opcode::InitLocal,
        flags: 0,
        a: 1,
        b: 2,
        c: 0,
    }));
    assert!(super::generic_bridge_candidate(crate::ir::Instruction {
        opcode: crate::ir::Opcode::Move,
        flags: 1,
        a: 0,
        b: 1,
        c: 2,
    }));
    assert!(!super::generic_bridge_candidate(crate::ir::Instruction {
        opcode: crate::ir::Opcode::Move,
        flags: 2,
        a: 0,
        b: 1,
        c: 2,
    }));
    assert!(super::generic_bridge_candidate(crate::ir::Instruction {
        opcode: crate::ir::Opcode::MarkUninitialized,
        flags: 1,
        a: 0,
        b: 0,
        c: 0,
    }));
    assert!(super::generic_bridge_candidate(crate::ir::Instruction {
        opcode: crate::ir::Opcode::MarkImmutable,
        flags: 0,
        a: 0,
        b: 0,
        c: 0,
    }));
    assert!(super::generic_bridge_candidate(crate::ir::Instruction {
        opcode: crate::ir::Opcode::CheckInitialized,
        flags: 0,
        a: 0,
        b: 0,
        c: 0,
    }));
    assert!(super::generic_bridge_candidate(crate::ir::Instruction {
        opcode: crate::ir::Opcode::RequireObjectCoercible,
        flags: 0,
        a: 0,
        b: 0,
        c: 0,
    }));
    assert!(super::generic_bridge_candidate(crate::ir::Instruction {
        opcode: crate::ir::Opcode::Throw,
        flags: 0,
        a: 0,
        b: 0,
        c: 0,
    }));
    assert!(super::generic_bridge_candidate(crate::ir::Instruction::ret(0)));
    assert_eq!(
        crate::stencil_select::numeric_region_key(crate::ir::Opcode::NumericAdd),
        Some(crate::stencil_select::increment_region_key())
    );
    assert_eq!(
        crate::stencil_select::numeric_region_key(crate::ir::Opcode::NumericSubtract),
        Some(crate::stencil_select::decrement_region_key())
    );
    for opcode in crate::ir::Opcode::ALL {
        let instruction = crate::ir::Instruction {
            opcode: *opcode,
            flags: 0,
            a: 0,
            b: 1,
            c: 2,
        };
        if super::generic_bridge_candidate(instruction) {
            assert!(
                !opcode.has_effect(crate::facts::OperationEffect::ReadHeap)
                    && !opcode.has_effect(crate::facts::OperationEffect::WriteHeap)
                    && !opcode.has_effect(crate::facts::OperationEffect::Allocate)
                    && !opcode.has_effect(crate::facts::OperationEffect::Observable),
                "Bridge candidate {opcode:?} crossed a generated effect boundary"
            );
            assert_ne!(
                opcode.control_flow(),
                crate::facts::ControlFlow::Loop,
                "Bridge candidate {opcode:?} swallowed a structured loop gateway"
            );
        }
    }
}

#[test]
fn generic_bridge_return_is_admitted_only_without_helper_effects() {
    let function = super::FunctionCode::from_ops(vec![
        super::Op::Return { src: 0 },
    ]);
    let code = function.code().expect("return code");
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let arena = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("region arena"),
    ));
    let policy = crate::stencil_policy::ExecutionPolicy::bridge_opt_in_for_test()
        .with_leaf_dependencies();
    let admission = super::generic_cfg_region_admission(&entries, &cfg, 0, policy, &arena);
    assert!(admission.is_some(), "return leaf should be a standalone bridge");

    let helper = super::FunctionCode::from_ops(vec![
        super::Op::Call {
            dst: 0,
            callee: 1,
            receiver: None,
            args: Vec::new(),
            spreads: Vec::new(),
        },
        super::Op::Return { src: 0 },
    ]);
    let helper_code = helper.code().expect("helper code");
    let helper_entries = super::baseline_entries(helper_code);
    let helper_operand_windows = (0..helper_entries.len())
        .map(|pc| helper_code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let helper_cfg = super::ControlFlowFacts::new(&helper_entries, &helper_operand_windows);
    assert!(
        super::generic_cfg_region_admission(
            &helper_entries,
            &helper_cfg,
            0,
            policy,
            &arena,
        )
        .is_none(),
        "helper boundary must stay canonical"
    );

    let result = crate::stencil_policy::with_policy_for_test(policy, || {
        let plan = super::BaselinePlan::compile_for_test(code, policy);
        assert!(plan.native_region_at(0).is_some());
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            crate::value::Value::Number(7.0),
        ]);
        let environment = crate::environment::Environment::new();
        let (completion, next) = crate::vm::execute_baseline_code_from(
            code,
            &plan,
            0,
            &mut registers,
            &crate::vm::VmContext::default(),
            environment,
        )
        .expect("standalone return bridge executes");
        let entered = plan
            .native_region_at(0)
            .is_some_and(|region| region.borrow().last_native_execution());
        (completion, next, entered)
    });
    assert_eq!(
        result.0,
        crate::completion::Completion::Return(crate::value::Value::Number(7.0))
    );
    assert_eq!(result.1, 1);
    assert!(result.2, "validated return artifact should be entered");
}

#[test]
fn generic_bridge_stops_at_helper_boundary_without_poisoning_prefix() {
    let function = super::FunctionCode::from_ops(vec![
        super::Op::Const {
            dst: 0,
            value: crate::ops::Constant::Number(1.0),
        },
        super::Op::Call {
            dst: 0,
            callee: 1,
            receiver: None,
            args: Vec::new(),
            spreads: Vec::new(),
        },
        super::Op::Return { src: 0 },
    ]);
    let code = function.code().expect("prefix/helper code");
    let mut policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    policy.fused_regions = true;
    let plan = super::BaselinePlan::compile_for_test(code, policy);
    let region = plan
        .native_region_at(0)
        .expect("pure prefix remains bridge-admissible");
    assert_eq!(
        region.borrow().trace_operations().as_ref(),
        &[crate::ir::Opcode::LoadConst]
    );
    assert_ne!(
        plan.native_region_at(1)
            .map(|region| region.borrow().key_for_test()),
        Some(crate::stencil_select::dispatch_region_key()),
        "helper boundary must not be folded into the generic bridge"
    );
    let environment = crate::environment::Environment::new();
    let mut registers = crate::register_file::RegisterFile::with_undefined(2);
    let transition = crate::stencil_policy::with_policy_for_test(policy, || {
        plan.native_region_at(0)
            .expect("prefix region")
            .borrow_mut()
            .execute(
                code,
                0,
                &mut registers,
                &crate::vm::VmContext::default(),
                Some(&environment),
            )
    })
    .expect("pure prefix executes");
    assert_eq!(transition.target, crate::vm::DispatchTarget::Callee(1));
    assert!(
        plan.native_region_at(0)
            .is_some_and(|region| region.borrow().last_native_execution()),
        "pure prefix did not reach generated execution"
    );
}

#[test]
fn generic_bridge_keeps_branch_edges_to_helper_boundary_external() {
    let instructions = vec![
        crate::ir::Instruction::jump_if_false(0, 3),
        crate::ir::Instruction::load_const(0, 0),
        crate::ir::Instruction::jump(4),
        crate::ir::Instruction {
            opcode: crate::ir::Opcode::Call,
            flags: 0,
            a: 0,
            b: 1,
            c: 0,
        },
        crate::ir::Instruction::ret(0),
    ];
    let store = std::rc::Rc::new(super::CodeStore {
        instructions: instructions.into(),
        cold: Vec::<super::Op>::new().into(),
        ranges: vec![(0, 5)].into(),
        constants: vec![super::ConstantPool::new(vec![super::Constant::Number(1.0)])].into(),
        metadata: vec![vec![super::InstructionMeta::empty(); 5]].into(),
        layouts: vec![super::FunctionLayout {
            register_count: 2,
            frame_register_count: 2,
            parameter_end: None,
        }]
        .into(),
        quickening_sites: vec![
            Vec::<std::cell::RefCell<crate::quickening::QuickeningSite<4>>>::new()
                .into_boxed_slice(),
        ]
        .into(),
        operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
        catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
    });
    let code = store
        .code(super::CodeRange::new(super::CodeId(0), 0, 5).expect("branch/helper range"))
        .expect("branch/helper code");
    let mut policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    policy.fused_regions = true;
    let plan = super::BaselinePlan::compile_for_test(code, policy);
    let region = plan
        .native_region_at(0)
        .expect("branch prefix remains bridge-admissible");
    assert_eq!(
        region.borrow().trace_operations().as_ref(),
        &[
            crate::ir::Opcode::JumpIfFalse,
            crate::ir::Opcode::LoadConst,
            crate::ir::Opcode::Jump,
        ]
    );

    // Planning alone is insufficient here: execute the prefix and prove the
    // valid external edge reaches the helper boundary at pc 4.  The bridge
    // must retire only its three fused operations and leave the helper to the
    // ordinary dispatcher.
    let context = crate::vm::current_context_or_default();
    let environment = crate::environment::Environment::new();
    let mut registers = crate::register_file::RegisterFile::from_values(vec![
        crate::value::Value::Boolean(true),
        crate::value::Value::Undefined,
    ]);
    let transition = crate::stencil_policy::with_policy_for_test(policy, || {
        region
            .borrow_mut()
            .execute(code, 0, &mut registers, &context, Some(&environment))
            .expect("external helper edge executes")
    });
    assert_eq!(transition.target, crate::vm::DispatchTarget::Callee(4));
    assert_eq!(transition.completion, None);
    assert_eq!(region.borrow().retired_operations(), 3);
    assert!(region.borrow().last_native_execution());
}

#[test]
fn local_numeric_fusion_respects_opaque_register_windows() {
    for opcode in [
        crate::ir::Opcode::MakeObject,
        crate::ir::Opcode::MakeArray,
        crate::ir::Opcode::Construct,
        crate::ir::Opcode::TailCall,
        crate::ir::Opcode::ForOf,
    ] {
        assert!(
            super::implicit_register_consumer(opcode),
            "{opcode:?} consumes an implicit register window"
        );
    }
    assert!(!super::implicit_register_consumer(crate::ir::Opcode::Add));
    assert!(!super::implicit_register_consumer(crate::ir::Opcode::StoreLocal));
}

#[test]
fn generic_cfg_bridge_consumes_initialize_local_transition() {
    let mut arena = super::CodeArena::new();
    let range = arena.append(vec![
        super::Op::InitializeLocal { slot: 0 },
        super::Op::Return { src: 1 },
    ]);
    let store = arena.freeze();
    let code = store.code(range).expect("initialize code");
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let mut policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    policy.fused_regions = true;
    let arena = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("initialize arena"),
    ));
    assert!(super::generic_cfg_region_admission(&entries, &cfg, 0, policy, &arena).is_some());

    crate::stencil_policy::with_policy_for_test(policy, || {
        let baseline = super::BaselinePlan::compile_for_test(code, policy);
        let environment = crate::environment::Environment::new();
        environment.mark_uninitialized(0);
        let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
        let mut registers = crate::register_file::RegisterFile::with_undefined(2);
        registers.write_number(1, 7.0);
        let completion = crate::vm::execute_baseline_code_from(
            code,
            &baseline,
            0,
            &mut registers,
            &crate::vm::current_context_or_default(),
            environment.clone(),
        )
        .expect("initialize execution")
        .0;
        assert_eq!(
            completion,
            crate::completion::Completion::Return(crate::value::Value::Number(7.0))
        );
        assert!(!environment.is_uninitialized(0));
        let region = baseline.native_region_at(0).expect("initialize region");
        let region = region.borrow();
        assert!(region.last_native_execution());
        assert_eq!(region.retired_operations(), 2);
    });
}

#[test]
fn generic_cfg_bridge_consumes_init_local_value_transition() {
    let instructions = vec![
        crate::ir::Instruction::init_local(1, 2),
        crate::ir::Instruction::ret(2),
    ];
    let store = std::rc::Rc::new(super::CodeStore {
        instructions: instructions.into(),
        cold: Vec::<super::Op>::new().into(),
        ranges: vec![(0, 2)].into(),
        constants: vec![super::ConstantPool::empty()].into(),
        metadata: vec![vec![super::InstructionMeta::empty(); 2]].into(),
        layouts: vec![super::FunctionLayout {
            register_count: 3,
            frame_register_count: 3,
            parameter_end: None,
        }]
        .into(),
        quickening_sites: vec![
            Vec::<std::cell::RefCell<crate::quickening::QuickeningSite<4>>>::new()
                .into_boxed_slice(),
        ]
        .into(),
        operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
        catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
    });
    let range = super::CodeRange::new(super::CodeId(0), 0, 2).expect("init-local range");
    let code = store.code(range).expect("init-local code");
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let mut policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    policy.fused_regions = true;
    let arena = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("init-local arena"),
    ));
    assert!(super::generic_cfg_region_admission(&entries, &cfg, 0, policy, &arena).is_some());

    crate::stencil_policy::with_policy_for_test(policy, || {
        let baseline = super::BaselinePlan::compile_for_test(code, policy);
        let environment = crate::environment::Environment::new();
        environment.mark_uninitialized(1);
        let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
        let mut registers = crate::register_file::RegisterFile::with_undefined(3);
        registers.write_number(2, 13.0);
        let completion = crate::vm::execute_baseline_code_from(
            code,
            &baseline,
            0,
            &mut registers,
            &crate::vm::current_context_or_default(),
            environment.clone(),
        )
        .expect("init-local execution")
        .0;
        assert_eq!(
            completion,
            crate::completion::Completion::Return(crate::value::Value::Number(13.0))
        );
        assert!(!environment.is_uninitialized(1));
        assert_eq!(environment.get_number(1), Some(13.0));
        let region = baseline.native_region_at(0).expect("init-local region");
        let region = region.borrow();
        assert!(region.last_native_execution());
        assert_eq!(region.retired_operations(), 2);
    });
}

#[test]
fn generic_cfg_bridge_consumes_environment_metadata_transitions() {
    let mut arena = super::CodeArena::new();
    let range = arena.append(vec![
        super::Op::MarkUninitialized {
            slot: 0,
            shared: true,
        },
        super::Op::InitializeLocal { slot: 0 },
        super::Op::CheckInitialized {
            slot: 0,
            name: "x".into(),
        },
        super::Op::MarkImmutable { slot: 0 },
        super::Op::Return { src: 1 },
    ]);
    let store = arena.freeze();
    let code = store.code(range).expect("metadata code");
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let mut policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    policy.fused_regions = true;
    let arena = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("metadata arena"),
    ));
    assert!(super::generic_cfg_region_admission(&entries, &cfg, 0, policy, &arena).is_some());

    crate::stencil_policy::with_policy_for_test(policy, || {
        let baseline = super::BaselinePlan::compile_for_test(code, policy);
        let environment = crate::environment::Environment::new();
        let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
        let mut registers = crate::register_file::RegisterFile::with_undefined(2);
        registers.write_number(1, 11.0);
        let completion = crate::vm::execute_baseline_code_from(
            code,
            &baseline,
            0,
            &mut registers,
            &crate::vm::current_context_or_default(),
            environment.clone(),
        )
        .expect("metadata execution")
        .0;
        assert_eq!(
            completion,
            crate::completion::Completion::Return(crate::value::Value::Number(11.0))
        );
        assert!(!environment.is_uninitialized(0));
        assert!(environment.is_immutable_slot(0));
        let region = baseline.native_region_at(0).expect("metadata region");
        let region = region.borrow();
        assert!(region.last_native_execution());
        assert_eq!(region.retired_operations(), 5);
    });
}

#[test]
fn generic_cfg_bridge_guards_object_coercibility_before_canonical_throw() {
    let mut arena = super::CodeArena::new();
    let range = arena.append(vec![
        super::Op::RequireObjectCoercible { src: 1 },
        super::Op::Return { src: 1 },
    ]);
    let store = arena.freeze();
    let code = store.code(range).expect("coercibility code");
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let mut policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    policy.fused_regions = true;
    let arena = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("coercibility arena"),
    ));
    assert!(super::generic_cfg_region_admission(&entries, &cfg, 0, policy, &arena).is_some());

    crate::stencil_policy::with_policy_for_test(policy, || {
        for (value, native) in [
            (crate::value::Value::Number(5.0), true),
            (crate::value::Value::Null, false),
        ] {
            let baseline = super::BaselinePlan::compile_for_test(code, policy);
            let environment = crate::environment::Environment::new();
            let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
            let mut registers = crate::register_file::RegisterFile::from_values(vec![
                crate::value::Value::Undefined,
                value,
            ]);
            let result = crate::vm::execute_baseline_code_from(
                code,
                &baseline,
                0,
                &mut registers,
                &crate::vm::current_context_or_default(),
                environment,
            );
            if native {
                assert_eq!(
                    result.expect("non-nullish coercibility").0,
                    crate::completion::Completion::Return(crate::value::Value::Number(5.0))
                );
            } else {
                assert!(
                    matches!(
                        result.expect("canonical coercibility throw").0,
                        crate::completion::Completion::Throw(_)
                    ),
                    "nullish coercibility must throw"
                );
            }
            let region = baseline.native_region_at(0).expect("coercibility region");
            assert_eq!(region.borrow().last_native_execution(), native);
        }
    });
}

#[test]
fn generic_cfg_bridge_materializes_terminal_throw_completion() {
    let mut arena = super::CodeArena::new();
    let range = arena.append(vec![
        super::Op::Const {
            dst: 1,
            value: crate::ops::Constant::Number(21.0),
        },
        super::Op::Throw { src: 1 },
    ]);
    let store = arena.freeze();
    let code = store.code(range).expect("throw code");
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let mut policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    policy.fused_regions = true;
    let arena = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("throw arena"),
    ));
    assert!(super::generic_cfg_region_admission(&entries, &cfg, 0, policy, &arena).is_some());

    crate::stencil_policy::with_policy_for_test(policy, || {
        let baseline = super::BaselinePlan::compile_for_test(code, policy);
        let environment = crate::environment::Environment::new();
        let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
        let mut registers = crate::register_file::RegisterFile::with_undefined(2);
        let completion = crate::vm::execute_baseline_code_from(
            code,
            &baseline,
            0,
            &mut registers,
            &crate::vm::current_context_or_default(),
            environment,
        )
        .expect("throw execution")
        .0;
        assert_eq!(
            completion,
            crate::completion::Completion::Throw(crate::value::Value::Number(21.0))
        );
        let region = baseline.native_region_at(0).expect("throw region");
        let region = region.borrow();
        assert!(region.last_native_execution());
        assert_eq!(region.retired_operations(), 2);
    });
}

#[test]
fn generic_cfg_bridge_admits_multiple_terminal_exits_without_static_shape() {
    let instructions = vec![
        crate::ir::Instruction::load_local(0, 0),
        crate::ir::Instruction::jump_if_false(0, 6),
        crate::ir::Instruction::load_const(1, 0),
        crate::ir::Instruction::load_const(2, 1),
        crate::ir::Instruction::binary(crate::ir::Opcode::Add, 3, 1, 2),
        crate::ir::Instruction::ret(3),
        crate::ir::Instruction::load_const(3, 2),
        crate::ir::Instruction::ret(3),
    ];
    let store = std::rc::Rc::new(super::CodeStore {
        instructions: instructions.into(),
        cold: Vec::<super::Op>::new().into(),
        ranges: vec![(0, 8)].into(),
        constants: vec![super::ConstantPool::new(vec![
            super::Constant::Number(1.0),
            super::Constant::Number(2.0),
            super::Constant::Number(9.0),
        ])]
        .into(),
        metadata: vec![vec![super::InstructionMeta::empty(); 8]].into(),
        layouts: vec![super::FunctionLayout {
            register_count: 4,
            frame_register_count: 4,
            parameter_end: None,
        }]
        .into(),
        quickening_sites: vec![
            Vec::<std::cell::RefCell<crate::quickening::QuickeningSite<4>>>::new()
                .into_boxed_slice(),
        ]
        .into(),
        operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
        catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
    });
    let range = super::CodeRange::new(super::CodeId(0), 0, 8).expect("generic CFG range");
    let code = store.code(range).expect("generic CFG code");
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let operations = entries
        .iter()
        .map(|entry| entry.instruction.opcode)
        .collect::<Vec<_>>();
    let control = cfg
        .region_plan_with_terminal_exits(&entries, 0, &operations)
        .expect("multiple terminal exits are CFG-valid");
    assert_eq!(control.end(), 8);
    assert_eq!(control.internal_backedges(), []);

    let dispatch_key = crate::stencil_select::dispatch_region_key();
    let dispatch = crate::stencil_select::select_region(dispatch_key).expect("dispatch row");
    assert_eq!(dispatch.abi, crate::stencil_select::RegionAbi::Bridge);
    assert!(dispatch.executable);
    assert!(super::generic_region_context_abi_for_test(dispatch.abi));
    let dispatch_view = crate::stencil_select::select_physical(dispatch_key).expect("dispatch view");
    assert!(dispatch_view.executable);
    assert_eq!(super::validate_physical_view(dispatch, dispatch_view.stencil), Ok(()));
    let mut policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    policy.fused_regions = true;
    let candidate_arena = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("candidate arena"),
    ));
    assert!(super::generic_cfg_region_admission(
        &entries,
        &cfg,
        0,
        policy,
        &candidate_arena
    )
    .is_some());

    crate::stencil_policy::with_policy_for_test(policy, || {
        let baseline = super::BaselinePlan::compile_for_test(code, policy);
        let region = baseline.native_region_at(0).expect("generic CFG admission");
        assert_eq!(region.borrow().key_for_test(), dispatch_key);
        assert_eq!(region.borrow().trace_operations().len(), 8);
        let context = crate::vm::current_context_or_default();
        for (condition, expected, constants, binaries, loads, branches, retired) in [
            (true, 3.0, 2, 1, 1, 1, 6),
            (false, 9.0, 1, 0, 1, 1, 4),
        ] {
            let baseline = super::BaselinePlan::compile_for_test(code, policy);
            let environment = crate::environment::Environment::new();
            environment.set(0, crate::value::Value::Boolean(condition));
            let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
            let mut registers = crate::register_file::RegisterFile::with_undefined(4);
            let completion = crate::vm::execute_baseline_code_from(
                code,
                &baseline,
                0,
                &mut registers,
                &context,
                environment,
            )
            .expect("generic CFG execution")
            .0;
            assert_eq!(
                completion,
                crate::completion::Completion::Return(crate::value::Value::Number(expected))
            );
            let region = baseline.native_region_at(0).expect("region retained");
            let region = region.borrow();
            assert_eq!(region.key_for_test(), dispatch_key);
            assert!(region.last_native_execution());
            assert_eq!(region.constant_entry_count_for_test(), constants);
            assert_eq!(region.binary_entry_count_for_test(), binaries);
            assert_eq!(region.load_local_entry_count_for_test(), loads);
            assert_eq!(region.branch_entry_count_for_test(), branches);
            assert_eq!(region.retired_operations(), retired);
        }
    });
}

#[test]
fn generic_cfg_bridge_stops_before_allocation_boundary() {
    let instructions = vec![
        crate::ir::Instruction::load_const(0, 0),
        crate::ir::Instruction {
            opcode: crate::ir::Opcode::MakeObject,
            flags: 0,
            a: 0,
            b: 0,
            c: 0,
        },
        crate::ir::Instruction::ret(0),
    ];
    let entries = instructions
        .into_iter()
        .map(|instruction| super::BaselineEntry {
            control: instruction.opcode.control_operands(instruction),
            instruction,
        })
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &[None; 3]);
    assert_eq!(cfg.pure_control_prefix_end(0), Some(1));
    assert_eq!(cfg.pure_control_prefix_end(1), Some(1));
    assert_eq!(cfg.pure_control_prefix_end(2), Some(3));

    let arena = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("region arena"),
    ));
    let policy = crate::stencil_policy::ExecutionPolicy::bridge_opt_in_for_test()
        .with_leaf_dependencies();
    let admission = super::generic_cfg_region_admission(&entries, &cfg, 0, policy, &arena)
        .expect("value prefix before allocation should remain admissible");
    let super::NativeAdmission::Region(region) = admission else {
        panic!("allocation boundary selected a non-region admission");
    };
    assert_eq!(region.borrow().admitted_control_for_test().unwrap().span_len(), 1);
}

#[test]
fn generic_cfg_bridge_resides_arbitrary_numeric_backedge() {
    let instructions = vec![
        crate::ir::Instruction::load_local(0, 0),
        crate::ir::Instruction::jump_if_false(0, 6),
        crate::ir::Instruction::update_local(1, 2, 0, true),
        crate::ir::Instruction::jump(0),
        crate::ir::Instruction::load_const(3, 0),
        crate::ir::Instruction::ret(3),
        crate::ir::Instruction::load_local(3, 0),
        crate::ir::Instruction::ret(3),
    ];
    let store = std::rc::Rc::new(super::CodeStore {
        instructions: instructions.into(),
        cold: Vec::<super::Op>::new().into(),
        ranges: vec![(0, 8)].into(),
        constants: vec![super::ConstantPool::new(vec![super::Constant::Number(-1.0)])].into(),
        metadata: vec![vec![super::InstructionMeta::empty(); 8]].into(),
        layouts: vec![super::FunctionLayout {
            register_count: 4,
            frame_register_count: 4,
            parameter_end: None,
        }]
        .into(),
        quickening_sites: vec![
            Vec::<std::cell::RefCell<crate::quickening::QuickeningSite<4>>>::new()
                .into_boxed_slice(),
        ]
        .into(),
        operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
        catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
    });
    let range = super::CodeRange::new(super::CodeId(0), 0, 8).expect("backedge range");
    let code = store.code(range).expect("backedge code");
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let operations = entries
        .iter()
        .map(|entry| entry.instruction.opcode)
        .collect::<Vec<_>>();
    let control = cfg
        .region_plan_with_terminal_exits(&entries, 0, &operations)
        .expect("arbitrary backedge CFG");
    assert_eq!(control.internal_backedges(), [crate::stencil_cfg::RegionEdge { from: 3, to: 0 }]);

    let dispatch_key = crate::stencil_select::dispatch_region_key();
    let mut policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    policy.fused_regions = true;
    let arena = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("backedge arena"),
    ));
    assert!(super::generic_cfg_region_admission(&entries, &cfg, 0, policy, &arena).is_some());

    crate::stencil_policy::with_policy_for_test(policy, || {
        let baseline = super::BaselinePlan::compile_for_test(code, policy);
        let region = baseline.native_region_at(0).expect("generic backedge region");
        assert_eq!(region.borrow().key_for_test(), dispatch_key);
        let environment = crate::environment::Environment::new();
        environment.set(0, crate::value::Value::Number(3.0));
        let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
        let context = crate::vm::current_context_or_default();
        let mut registers = crate::register_file::RegisterFile::with_undefined(4);
        let completion = crate::vm::execute_baseline_code_from(
            code,
            &baseline,
            0,
            &mut registers,
            &context,
            environment.clone(),
        )
        .expect("generic backedge execution")
        .0;
        assert_eq!(
            completion,
            crate::completion::Completion::Return(crate::value::Value::Number(0.0))
        );
        let region = baseline.native_region_at(0).expect("backedge region retained");
        let region = region.borrow();
        assert!(region.last_native_execution());
        assert_eq!(region.load_local_entry_count_for_test(), 5);
        assert_eq!(region.update_entry_count_for_test(), 3);
        assert_eq!(region.branch_entry_count_for_test(), 0);
        assert_eq!(region.truthiness_entry_count_for_test(), 4);
        assert_eq!(region.jump_entry_count_for_test(), 3);
        assert_eq!(region.return_entry_count_for_test(), 1);
        assert_eq!(region.retired_operations(), 16);
        assert_eq!(environment.get(0), crate::value::Value::Number(0.0));
    });
}

#[test]
fn generic_cfg_bridge_consumes_add_const_pool_operand() {
    let instructions = vec![
        crate::ir::Instruction::load_local(0, 0),
        crate::ir::Instruction::add_const(1, 0, 0),
        crate::ir::Instruction::ret(1),
    ];
    let store = std::rc::Rc::new(super::CodeStore {
        instructions: instructions.into(),
        cold: Vec::<super::Op>::new().into(),
        ranges: vec![(0, 3)].into(),
        constants: vec![super::ConstantPool::new(vec![super::Constant::Number(2.25)])].into(),
        metadata: vec![vec![super::InstructionMeta::empty(); 3]].into(),
        layouts: vec![super::FunctionLayout {
            register_count: 2,
            frame_register_count: 2,
            parameter_end: None,
        }]
        .into(),
        quickening_sites: vec![
            Vec::<std::cell::RefCell<crate::quickening::QuickeningSite<4>>>::new()
                .into_boxed_slice(),
        ]
        .into(),
        operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
        catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
    });
    let range = super::CodeRange::new(super::CodeId(0), 0, 3).expect("AddConst range");
    let code = store.code(range).expect("AddConst code");
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let mut policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    policy.fused_regions = true;
    let arena = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("AddConst arena"),
    ));
    assert!(super::generic_cfg_region_admission(&entries, &cfg, 0, policy, &arena).is_some());

    crate::stencil_policy::with_policy_for_test(policy, || {
        let baseline = super::BaselinePlan::compile_for_test(code, policy);
        let region = baseline.native_region_at(0).expect("generic AddConst region");
        assert_eq!(region.borrow().key_for_test(), crate::stencil_select::dispatch_region_key());
        let environment = crate::environment::Environment::new();
        environment.set(0, crate::value::Value::Number(3.5));
        let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
        let context = crate::vm::current_context_or_default();
        let mut registers = crate::register_file::RegisterFile::with_undefined(2);
        let completion = crate::vm::execute_baseline_code_from(
            code,
            &baseline,
            0,
            &mut registers,
            &context,
            environment,
        )
        .expect("generic AddConst execution")
        .0;
        assert_eq!(
            completion,
            crate::completion::Completion::Return(crate::value::Value::Number(5.75))
        );
        let region = baseline.native_region_at(0).expect("region retained");
        let region = region.borrow();
        assert!(region.last_native_execution());
        assert_eq!(region.binary_entry_count_for_test(), 1);
        assert_eq!(region.load_local_entry_count_for_test(), 1);
        assert_eq!(region.retired_operations(), 3);
    });
}

#[test]
fn generic_cfg_bridge_consumes_numeric_update_opcode_leaf() {
    let instructions = vec![
        crate::ir::Instruction::load_const(0, 0),
        crate::ir::Instruction::load_const(2, 1),
        crate::ir::Instruction::binary(crate::ir::Opcode::NumericAdd, 1, 0, 2),
        crate::ir::Instruction::ret(1),
    ];
    let store = std::rc::Rc::new(super::CodeStore {
        instructions: instructions.into(),
        cold: Vec::<super::Op>::new().into(),
        ranges: vec![(0, 4)].into(),
        constants: vec![super::ConstantPool::new(vec![
            super::Constant::Number(2.0),
            // NumericAdd is an update operation: its encoded RHS is not
            // consulted. An undefined RHS proves the generic binary operand
            // path does not accidentally impose a binary-number guard.
            super::Constant::Undefined,
        ])]
        .into(),
        metadata: vec![vec![super::InstructionMeta::empty(); 4]].into(),
        layouts: vec![super::FunctionLayout {
            register_count: 3,
            frame_register_count: 3,
            parameter_end: None,
        }]
        .into(),
        quickening_sites: vec![
            Vec::<std::cell::RefCell<crate::quickening::QuickeningSite<4>>>::new()
                .into_boxed_slice(),
        ]
        .into(),
        operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
        catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
    });
    let range = super::CodeRange::new(super::CodeId(0), 0, 4).expect("numeric update range");
    let code = store.code(range).expect("numeric update code");
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let mut policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    policy.fused_regions = true;
    let arena = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("numeric update arena"),
    ));
    assert!(super::generic_cfg_region_admission(&entries, &cfg, 0, policy, &arena).is_some());

    crate::stencil_policy::with_policy_for_test(policy, || {
        let baseline = super::BaselinePlan::compile_for_test(code, policy);
        let environment = crate::environment::Environment::new();
        let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
        let context = crate::vm::current_context_or_default();
        let mut registers = crate::register_file::RegisterFile::with_undefined(3);
        let completion = crate::vm::execute_baseline_code_from(
            code,
            &baseline,
            0,
            &mut registers,
            &context,
            environment,
        )
        .expect("generic NumericAdd execution")
        .0;
        assert_eq!(
            completion,
            crate::completion::Completion::Return(crate::value::Value::Number(3.0))
        );
        let region = baseline.native_region_at(0).expect("numeric update region");
        let region = region.borrow();
        assert!(region.last_native_execution());
        assert_eq!(region.binary_entry_count_for_test(), 1);
        assert_eq!(region.last_native_view_for_test().map(|view| view.key),
            Some(crate::stencil_select::increment_region_key()));
        assert_eq!(region.retired_operations(), 4);
    });
}

#[test]
fn generic_cfg_bridge_consumes_numeric_decrement_with_dead_rhs() {
    let function = super::FunctionCode::from_ops(vec![
        super::Op::Const {
            dst: 0,
            value: super::Constant::Number(2.0),
        },
        super::Op::Const {
            dst: 2,
            value: super::Constant::Undefined,
        },
        super::Op::Binary {
            dst: 1,
            operator: crate::ops::BinaryOp::NumericSubtract,
            lhs: 0,
            rhs: 2,
        },
        super::Op::Return { src: 1 },
    ]);
    let code = function.code().expect("numeric decrement code");
    let mut policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    policy.fused_regions = true;
    crate::stencil_policy::with_policy_for_test(policy, || {
        let baseline = super::BaselinePlan::compile_for_test(code, policy);
        let environment = crate::environment::Environment::new();
        let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
        let mut registers = crate::register_file::RegisterFile::with_undefined(3);
        let completion = crate::vm::execute_baseline_code_from(
            code,
            &baseline,
            0,
            &mut registers,
            &crate::vm::current_context_or_default(),
            environment,
        )
        .expect("generic NumericSubtract execution")
        .0;
        assert_eq!(
            completion,
            crate::completion::Completion::Return(crate::value::Value::Number(1.0))
        );
        let region = baseline
            .native_region_at(0)
            .expect("numeric decrement region");
        let region = region.borrow();
        assert!(region.last_native_execution());
        assert_eq!(region.binary_entry_count_for_test(), 1);
        assert_eq!(
            region.last_native_view_for_test().map(|view| view.key),
            Some(crate::stencil_select::decrement_region_key())
        );
    });
}

#[test]
fn generic_cfg_bridge_consumes_generic_binary_numeric_decrement() {
    let instructions = vec![
        crate::ir::Instruction::load_const(0, 0),
        crate::ir::Instruction::load_const(2, 1),
        crate::ir::Instruction {
            opcode: crate::ir::Opcode::Binary,
            flags: crate::ir::compact_binary_id(crate::ops::BinaryOp::NumericSubtract),
            a: 1,
            b: 0,
            c: 2,
        },
        crate::ir::Instruction::ret(1),
    ];
    let store = std::rc::Rc::new(super::CodeStore {
        instructions: instructions.into(),
        cold: Vec::<super::Op>::new().into(),
        ranges: vec![(0, 4)].into(),
        constants: vec![super::ConstantPool::new(vec![
            super::Constant::Number(2.0),
            super::Constant::Undefined,
        ])]
        .into(),
        metadata: vec![vec![super::InstructionMeta::empty(); 4]].into(),
        layouts: vec![super::FunctionLayout {
            register_count: 3,
            frame_register_count: 3,
            parameter_end: None,
        }]
        .into(),
        quickening_sites: vec![
            Vec::<std::cell::RefCell<crate::quickening::QuickeningSite<4>>>::new()
                .into_boxed_slice(),
        ]
        .into(),
        operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
        catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
    });
    let range = super::CodeRange::new(super::CodeId(0), 0, 4).expect("generic binary range");
    let code = store.code(range).expect("generic binary code");
    let mut policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    policy.fused_regions = true;
    crate::stencil_policy::with_policy_for_test(policy, || {
        let baseline = super::BaselinePlan::compile_for_test(code, policy);
        let environment = crate::environment::Environment::new();
        let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
        let mut registers = crate::register_file::RegisterFile::with_undefined(3);
        let completion = crate::vm::execute_baseline_code_from(
            code,
            &baseline,
            0,
            &mut registers,
            &crate::vm::current_context_or_default(),
            environment,
        )
        .expect("generic Binary NumericSubtract execution")
        .0;
        assert_eq!(
            completion,
            crate::completion::Completion::Return(crate::value::Value::Number(1.0))
        );
        let region = baseline
            .native_region_at(0)
            .expect("generic binary decrement region");
        let region = region.borrow();
        assert!(region.last_native_execution());
        assert_eq!(region.binary_entry_count_for_test(), 1);
        assert_eq!(
            region.last_native_view_for_test().map(|view| view.key),
            Some(crate::stencil_select::decrement_region_key())
        );
    });
}

#[test]
fn generic_cfg_bridge_consumes_proven_local_move() {
    let instructions = vec![
        crate::ir::Instruction::move_local(2, 0, 1),
        crate::ir::Instruction::ret(2),
    ];
    let store = std::rc::Rc::new(super::CodeStore {
        instructions: instructions.into(),
        cold: Vec::<super::Op>::new().into(),
        ranges: vec![(0, 2)].into(),
        constants: vec![super::ConstantPool::empty()].into(),
        metadata: vec![vec![super::InstructionMeta::empty(); 2]].into(),
        layouts: vec![super::FunctionLayout {
            register_count: 3,
            frame_register_count: 3,
            parameter_end: None,
        }]
        .into(),
        quickening_sites: vec![
            Vec::<std::cell::RefCell<crate::quickening::QuickeningSite<4>>>::new()
                .into_boxed_slice(),
        ]
        .into(),
        operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
        catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
    });
    let range = super::CodeRange::new(super::CodeId(0), 0, 2).expect("local-move range");
    let code = store.code(range).expect("local-move code");
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let mut policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    policy.fused_regions = true;
    let arena = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("local-move arena"),
    ));
    let operations = entries
        .iter()
        .map(|entry| entry.instruction.opcode)
        .collect::<Vec<_>>();
    assert!(
        cfg.region_plan_with_terminal_exits(&entries, 0, &operations)
            .is_some()
    );
    assert!(super::generic_bridge_candidate(entries[0].instruction));
    assert!(super::generic_cfg_region_admission(&entries, &cfg, 0, policy, &arena).is_some());
    assert!(
        super::region_admission_with_code(code, &entries, &cfg, 0, policy, &arena).is_some()
    );

    crate::stencil_policy::with_policy_for_test(policy, || {
        let baseline = super::BaselinePlan::compile_for_test(code, policy);
        let environment = crate::environment::Environment::new();
        environment.set(0, crate::value::Value::Number(17.0));
        environment.set(1, crate::value::Value::Number(0.0));
        let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
        let mut registers = crate::register_file::RegisterFile::with_undefined(3);
        let completion = crate::vm::execute_baseline_code_from(
            code,
            &baseline,
            0,
            &mut registers,
            &crate::vm::current_context_or_default(),
            environment.clone(),
        )
        .expect("local-move execution")
        .0;
        assert_eq!(
            completion,
            crate::completion::Completion::Return(crate::value::Value::Number(17.0))
        );
        assert_eq!(environment.get_number(1), Some(17.0));
        let region = baseline.native_region_at(0).expect("local-move region");
        let region = region.borrow();
        assert!(region.last_native_execution());
        assert_eq!(region.move_entry_count_for_test(), 1);
        assert_eq!(region.retired_operations(), 2);
    });
}

#[test]
fn generic_cfg_bridge_rejects_non_numeric_add_const_at_admission() {
    let instructions = vec![
        crate::ir::Instruction::add_const(0, 1, 0),
        crate::ir::Instruction::ret(0),
    ];
    let store = std::rc::Rc::new(super::CodeStore {
        instructions: instructions.into(),
        cold: Vec::<super::Op>::new().into(),
        ranges: vec![(0, 2)].into(),
        constants: vec![super::ConstantPool::new(vec![super::Constant::String(
            "not numeric".into(),
        )])]
        .into(),
        metadata: vec![vec![super::InstructionMeta::empty(); 2]].into(),
        layouts: vec![super::FunctionLayout {
            register_count: 2,
            frame_register_count: 2,
            parameter_end: None,
        }]
        .into(),
        quickening_sites: vec![
            Vec::<std::cell::RefCell<crate::quickening::QuickeningSite<4>>>::new()
                .into_boxed_slice(),
        ]
        .into(),
        operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
        catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
    });
    let range = super::CodeRange::new(super::CodeId(0), 0, 2).expect("string AddConst range");
    let code = store.code(range).expect("string AddConst code");
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let arena = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("string AddConst arena"),
    ));
    assert!(
        super::region_admission_with_code(code, &entries, &cfg, 0, policy, &arena).is_none(),
        "non-numeric AddConst must not publish a generic bridge"
    );
    assert!(!super::generic_bridge_candidate_at(
        code,
        crate::ir::Instruction::load_const(0, 0),
    ));
}

#[test]
fn generic_bridge_constant_pool_guard_matches_tagged_word_contract() {
    // `LoadConst` is a candidate only when its range-owned constant can be
    // represented by the immutable tagged-word leaf.  Heap-owning strings
    // and BigInts must remain canonical even when a later Return makes the
    // surrounding bridge useful; primitive words are eligible.
    let cases = [
        (crate::ops::Constant::Number(1.5), true),
        (crate::ops::Constant::Boolean(true), true),
        (crate::ops::Constant::Null, true),
        (crate::ops::Constant::Undefined, true),
        (crate::ops::Constant::String("heap-owned".into()), false),
        (crate::ops::Constant::BigInt("123".into()), false),
    ];
    for (constant, expected) in cases {
        let executable = crate::machine::ExecutableCode::from_ops(vec![
            crate::ops::Op::Const { dst: 0, value: constant },
            crate::ops::Op::Return { src: 0 },
        ]);
        let code = executable.code();
        let load = code.instruction(0).expect("constant lowering emits LoadConst");
        assert_eq!(load.opcode, crate::ir::Opcode::LoadConst);
        assert_eq!(
            super::generic_bridge_candidate_at(code, load),
            expected,
            "LoadConst candidate must match its tagged-word representation"
        );
    }
}

#[test]
fn generic_cfg_bridge_consumes_bitwise_leaf() {
    let instructions = vec![
        crate::ir::Instruction::load_local(0, 0),
        crate::ir::Instruction::load_local(1, 1),
        crate::ir::Instruction {
            opcode: crate::ir::Opcode::BitwiseAnd,
            flags: 0,
            a: 2,
            b: 0,
            c: 1,
        },
        crate::ir::Instruction::ret(2),
    ];
    let store = std::rc::Rc::new(super::CodeStore {
        instructions: instructions.into(),
        cold: Vec::<super::Op>::new().into(),
        ranges: vec![(0, 4)].into(),
        constants: vec![super::ConstantPool::new(Vec::new())].into(),
        metadata: vec![vec![super::InstructionMeta::empty(); 4]].into(),
        layouts: vec![super::FunctionLayout {
            register_count: 3,
            frame_register_count: 3,
            parameter_end: None,
        }]
        .into(),
        quickening_sites: vec![
            Vec::<std::cell::RefCell<crate::quickening::QuickeningSite<4>>>::new()
                .into_boxed_slice(),
        ]
        .into(),
        operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
        catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
    });
    let range = super::CodeRange::new(super::CodeId(0), 0, 4).expect("bitwise range");
    let code = store.code(range).expect("bitwise code");
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let mut policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    policy.fused_regions = true;
    let arena = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("bitwise arena"),
    ));
    assert!(super::generic_cfg_region_admission(&entries, &cfg, 0, policy, &arena).is_some());

    crate::stencil_policy::with_policy_for_test(policy, || {
        let baseline = super::BaselinePlan::compile_for_test(code, policy);
        let environment = crate::environment::Environment::new();
        environment.set(0, crate::value::Value::Number(5.0));
        environment.set(1, crate::value::Value::Number(3.0));
        let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
        let context = crate::vm::current_context_or_default();
        let mut registers = crate::register_file::RegisterFile::with_undefined(3);
        let completion = crate::vm::execute_baseline_code_from(
            code,
            &baseline,
            0,
            &mut registers,
            &context,
            environment,
        )
        .expect("generic bitwise execution")
        .0;
        assert_eq!(
            completion,
            crate::completion::Completion::Return(crate::value::Value::Number(1.0))
        );
        let region = baseline.native_region_at(0).expect("bitwise region retained");
        let region = region.borrow();
        assert!(region.last_native_execution());
        assert_eq!(region.binary_entry_count_for_test(), 1);
        assert_eq!(region.load_local_entry_count_for_test(), 2);
        assert_eq!(region.retired_operations(), 4);
    });
}

#[test]
fn generic_cfg_bridge_consumes_flagged_binary_arithmetic_leaf() {
    let instructions = vec![
        crate::ir::Instruction::load_local(0, 0),
        crate::ir::Instruction::load_local(1, 1),
        crate::ir::Instruction {
            opcode: crate::ir::Opcode::Binary,
            flags: crate::ir::compact_binary_id(crate::ops::BinaryOp::Add),
            a: 2,
            b: 0,
            c: 1,
        },
        crate::ir::Instruction::ret(2),
    ];
    let store = std::rc::Rc::new(super::CodeStore {
        instructions: instructions.into(),
        cold: Vec::<super::Op>::new().into(),
        ranges: vec![(0, 4)].into(),
        constants: vec![super::ConstantPool::new(Vec::new())].into(),
        metadata: vec![vec![super::InstructionMeta::empty(); 4]].into(),
        layouts: vec![super::FunctionLayout {
            register_count: 3,
            frame_register_count: 3,
            parameter_end: None,
        }]
        .into(),
        quickening_sites: vec![
            Vec::<std::cell::RefCell<crate::quickening::QuickeningSite<4>>>::new()
                .into_boxed_slice(),
        ]
        .into(),
        operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
        catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
    });
    let range = super::CodeRange::new(super::CodeId(0), 0, 4).expect("binary range");
    let code = store.code(range).expect("binary code");
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let mut policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    policy.fused_regions = true;
    let arena = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("binary arena"),
    ));
    assert!(super::generic_bridge_candidate(entries[2].instruction));
    assert!(super::generic_cfg_region_admission(&entries, &cfg, 0, policy, &arena).is_some());

    crate::stencil_policy::with_policy_for_test(policy, || {
        let baseline = super::BaselinePlan::compile_for_test(code, policy);
        let environment = crate::environment::Environment::new();
        environment.set(0, crate::value::Value::Number(2.0));
        environment.set(1, crate::value::Value::Number(3.0));
        let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
        let context = crate::vm::current_context_or_default();
        let mut registers = crate::register_file::RegisterFile::with_undefined(3);
        let completion = crate::vm::execute_baseline_code_from(
            code,
            &baseline,
            0,
            &mut registers,
            &context,
            environment,
        )
        .expect("generic binary execution")
        .0;
        assert_eq!(
            completion,
            crate::completion::Completion::Return(crate::value::Value::Number(5.0))
        );
        let region = baseline.native_region_at(0).expect("binary region retained");
        let region = region.borrow();
        assert!(region.last_native_execution());
        assert_eq!(region.binary_entry_count_for_test(), 1);
        assert_eq!(region.load_local_entry_count_for_test(), 2);
        assert_eq!(region.retired_operations(), 4);
    });
}

#[test]
fn generic_cfg_bridge_consumes_flagged_binary_compare_branch() {
    let instructions = vec![
        crate::ir::Instruction::load_local(0, 0),
        crate::ir::Instruction::load_local(1, 1),
        crate::ir::Instruction {
            opcode: crate::ir::Opcode::Binary,
            flags: crate::ir::compact_binary_id(crate::ops::BinaryOp::LessThan),
            a: 2,
            b: 0,
            c: 1,
        },
        crate::ir::Instruction::jump_if_false(2, 6),
        crate::ir::Instruction::load_const(3, 0),
        crate::ir::Instruction::ret(3),
        crate::ir::Instruction::load_const(3, 1),
        crate::ir::Instruction::ret(3),
    ];
    let store = std::rc::Rc::new(super::CodeStore {
        instructions: instructions.into(),
        cold: Vec::<super::Op>::new().into(),
        ranges: vec![(0, 8)].into(),
        constants: vec![super::ConstantPool::new(vec![
            super::Constant::Number(10.0),
            super::Constant::Number(20.0),
        ])]
        .into(),
        metadata: vec![vec![super::InstructionMeta::empty(); 8]].into(),
        layouts: vec![super::FunctionLayout {
            register_count: 4,
            frame_register_count: 4,
            parameter_end: None,
        }]
        .into(),
        quickening_sites: vec![
            Vec::<std::cell::RefCell<crate::quickening::QuickeningSite<4>>>::new()
                .into_boxed_slice(),
        ]
        .into(),
        operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
        catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
    });
    let code = store
        .code(super::CodeRange::new(super::CodeId(0), 0, 8).expect("compare branch range"))
        .expect("compare branch code");
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let mut policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    policy.fused_regions = true;
    let arena = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("compare branch arena"),
    ));
    assert!(super::generic_cfg_region_admission(&entries, &cfg, 0, policy, &arena).is_some());

    crate::stencil_policy::with_policy_for_test(policy, || {
        for (left, right, expected) in [(1.0, 2.0, 10.0), (3.0, 2.0, 20.0)] {
            let baseline = super::BaselinePlan::compile_for_test(code, policy);
            let environment = crate::environment::Environment::new();
            environment.set(0, crate::value::Value::Number(left));
            environment.set(1, crate::value::Value::Number(right));
            let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
            let mut registers = crate::register_file::RegisterFile::with_undefined(4);
            let completion = crate::vm::execute_baseline_code_from(
                code,
                &baseline,
                0,
                &mut registers,
                &crate::vm::current_context_or_default(),
                environment,
            )
            .expect("generic compare branch execution")
            .0;
            assert_eq!(
                completion,
                crate::completion::Completion::Return(crate::value::Value::Number(expected))
            );
            let region = baseline.native_region_at(0).expect("compare branch region retained");
            let region = region.borrow();
            assert!(region.last_native_execution());
            assert_eq!(region.binary_entry_count_for_test(), 1);
            assert!(region.branch_entry_count_for_test() >= 1);
            assert_eq!(region.retired_operations(), 6);
        }
    });
}

#[test]
fn generic_cfg_bridge_consumes_tagged_strict_equality_leaf() {
    let instructions = vec![
        crate::ir::Instruction::load_local(0, 0),
        crate::ir::Instruction::load_local(1, 1),
        crate::ir::Instruction {
            opcode: crate::ir::Opcode::StrictEqual,
            flags: 0,
            a: 2,
            b: 0,
            c: 1,
        },
        crate::ir::Instruction::ret(2),
    ];
    let store = std::rc::Rc::new(super::CodeStore {
        instructions: instructions.into(),
        cold: Vec::<super::Op>::new().into(),
        ranges: vec![(0, 4)].into(),
        constants: vec![super::ConstantPool::new(Vec::new())].into(),
        metadata: vec![vec![super::InstructionMeta::empty(); 4]].into(),
        layouts: vec![super::FunctionLayout {
            register_count: 3,
            frame_register_count: 3,
            parameter_end: None,
        }]
        .into(),
        quickening_sites: vec![
            Vec::<std::cell::RefCell<crate::quickening::QuickeningSite<4>>>::new()
                .into_boxed_slice(),
        ]
        .into(),
        operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
        catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
    });
    let range = super::CodeRange::new(super::CodeId(0), 0, 4).expect("strict equality range");
    let code = store.code(range).expect("strict equality code");
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let mut policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    policy.fused_regions = true;
    let arena = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("strict equality arena"),
    ));
    assert!(super::generic_cfg_region_admission(&entries, &cfg, 0, policy, &arena).is_some());

    crate::stencil_policy::with_policy_for_test(policy, || {
        let baseline = super::BaselinePlan::compile_for_test(code, policy);
        let environment = crate::environment::Environment::new();
        environment.set(0, crate::value::Value::Boolean(true));
        environment.set(1, crate::value::Value::Boolean(true));
        let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
        let context = crate::vm::current_context_or_default();
        let mut registers = crate::register_file::RegisterFile::with_undefined(3);
        let completion = crate::vm::execute_baseline_code_from(
            code,
            &baseline,
            0,
            &mut registers,
            &context,
            environment,
        )
        .expect("generic strict equality execution")
        .0;
        assert_eq!(
            completion,
            crate::completion::Completion::Return(crate::value::Value::Boolean(true))
        );
        let region = baseline
            .native_region_at(0)
            .expect("strict equality region retained");
        let region = region.borrow();
        assert!(region.last_native_execution());
        assert_eq!(region.binary_entry_count_for_test(), 1);
        assert_eq!(region.load_local_entry_count_for_test(), 2);
        assert_eq!(region.retired_operations(), 4);
    });
}

#[test]
fn generic_cfg_bridge_consumes_update_local_leaf() {
    let instructions = vec![
        crate::ir::Instruction::load_local(0, 0),
        crate::ir::Instruction::update_local(1, 2, 0, false),
        crate::ir::Instruction::ret(2),
    ];
    let store = std::rc::Rc::new(super::CodeStore {
        instructions: instructions.into(),
        cold: Vec::<super::Op>::new().into(),
        ranges: vec![(0, 3)].into(),
        constants: vec![super::ConstantPool::empty()].into(),
        metadata: vec![vec![super::InstructionMeta::empty(); 3]].into(),
        layouts: vec![super::FunctionLayout {
            register_count: 3,
            frame_register_count: 3,
            parameter_end: None,
        }]
        .into(),
        quickening_sites: vec![
            Vec::<std::cell::RefCell<crate::quickening::QuickeningSite<4>>>::new()
                .into_boxed_slice(),
        ]
        .into(),
        operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
        catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
    });
    let range = super::CodeRange::new(super::CodeId(0), 0, 3).expect("update range");
    let code = store.code(range).expect("update code");
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let mut policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    policy.fused_regions = true;
    let arena = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("update arena"),
    ));
    assert!(super::generic_cfg_region_admission(&entries, &cfg, 0, policy, &arena).is_some());

    crate::stencil_policy::with_policy_for_test(policy, || {
        let baseline = super::BaselinePlan::compile_for_test(code, policy);
        let region = baseline.native_region_at(0).expect("generic update region");
        assert_eq!(
            region.borrow().key_for_test(),
            crate::stencil_select::dispatch_region_key()
        );
        let environment = crate::environment::Environment::new();
        environment.set(0, crate::value::Value::Number(3.0));
        let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
        let context = crate::vm::current_context_or_default();
        let mut registers = crate::register_file::RegisterFile::with_undefined(3);
        let completion = crate::vm::execute_baseline_code_from(
            code,
            &baseline,
            0,
            &mut registers,
            &context,
            environment.clone(),
        )
        .expect("generic update execution")
        .0;
        assert_eq!(
            completion,
            crate::completion::Completion::Return(crate::value::Value::Number(4.0))
        );
        let region = baseline.native_region_at(0).expect("update region retained");
        let region = region.borrow();
        assert!(region.last_native_execution());
        assert_eq!(region.update_entry_count_for_test(), 1);
        assert_eq!(region.load_local_entry_count_for_test(), 1);
        assert_eq!(region.retired_operations(), 3);
        assert_eq!(environment.get(0), crate::value::Value::Number(4.0));
    });
}

#[test]
fn control_only_bridge_consumes_numeric_truthiness_leaf_when_enabled() {
    let executable = super::ExecutableCode::from_ops(vec![
        super::Op::Branch {
            condition: 0,
            then_ops: super::FunctionCode::pending(vec![super::Op::Move { dst: 3, src: 1 }]),
            else_ops: super::FunctionCode::pending(vec![super::Op::Move { dst: 3, src: 2 }]),
        },
        super::Op::Return { src: 3 },
    ]);
    let code = executable.code();
    let key = crate::stencil_select::branch_glue_region_key();
    let record = crate::stencil_select::select_region(key).expect("branch row");
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let control = cfg
        .region_plan(&entries, 0, record.operations)
        .expect("verified branch CFG");
    let context = crate::vm::current_context_or_default();
    crate::stencil_policy::with_policy_for_test(
        crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
        || {
            for (condition, expected) in [(0.0, 22.0), (3.0, 11.0)] {
                let arena = std::rc::Rc::new(std::cell::RefCell::new(
                    crate::stencil_arena::SharedStencilSlab::new(4096).expect("region arena"),
                ));
                let mut region = super::NativeRegionPlan::new_inner(
                    key,
                    true,
                    arena,
                    Some(control.clone()),
                )
                .expect("branch region plan");
                let mut registers = crate::register_file::RegisterFile::from_values(vec![
                    crate::value::Value::Number(condition),
                    crate::value::Value::Number(11.0),
                    crate::value::Value::Number(22.0),
                    crate::value::Value::Undefined,
                ]);
                let transition = region
                    .execute(code, 0, &mut registers, &context, None)
                    .expect("branch region execution");
                assert_eq!(
                    transition.completion,
                    Some(crate::completion::Completion::Return(
                        crate::value::Value::Number(expected),
                    ))
                );
                assert!(region.last_native_execution());
                assert_eq!(region.truthiness_entry_count_for_test(), 1);
            }
        },
    );
}

#[test]
fn bridge_region_commits_proven_store_local_word_before_return() {
    let executable = super::ExecutableCode::from_ops(vec![
        super::Op::LoadLocal { dst: 1, slot: 0 },
        super::Op::StoreLocal { slot: 1, src: 1 },
        super::Op::Return { src: 1 },
    ]);
    let code = executable.code();
    let key = crate::stencil_select::store_glue_region_key();
    let record = crate::stencil_select::select_region(key).expect("store row");
    assert_eq!(code.len(), record.operations.len());
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let control = cfg
        .region_plan(&entries, 0, record.operations)
        .expect("verified store CFG");
    let context = crate::vm::current_context_or_default();
    let (transition, native, loads, stores, retired, stored_value) =
        crate::stencil_policy::with_policy_for_test(
        crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
        || {
            let environment = crate::environment::Environment::new();
            environment.set(0, crate::value::Value::Number(41.0));
            environment.set(1, crate::value::Value::Number(0.0));
            let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
            let arena = std::rc::Rc::new(std::cell::RefCell::new(
                crate::stencil_arena::SharedStencilSlab::new(4096).expect("region arena"),
            ));
            let mut region = super::NativeRegionPlan::new_inner(
                key,
                true,
                arena,
                Some(control.clone()),
            )
            .expect("store region plan");
            let mut registers = crate::register_file::RegisterFile::with_undefined(4);
            let transition = region
                .execute(code, 0, &mut registers, &context, Some(&environment))
                .expect("store region execution");
            (
                transition,
                region.last_native_execution(),
                region.load_local_entry_count_for_test(),
                region.store_local_entry_count_for_test(),
                region.retired_operations(),
                environment.get(1),
            )
        },
    );
    assert_eq!(
        transition.completion,
        Some(crate::completion::Completion::Return(
            crate::value::Value::Number(41.0),
        ))
    );
    assert_eq!(native, true);
    assert_eq!(loads, 1);
    assert_eq!(stores, 1);
    assert_eq!(retired, 3);
    assert_eq!(stored_value, crate::value::Value::Number(41.0));
}

#[test]
fn bridge_region_consumes_generated_increment_leaf() {
    let key = crate::stencil_select::inc_glue_region_key();
    let record = crate::stencil_select::select_region(key).expect("increment row");
    let context = crate::vm::current_context_or_default();
    for (decrement, expected) in [(false, 42.0), (true, 40.0)] {
        let mut arena = super::CodeArena::new();
        let range = arena.append(vec![
            super::Op::Move { dst: 2, src: 1 },
            super::Op::Return { src: 2 },
        ]);
        arena.instructions[0] = crate::ir::Instruction::inc_i(2, 1, decrement);
        let store = arena.freeze();
        let code = store.code(range).expect("increment code range");
        assert_eq!(code.len(), record.operations.len());
        let entries = super::baseline_entries(code);
        let operand_windows = (0..entries.len())
            .map(|pc| code.operand_window_at(pc))
            .collect::<Vec<_>>();
        let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
        let control = cfg
            .region_plan(&entries, 0, record.operations)
            .expect("verified increment CFG");
        let (transition, native, binaries, view_key, retired) =
            crate::stencil_policy::with_policy_for_test(
                crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
                || {
                    let environment = crate::environment::Environment::new();
                    environment.set(0, crate::value::Value::Number(41.0));
                    let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
                    let arena = std::rc::Rc::new(std::cell::RefCell::new(
                        crate::stencil_arena::SharedStencilSlab::new(4096)
                            .expect("region arena"),
                    ));
                    let mut region = super::NativeRegionPlan::new_inner(
                        key,
                        true,
                        arena,
                        Some(control.clone()),
                    )
                    .expect("increment region plan");
                    let mut registers = crate::register_file::RegisterFile::with_undefined(4);
                    registers.write_number(1, 41.0);
                    let transition = region
                        .execute(code, 0, &mut registers, &context, Some(&environment))
                        .expect("increment region execution");
                    (
                        transition,
                        region.last_native_execution(),
                        region.binary_entry_count_for_test(),
                        region.last_native_view_for_test().map(|view| view.key),
                        region.retired_operations(),
                    )
                },
            );
        assert_eq!(
            transition.completion,
            Some(crate::completion::Completion::Return(
                crate::value::Value::Number(expected),
            ))
        );
        assert!(native);
        assert_eq!(binaries, 1);
        assert_eq!(
            view_key,
            Some(if decrement {
                crate::stencil_select::decrement_region_key()
            } else {
                crate::stencil_select::increment_region_key()
            })
        );
        assert_eq!(retired, 2);
    }
}

#[test]
fn bridge_region_consumes_generated_numeric_unary_leaf() {
    let key = crate::stencil_select::unary_glue_region_key();
    let record = crate::stencil_select::select_region(key).expect("unary row");
    let context = crate::vm::current_context_or_default();
    for (operator, input, expected) in [
        (crate::ops::UnaryOp::Plus, -0.0, -0.0),
        (crate::ops::UnaryOp::Minus, 3.25, -3.25),
        (crate::ops::UnaryOp::BitwiseNot, 3.75, -4.0),
    ] {
        let mut arena = super::CodeArena::new();
        let range = arena.append(vec![
            super::Op::Move { dst: 0, src: 1 },
            super::Op::Return { src: 0 },
        ]);
        arena.instructions[0] = crate::ir::Instruction::unary_operator(0, operator, 1);
        let store = arena.freeze();
        let code = store.code(range).expect("unary code range");
        assert_eq!(code.len(), record.operations.len());
        let entries = super::baseline_entries(code);
        let operand_windows = (0..entries.len())
            .map(|pc| code.operand_window_at(pc))
            .collect::<Vec<_>>();
        let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
        let control = cfg
            .region_plan(&entries, 0, record.operations)
            .expect("verified unary CFG");
        let (transition, native, entries, retired) = crate::stencil_policy::with_policy_for_test(
            crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
            || {
                let arena = std::rc::Rc::new(std::cell::RefCell::new(
                    crate::stencil_arena::SharedStencilSlab::new(4096).expect("region arena"),
                ));
                let mut region = super::NativeRegionPlan::new_inner(
                    key,
                    true,
                    arena,
                    Some(control.clone()),
                )
                .expect("unary region plan");
                let mut registers = crate::register_file::RegisterFile::with_undefined(2);
                registers.write_number(1, input);
                let transition = region
                    .execute(code, 0, &mut registers, &context, None)
                    .expect("unary region execution");
                (
                    transition,
                    region.last_native_execution(),
                    region.unary_entry_count_for_test(),
                    region.retired_operations(),
                )
            },
        );
        assert_eq!(
            transition.completion,
            Some(crate::completion::Completion::Return(
                crate::value::Value::Number(expected),
            ))
        );
        assert!(native);
        assert_eq!(entries, 1);
        assert_eq!(retired, 2);
    }
}

#[test]
fn bridge_region_unary_plus_falls_back_for_non_number() {
    let key = crate::stencil_select::unary_glue_region_key();
    let mut arena = super::CodeArena::new();
    let range = arena.append(vec![
        super::Op::Move { dst: 0, src: 1 },
        super::Op::Return { src: 0 },
    ]);
    arena.instructions[0] = crate::ir::Instruction::unary_operator(
        0,
        crate::ops::UnaryOp::Plus,
        1,
    );
    let store = arena.freeze();
    let code = store.code(range).expect("unary plus code range");
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let control = cfg
        .region_plan(&entries, 0, crate::stencil_select::select_region(key).unwrap().operations)
        .expect("verified unary plus CFG");
    let (transition, native, unary_entries) = crate::stencil_policy::with_policy_for_test(
        crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
        || {
            let arena = std::rc::Rc::new(std::cell::RefCell::new(
                crate::stencil_arena::SharedStencilSlab::new(4096).expect("region arena"),
            ));
            let mut region = super::NativeRegionPlan::new_inner(key, true, arena, Some(control))
                .expect("unary plus region plan");
            let mut registers = crate::register_file::RegisterFile::from_values(vec![
                crate::value::Value::Undefined,
                crate::value::Value::String("4".into()),
            ]);
            let transition = region
                .execute(code, 0, &mut registers, &crate::vm::current_context_or_default(), None)
                .expect("unary plus fallback");
            (
                transition,
                region.last_native_execution(),
                region.unary_entry_count_for_test(),
            )
        },
    );
    assert_eq!(
        transition.completion,
        Some(crate::completion::Completion::Return(
            crate::value::Value::Number(4.0),
        ))
    );
    assert!(!native);
    assert_eq!(unary_entries, 0);
}

#[test]
fn bridge_region_consumes_generated_void_constant_leaf() {
    let key = crate::stencil_select::unary_glue_region_key();
    let record = crate::stencil_select::select_region(key).expect("unary row");
    let mut arena = super::CodeArena::new();
    let range = arena.append(vec![
        super::Op::Move { dst: 0, src: 1 },
        super::Op::Return { src: 0 },
    ]);
    arena.instructions[0] = crate::ir::Instruction::unary_operator(
        0,
        crate::ops::UnaryOp::Void,
        1,
    );
    let store = arena.freeze();
    let code = store.code(range).expect("void code range");
    assert_eq!(code.len(), record.operations.len());
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let control = cfg
        .region_plan(&entries, 0, record.operations)
        .expect("verified void CFG");
    let (transition, native, constants, retired) = crate::stencil_policy::with_policy_for_test(
        crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
        || {
            let arena = std::rc::Rc::new(std::cell::RefCell::new(
                crate::stencil_arena::SharedStencilSlab::new(4096).expect("region arena"),
            ));
            let mut region = super::NativeRegionPlan::new_inner(
                key,
                true,
                arena,
                Some(control),
            )
            .expect("void region plan");
            let mut registers = crate::register_file::RegisterFile::from_values(vec![
                crate::value::Value::Undefined,
                crate::value::Value::Number(3.5),
            ]);
            let transition = region
                .execute(code, 0, &mut registers, &crate::vm::current_context_or_default(), None)
                .expect("void region execution");
            (
                transition,
                region.last_native_execution(),
                region.constant_entry_count_for_test(),
                region.retired_operations(),
            )
        },
    );
    assert_eq!(
        transition.completion,
        Some(crate::completion::Completion::Return(
            crate::value::Value::Undefined,
        ))
    );
    assert!(native);
    assert_eq!(constants, 1);
    assert_eq!(retired, 2);
}

#[test]
fn bridge_region_consumes_generated_delete_constant_leaf() {
    let key = crate::stencil_select::unary_glue_region_key();
    let mut arena = super::CodeArena::new();
    let range = arena.append(vec![
        super::Op::Move { dst: 0, src: 1 },
        super::Op::Return { src: 0 },
    ]);
    arena.instructions[0] = crate::ir::Instruction::unary_operator(
        0,
        crate::ops::UnaryOp::Delete,
        1,
    );
    let store = arena.freeze();
    let code = store.code(range).expect("delete code range");
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let operations = crate::stencil_select::select_region(key).unwrap().operations;
    let control = cfg
        .region_plan(&entries, 0, operations)
        .expect("verified delete CFG");
    let (transition, native, constants) = crate::stencil_policy::with_policy_for_test(
        crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
        || {
            let arena = std::rc::Rc::new(std::cell::RefCell::new(
                crate::stencil_arena::SharedStencilSlab::new(4096).expect("region arena"),
            ));
            let mut region = super::NativeRegionPlan::new_inner(key, true, arena, Some(control))
                .expect("delete region plan");
            let mut registers = crate::register_file::RegisterFile::from_values(vec![
                crate::value::Value::Undefined,
                crate::value::Value::Null,
            ]);
            let transition = region
                .execute(code, 0, &mut registers, &crate::vm::current_context_or_default(), None)
                .expect("delete region execution");
            (
                transition,
                region.last_native_execution(),
                region.constant_entry_count_for_test(),
            )
        },
    );
    assert_eq!(
        transition.completion,
        Some(crate::completion::Completion::Return(
            crate::value::Value::Boolean(true),
        ))
    );
    assert!(native);
    assert_eq!(constants, 1);
}

#[test]
fn bridge_region_consumes_generated_nullish_word_leaf() {
    let key = crate::stencil_select::unary_glue_region_key();
    let record = crate::stencil_select::select_region(key).expect("unary row");
    let context = crate::vm::current_context_or_default();
    for (input, expected) in [
        (crate::value::Value::Null, true),
        (crate::value::Value::Undefined, true),
        (crate::value::Value::Number(0.0), false),
    ] {
        let mut arena = super::CodeArena::new();
        let range = arena.append(vec![
            super::Op::Move { dst: 0, src: 1 },
            super::Op::Return { src: 0 },
        ]);
        arena.instructions[0] = crate::ir::Instruction::unary_operator(
            0,
            crate::ops::UnaryOp::IsNullish,
            1,
        );
        let store = arena.freeze();
        let code = store.code(range).expect("nullish code range");
        assert_eq!(code.len(), record.operations.len());
        let entries = super::baseline_entries(code);
        let operand_windows = (0..entries.len())
            .map(|pc| code.operand_window_at(pc))
            .collect::<Vec<_>>();
        let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
        let control = cfg
            .region_plan(&entries, 0, record.operations)
            .expect("verified nullish CFG");
        let (transition, native, entries, retired) = crate::stencil_policy::with_policy_for_test(
            crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
            || {
                let arena = std::rc::Rc::new(std::cell::RefCell::new(
                    crate::stencil_arena::SharedStencilSlab::new(4096).expect("region arena"),
                ));
                let mut region = super::NativeRegionPlan::new_inner(
                    key,
                    true,
                    arena,
                    Some(control.clone()),
                )
                .expect("nullish region plan");
                let mut registers = crate::register_file::RegisterFile::from_values(vec![
                    crate::value::Value::Undefined,
                    input,
                ]);
                let transition = region
                    .execute(code, 0, &mut registers, &context, None)
                    .expect("nullish region execution");
                (
                    transition,
                    region.last_native_execution(),
                    region.nullish_entry_count_for_test(),
                    region.retired_operations(),
                )
            },
        );
        assert_eq!(
            transition.completion,
            Some(crate::completion::Completion::Return(
                crate::value::Value::Boolean(expected),
            ))
        );
        assert!(native);
        assert_eq!(entries, 1);
        assert_eq!(retired, 2);
    }
}

#[test]
fn bridge_region_consumes_checked_local_word_pair_after_guards() {
    let executable = super::ExecutableCode::from_ops(vec![
        super::Op::CheckInitialized {
            slot: 0,
            name: "source".into(),
        },
        super::Op::LoadLocal { dst: 1, slot: 0 },
        super::Op::CheckInitialized {
            slot: 1,
            name: "target".into(),
        },
        super::Op::StoreLocal { slot: 1, src: 1 },
        super::Op::Return { src: 1 },
    ]);
    let code = executable.code();
    let key = crate::stencil_select::checked_store_glue_region_key();
    let record = crate::stencil_select::select_region(key).expect("checked store row");
    assert_eq!(code.len(), record.operations.len());
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let control = cfg
        .region_plan(&entries, 0, record.operations)
        .expect("verified checked store CFG");
    let context = crate::vm::current_context_or_default();
    let (transition, native, loads, stores, retired, stored_value) =
        crate::stencil_policy::with_policy_for_test(
            crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
            || {
                let environment = crate::environment::Environment::new();
                environment.set(0, crate::value::Value::Number(41.0));
                environment.set(1, crate::value::Value::Number(0.0));
                let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
                let arena = std::rc::Rc::new(std::cell::RefCell::new(
                    crate::stencil_arena::SharedStencilSlab::new(4096).expect("region arena"),
                ));
                let mut region = super::NativeRegionPlan::new_inner(
                    key,
                    true,
                    arena,
                    Some(control.clone()),
                )
                .expect("checked store region plan");
                let mut registers = crate::register_file::RegisterFile::with_undefined(4);
                let transition = region
                    .execute(code, 0, &mut registers, &context, Some(&environment))
                    .expect("checked store region execution");
                (
                    transition,
                    region.last_native_execution(),
                    region.load_local_entry_count_for_test(),
                    region.store_local_entry_count_for_test(),
                    region.retired_operations(),
                    environment.get(1),
                )
            },
        );
    assert_eq!(
        transition.completion,
        Some(crate::completion::Completion::Return(
            crate::value::Value::Number(41.0),
        ))
    );
    assert!(native);
    assert_eq!(loads, 1);
    assert_eq!(stores, 1);
    assert_eq!(retired, 3);
    assert_eq!(stored_value, crate::value::Value::Number(41.0));
}

#[test]
fn bridge_region_consumes_proven_parameter_word_load() {
    let executable = super::ExecutableCode::from_ops(vec![
        super::Op::LoadParameter { dst: 1, slot: 0 },
        super::Op::Return { src: 1 },
    ]);
    let code = executable.code();
    let key = crate::stencil_select::parameter_glue_region_key();
    let record = crate::stencil_select::select_region(key).expect("parameter row");
    assert_eq!(code.len(), record.operations.len());
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let control = cfg
        .region_plan(&entries, 0, record.operations)
        .expect("verified parameter CFG");
    let context = crate::vm::current_context_or_default();
    let (transition, native, loads, retired) = crate::stencil_policy::with_policy_for_test(
        crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
        || {
            let environment = crate::environment::Environment::new();
            environment.set(0, crate::value::Value::Number(17.5));
            let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
            let arena = std::rc::Rc::new(std::cell::RefCell::new(
                crate::stencil_arena::SharedStencilSlab::new(4096).expect("region arena"),
            ));
            let mut region = super::NativeRegionPlan::new_inner(
                key,
                true,
                arena,
                Some(control.clone()),
            )
            .expect("parameter region plan");
            let mut registers = crate::register_file::RegisterFile::with_undefined(4);
            let transition = region
                .execute(code, 0, &mut registers, &context, Some(&environment))
                .expect("parameter region execution");
            (
                transition,
                region.last_native_execution(),
                region.load_local_entry_count_for_test(),
                region.retired_operations(),
            )
        },
    );
    assert_eq!(
        transition.completion,
        Some(crate::completion::Completion::Return(
            crate::value::Value::Number(17.5),
        ))
    );
    assert!(native);
    assert_eq!(loads, 1);
    assert_eq!(retired, 2);
}

#[test]
fn loop_body_bridge_consumes_word_leaves_around_canonical_update() {
    let instructions = vec![
        crate::ir::Instruction::load_local_checked(1, 0),
        crate::ir::Instruction::load_local_checked(2, 1),
        crate::ir::Instruction::binary(crate::ir::Opcode::Add, 3, 1, 2),
        crate::ir::Instruction::store_local(3, 3),
        crate::ir::Instruction::move_(4, 3),
        crate::ir::Instruction::update_local(5, 6, 7, false),
        crate::ir::Instruction::ret(4),
    ];
    let store = std::rc::Rc::new(super::CodeStore {
        instructions: instructions.into(),
        cold: Vec::<super::Op>::new().into(),
        ranges: vec![(0, 7)].into(),
        constants: vec![super::ConstantPool::empty()].into(),
        metadata: vec![vec![super::InstructionMeta::empty(); 7]].into(),
        layouts: vec![super::FunctionLayout {
            register_count: 8,
            frame_register_count: 8,
            parameter_end: None,
        }]
        .into(),
        quickening_sites: vec![
            Vec::<std::cell::RefCell<crate::quickening::QuickeningSite<4>>>::new()
                .into_boxed_slice(),
        ]
        .into(),
        operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
        catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
    });
    let range = super::CodeRange::new(super::CodeId(0), 0, 7).expect("loop body range");
    let code = store.code(range).expect("loop body code");
    let key = crate::stencil_select::loop_body_region_key();
    let record = crate::stencil_select::select_region(key).expect("loop body row");
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let control = cfg
        .region_plan(&entries, 0, record.operations)
        .expect("verified loop body CFG");
    let context = crate::vm::current_context_or_default();
    let (transition, native, loads, stores, moves, updates, retired, stored, updated) =
        crate::stencil_policy::with_policy_for_test(
            crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
            || {
                let environment = crate::environment::Environment::new();
                environment.set(0, crate::value::Value::Number(2.0));
                environment.set(1, crate::value::Value::Number(3.0));
                environment.set(3, crate::value::Value::Number(0.0));
                environment.set(7, crate::value::Value::Number(10.0));
                let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
                let arena = std::rc::Rc::new(std::cell::RefCell::new(
                    crate::stencil_arena::SharedStencilSlab::new(4096).expect("region arena"),
                ));
                let mut region = super::NativeRegionPlan::new_inner(
                    key,
                    true,
                    arena,
                    Some(control.clone()),
                )
                .expect("loop body region plan");
                let mut registers = crate::register_file::RegisterFile::with_undefined(8);
                let transition = region
                    .execute(code, 0, &mut registers, &context, Some(&environment))
                    .expect("loop body region execution");
                (
                    transition,
                    region.last_native_execution(),
                    region.load_local_entry_count_for_test(),
                    region.store_local_entry_count_for_test(),
                    region.move_entry_count_for_test(),
                    region.update_entry_count_for_test(),
                    region.retired_operations(),
                    environment.get(3),
                    environment.get(7),
                )
            },
        );
    assert_eq!(
        transition.completion,
        Some(crate::completion::Completion::Return(
            crate::value::Value::Number(5.0),
        ))
    );
    assert!(native);
    assert_eq!(loads, 2);
    assert_eq!(stores, 1);
    assert_eq!(moves, 1);
    assert_eq!(updates, 1);
    assert_eq!(retired, 7);
    assert_eq!(stored, crate::value::Value::Number(5.0));
    assert_eq!(updated, crate::value::Value::Number(11.0));
}

#[test]
fn update_return_bridge_uses_inc_leaf_only_for_proven_direct_numeric_slots() {
    let key = crate::stencil_select::update_return_region_key();
    let record = crate::stencil_select::select_region(key).expect("update-return row");
    let context = crate::vm::current_context_or_default();
    for (decrement, expected_updated, expected_native, expected_entries) in
        [(false, 42.0, true, 1), (true, 40.0, true, 1)]
    {
        let store = std::rc::Rc::new(super::CodeStore {
            instructions: vec![
                crate::ir::Instruction::update_local(0, 1, 2, decrement),
                crate::ir::Instruction::ret(0),
            ]
            .into(),
            cold: Vec::<super::Op>::new().into(),
            ranges: vec![(0, 2)].into(),
            constants: vec![super::ConstantPool::empty()].into(),
            metadata: vec![vec![super::InstructionMeta::empty(); 2]].into(),
            layouts: vec![super::FunctionLayout {
                register_count: 3,
                frame_register_count: 3,
                parameter_end: None,
            }]
            .into(),
            quickening_sites: vec![
                Vec::<std::cell::RefCell<crate::quickening::QuickeningSite<4>>>::new()
                    .into_boxed_slice(),
            ]
            .into(),
            operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
            catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
        });
        let range = super::CodeRange::new(super::CodeId(0), 0, 2).expect("update-return range");
        let code = store.code(range).expect("update-return code");
        let entries = super::baseline_entries(code);
        let operand_windows = (0..entries.len())
            .map(|pc| code.operand_window_at(pc))
            .collect::<Vec<_>>();
        let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
        let control = cfg
            .region_plan(&entries, 0, record.operations)
            .expect("verified update-return CFG");
        crate::stencil_policy::with_policy_for_test(
            crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
            || {
                let environment = crate::environment::Environment::new();
                environment.set(2, crate::value::Value::Number(41.0));
                let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
                let arena = std::rc::Rc::new(std::cell::RefCell::new(
                    crate::stencil_arena::SharedStencilSlab::new(4096).expect("region arena"),
                ));
                let mut region = super::NativeRegionPlan::new_inner(
                    key,
                    true,
                    arena,
                    Some(control.clone()),
                )
                .expect("update-return region plan");
                let mut registers = crate::register_file::RegisterFile::with_undefined(3);
                let transition = region
                    .execute(code, 0, &mut registers, &context, Some(&environment))
                    .expect("update-return region execution");
                assert_eq!(
                    transition.completion,
                    Some(crate::completion::Completion::Return(
                        crate::value::Value::Number(41.0),
                    ))
                );
                assert_eq!(region.last_native_execution(), expected_native);
                assert_eq!(region.update_entry_count_for_test(), expected_entries);
                assert_eq!(environment.get(2), crate::value::Value::Number(expected_updated));
                assert_eq!(registers.read_number(1), Some(expected_updated));
            },
        );
    }

    let store = std::rc::Rc::new(super::CodeStore {
        instructions: vec![
            crate::ir::Instruction::update_local(0, 1, 2, false),
            crate::ir::Instruction::ret(0),
        ]
        .into(),
        cold: Vec::<super::Op>::new().into(),
        ranges: vec![(0, 2)].into(),
        constants: vec![super::ConstantPool::empty()].into(),
        metadata: vec![vec![super::InstructionMeta::empty(); 2]].into(),
        layouts: vec![super::FunctionLayout {
            register_count: 3,
            frame_register_count: 3,
            parameter_end: None,
        }]
        .into(),
        quickening_sites: vec![
            Vec::<std::cell::RefCell<crate::quickening::QuickeningSite<4>>>::new()
                .into_boxed_slice(),
        ]
        .into(),
        operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
        catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
    });
    let range = super::CodeRange::new(super::CodeId(0), 0, 2).expect("immutable range");
    let code = store.code(range).expect("immutable code");
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let control = cfg
        .region_plan(&entries, 0, record.operations)
        .expect("verified immutable CFG");
    crate::stencil_policy::with_policy_for_test(
        crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
        || {
            let environment = crate::environment::Environment::new();
            environment.set(2, crate::value::Value::Number(41.0));
            environment.mark_immutable_slot(2);
            let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
            let arena = std::rc::Rc::new(std::cell::RefCell::new(
                crate::stencil_arena::SharedStencilSlab::new(4096).expect("region arena"),
            ));
            let mut region = super::NativeRegionPlan::new_inner(
                key,
                true,
                arena,
                Some(control),
            )
            .expect("immutable update-return region plan");
            let mut registers = crate::register_file::RegisterFile::with_undefined(3);
            assert!(region
                .execute(code, 0, &mut registers, &context, Some(&environment))
                .is_err());
            assert!(!region.last_native_execution());
            assert_eq!(region.update_entry_count_for_test(), 0);
            assert_eq!(environment.get(2), crate::value::Value::Number(41.0));
        },
    );
}

#[test]
fn counted_bridge_resides_through_verified_backedge_with_physical_control() {
    let instructions = vec![
        crate::ir::Instruction::load_const(0, 0),
        crate::ir::Instruction::load_const(1, 1),
        crate::ir::Instruction::binary(crate::ir::Opcode::LessThan, 2, 0, 1),
        crate::ir::Instruction::jump_if_false(2, 6),
        crate::ir::Instruction::inc_i(0, 0, false),
        crate::ir::Instruction::jump(2),
        crate::ir::Instruction::ret(0),
    ];
    let store = std::rc::Rc::new(super::CodeStore {
        instructions: instructions.into(),
        cold: Vec::<super::Op>::new().into(),
        ranges: vec![(0, 7)].into(),
        constants: vec![super::ConstantPool::new(vec![
            super::Constant::Number(0.0),
            super::Constant::Number(3.0),
        ])]
        .into(),
        metadata: vec![vec![super::InstructionMeta::empty(); 7]].into(),
        layouts: vec![super::FunctionLayout {
            register_count: 3,
            frame_register_count: 3,
            parameter_end: None,
        }]
        .into(),
        quickening_sites: vec![
            Vec::<std::cell::RefCell<crate::quickening::QuickeningSite<4>>>::new()
                .into_boxed_slice(),
        ]
        .into(),
        operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
        catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
    });
    let range = super::CodeRange::new(super::CodeId(0), 0, 7).expect("counted range");
    let code = store.code(range).expect("counted code");
    let key = crate::stencil_select::counted_glue_region_key();
    let record = crate::stencil_select::select_region(key).expect("counted row");
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let control = cfg
        .region_plan(&entries, 0, record.operations)
        .expect("verified counted CFG");
    assert_eq!(
        control.internal_backedges(),
        [crate::stencil_cfg::RegionEdge { from: 5, to: 2 }]
    );
    let mut selection_policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    selection_policy.fused_regions = true;
    let candidate_arena = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("candidate arena"),
    ));
    assert!(super::region_admission(
        &entries,
        &cfg,
        0,
        selection_policy,
        &candidate_arena
    )
    .is_some());
    let baseline = super::BaselinePlan::compile_for_test(code, selection_policy);
    assert!(
        baseline.native_region_at(0).is_some(),
        "generic region admission must select the counted CFG witness"
    );
    let context = crate::vm::current_context_or_default();
    let (transition, native, constants, binaries, branches, jumps, retired) =
        crate::stencil_policy::with_policy_for_test(
            crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
            || {
                let arena = std::rc::Rc::new(std::cell::RefCell::new(
                    crate::stencil_arena::SharedStencilSlab::new(4096).expect("region arena"),
                ));
                let mut region = super::NativeRegionPlan::new_inner(
                    key,
                    true,
                    arena,
                    Some(control.clone()),
                )
                .expect("counted region plan");
                let mut registers = crate::register_file::RegisterFile::with_undefined(3);
                let transition = region
                    .execute(code, 0, &mut registers, &context, None)
                    .expect("counted region execution");
                (
                    transition,
                    region.last_native_execution(),
                    region.constant_entry_count_for_test(),
                    region.binary_entry_count_for_test(),
                    region.branch_entry_count_for_test(),
                    region.jump_entry_count_for_test(),
                    region.retired_operations(),
                )
            },
        );
    assert_eq!(
        transition.completion,
        Some(crate::completion::Completion::Return(
            crate::value::Value::Number(3.0),
        ))
    );
    assert!(native);
    assert_eq!(constants, 2);
    assert_eq!(binaries, 7);
    assert_eq!(branches, 4);
    assert_eq!(jumps, 3);
    assert_eq!(retired, 17);

    let interrupt_context = crate::vm::VmContext::default();
    interrupt_context.request_interrupt();
    let (handoff, interrupted_retired, interrupted_value, interrupt_cleared) =
        crate::stencil_policy::with_policy_for_test(
            crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
            || {
                let arena = std::rc::Rc::new(std::cell::RefCell::new(
                    crate::stencil_arena::SharedStencilSlab::new(4096).expect("region arena"),
                ));
                let mut region = super::NativeRegionPlan::new_inner(
                    key,
                    true,
                    arena,
                    Some(control.clone()),
                )
                .expect("interrupted counted region plan");
                let mut registers = crate::register_file::RegisterFile::with_undefined(3);
                let handoff = region
                    .execute(code, 0, &mut registers, &interrupt_context, None)
                    .expect("interrupted counted region execution");
                (
                    handoff,
                    region.retired_operations(),
                    registers.read_number(0),
                    !unsafe { &*interrupt_context.interrupt_flag() }
                        .load(std::sync::atomic::Ordering::Acquire),
                )
            },
        );
    assert!(matches!(
        handoff.target,
        crate::vm::DispatchTarget::Callee(2)
    ));
    assert!(handoff.completion.is_none());
    assert_eq!(interrupted_retired, 6);
    assert_eq!(interrupted_value, Some(1.0));
    assert!(interrupt_cleared);
}

#[test]
fn counted_decrement_bridge_resides_with_distinct_minus_one_artifact() {
    let instructions = vec![
        crate::ir::Instruction::load_const(0, 0),
        crate::ir::Instruction::load_const(1, 1),
        crate::ir::Instruction::binary(crate::ir::Opcode::GreaterThan, 2, 0, 1),
        crate::ir::Instruction::jump_if_false(2, 6),
        crate::ir::Instruction::inc_i(0, 0, true),
        crate::ir::Instruction::jump(2),
        crate::ir::Instruction::ret(0),
    ];
    let store = std::rc::Rc::new(super::CodeStore {
        instructions: instructions.into(),
        cold: Vec::<super::Op>::new().into(),
        ranges: vec![(0, 7)].into(),
        constants: vec![super::ConstantPool::new(vec![
            super::Constant::Number(3.0),
            super::Constant::Number(0.0),
        ])]
        .into(),
        metadata: vec![vec![super::InstructionMeta::empty(); 7]].into(),
        layouts: vec![super::FunctionLayout {
            register_count: 3,
            frame_register_count: 3,
            parameter_end: None,
        }]
        .into(),
        quickening_sites: vec![
            Vec::<std::cell::RefCell<crate::quickening::QuickeningSite<4>>>::new()
                .into_boxed_slice(),
        ]
        .into(),
        operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
        catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
    });
    let range = super::CodeRange::new(super::CodeId(0), 0, 7).expect("decrement range");
    let code = store.code(range).expect("decrement code");
    let key = crate::stencil_select::counted_decrement_glue_region_key();
    let record = crate::stencil_select::select_region(key).expect("decrement counted row");
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let control = cfg
        .region_plan(&entries, 0, record.operations)
        .expect("verified decrement CFG");
    assert_eq!(
        control.internal_backedges(),
        [crate::stencil_cfg::RegionEdge { from: 5, to: 2 }]
    );
    let mut selection_policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    selection_policy.fused_regions = true;
    let candidate_arena = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("candidate arena"),
    ));
    assert!(super::region_admission(
        &entries,
        &cfg,
        0,
        selection_policy,
        &candidate_arena
    )
    .is_some());
    let baseline = super::BaselinePlan::compile_for_test(code, selection_policy);
    assert_eq!(
        baseline
            .native_region_at(0)
            .expect("nested continue admission")
            .borrow()
            .key_for_test(),
        key
    );
    let context = crate::vm::current_context_or_default();
    let (transition, native, binaries, branches, jumps, retired) =
        crate::stencil_policy::with_policy_for_test(
            crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
            || {
                let arena = std::rc::Rc::new(std::cell::RefCell::new(
                    crate::stencil_arena::SharedStencilSlab::new(4096).expect("region arena"),
                ));
                let mut region = super::NativeRegionPlan::new_inner(
                    key,
                    true,
                    arena,
                    Some(control.clone()),
                )
                .expect("decrement counted region plan");
                let mut registers = crate::register_file::RegisterFile::with_undefined(3);
                let transition = region
                    .execute(code, 0, &mut registers, &context, None)
                    .expect("decrement counted region execution");
                (
                    transition,
                    region.last_native_execution(),
                    region.binary_entry_count_for_test(),
                    region.branch_entry_count_for_test(),
                    region.jump_entry_count_for_test(),
                    region.retired_operations(),
                )
            },
        );
    assert_eq!(
        transition.completion,
        Some(crate::completion::Completion::Return(
            crate::value::Value::Number(0.0),
        ))
    );
    assert!(native);
    assert_eq!(binaries, 7);
    assert_eq!(branches, 4);
    assert_eq!(jumps, 3);
    assert_eq!(retired, 17);
}

#[test]
fn counted_continue_bridge_follows_nested_exits_and_resident_backedge() {
    let instructions = vec![
        crate::ir::Instruction::load_const(0, 0),
        crate::ir::Instruction::load_const(1, 1),
        crate::ir::Instruction::binary(crate::ir::Opcode::LessThan, 2, 0, 1),
        crate::ir::Instruction::jump_if_false(2, 12),
        crate::ir::Instruction::load_local(3, 0),
        crate::ir::Instruction::jump_if_false(3, 8),
        crate::ir::Instruction::load_local(4, 1),
        crate::ir::Instruction::jump_if_false(4, 9),
        crate::ir::Instruction::jump(12),
        crate::ir::Instruction::inc_i(0, 0, false),
        crate::ir::Instruction::jump(2),
        crate::ir::Instruction::jump(12),
        crate::ir::Instruction::ret(0),
    ];
    let store = std::rc::Rc::new(super::CodeStore {
        instructions: instructions.into(),
        cold: Vec::<super::Op>::new().into(),
        ranges: vec![(0, 13)].into(),
        constants: vec![super::ConstantPool::new(vec![
            super::Constant::Number(0.0),
            super::Constant::Number(3.0),
        ])]
        .into(),
        metadata: vec![vec![super::InstructionMeta::empty(); 13]].into(),
        layouts: vec![super::FunctionLayout {
            register_count: 5,
            frame_register_count: 5,
            parameter_end: None,
        }]
        .into(),
        quickening_sites: vec![
            Vec::<std::cell::RefCell<crate::quickening::QuickeningSite<4>>>::new()
                .into_boxed_slice(),
        ]
        .into(),
        operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
        catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
    });
    let range = super::CodeRange::new(super::CodeId(0), 0, 13).expect("continue range");
    let code = store.code(range).expect("continue code");
    let key = crate::stencil_select::counted_continue_glue_region_key();
    let record = crate::stencil_select::select_region(key).expect("continue row");
    let entries = super::baseline_entries(code);
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &operand_windows);
    let control = cfg
        .region_plan(&entries, 0, record.operations)
        .expect("verified nested continue CFG");
    assert_eq!(
        control.internal_backedges(),
        [crate::stencil_cfg::RegionEdge { from: 10, to: 2 }]
    );
    assert_eq!(control.join_blocks(), [12, 8]);
    let mut selection_policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    selection_policy.fused_regions = true;
    let candidate_arena = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("candidate arena"),
    ));
    assert!(super::region_admission(
        &entries,
        &cfg,
        0,
        selection_policy,
        &candidate_arena
    )
    .is_some());
    let context = crate::vm::current_context_or_default();

    crate::stencil_policy::with_policy_for_test(
        crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
        || {
            for (skip, break_now, expected, constants, binaries, branches, jumps, loads, retired) in [
                (true, false, 3.0, 2, 7, 10, 3, 6, 29),
                (false, false, 0.0, 2, 1, 2, 1, 1, 8),
                (true, true, 0.0, 2, 1, 3, 1, 2, 10),
            ] {
                let environment = crate::environment::Environment::new();
                environment.set(0, crate::value::Value::Boolean(skip));
                environment.set(1, crate::value::Value::Boolean(break_now));
                let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
                let arena = std::rc::Rc::new(std::cell::RefCell::new(
                    crate::stencil_arena::SharedStencilSlab::new(4096)
                        .expect("region arena"),
                ));
                let mut region = super::NativeRegionPlan::new_inner(
                    key,
                    true,
                    arena,
                    Some(control.clone()),
                )
                .expect("nested continue region plan");
                let mut registers = crate::register_file::RegisterFile::with_undefined(5);
                let transition = region
                    .execute(code, 0, &mut registers, &context, Some(&environment))
                    .expect("nested continue region execution");
                assert_eq!(
                    transition.completion,
                    Some(crate::completion::Completion::Return(
                        crate::value::Value::Number(expected),
                    ))
                );
                assert!(region.last_native_execution());
                assert_eq!(region.constant_entry_count_for_test(), constants);
                assert_eq!(region.binary_entry_count_for_test(), binaries);
                assert_eq!(region.branch_entry_count_for_test(), branches);
                assert_eq!(region.jump_entry_count_for_test(), jumps);
                assert_eq!(region.load_local_entry_count_for_test(), loads);
                assert_eq!(region.retired_operations(), retired);
            }

            let interrupt_context = crate::vm::VmContext::default();
            interrupt_context.request_interrupt();
            let environment = crate::environment::Environment::new();
            environment.set(0, crate::value::Value::Boolean(true));
            environment.set(1, crate::value::Value::Boolean(false));
            let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
            let arena = std::rc::Rc::new(std::cell::RefCell::new(
                crate::stencil_arena::SharedStencilSlab::new(4096).expect("region arena"),
            ));
            let mut region = super::NativeRegionPlan::new_inner(
                key,
                true,
                arena,
                Some(control),
            )
            .expect("interrupted nested continue region plan");
            let mut registers = crate::register_file::RegisterFile::with_undefined(5);
            let handoff = region
                .execute(
                    code,
                    0,
                    &mut registers,
                    &interrupt_context,
                    Some(&environment),
                )
                .expect("interrupted nested continue execution");
            assert!(matches!(
                handoff.target,
                crate::vm::DispatchTarget::Callee(2)
            ));
            assert!(handoff.completion.is_none());
            assert_eq!(region.retired_operations(), 10);
            assert_eq!(registers.read_number(0), Some(1.0));
            assert!(!unsafe { &*interrupt_context.interrupt_flag() }
                .load(std::sync::atomic::Ordering::Acquire));
        },
    );
}

#[test]
fn physical_failure_releases_local_installation_storage() {
    let mut physical = super::PhysicalInstallation::local(super::InstalledRegionEntry::Unpublished);
    physical
        .storage
        .local_mut()
        .expect("local physical storage");
    assert!(physical.storage.local().is_some());
    let failure: Result<(), super::NativeDispatchError> =
        Err(super::NativeDispatchError::Physical("invalid entry".into()));

    physical.apply_dispatch_outcome(
        &failure,
        None,
        super::InstalledRegionEntry::Unpublished,
    );

    assert!(
        physical.storage.local().is_none(),
        "failed physical publication must release its local mapping"
    );
    assert_eq!(
        physical.state.lifecycle.state(),
        crate::stencil_lifecycle::StencilState::Cold
    );
}

#[test]
fn committed_failure_releases_local_storage_and_retires_state() {
    let mut physical = super::PhysicalInstallation::local(super::InstalledRegionEntry::Unpublished);
    physical
        .storage
        .local_mut()
        .expect("local physical storage");
    let failure: Result<(), super::NativeDispatchError> =
        Err(super::NativeDispatchError::committed(3, "post-entry failure"));

    physical.apply_dispatch_outcome(
        &failure,
        None,
        super::InstalledRegionEntry::Unpublished,
    );

    assert!(physical.storage.local().is_none());
    assert_eq!(
        physical.state.lifecycle.state(),
        crate::stencil_lifecycle::StencilState::Retired
    );
}

#[test]
fn region_admission_rejects_noncanonical_operands_before_publication() {
    let record =
        crate::stencil_select::select_region(crate::stencil_select::loop_body_region_key())
            .expect("generated loop-body declaration");
    let mut entries = record
        .operations
        .iter()
        .copied()
        .map(|opcode| {
            let instruction = crate::ir::Instruction {
                opcode,
                flags: 0,
                a: 0,
                b: 0,
                c: 0,
            };
            super::BaselineEntry {
                instruction,
                control: opcode.control_operands(instruction),
            }
        })
        .collect::<Vec<_>>();
    entries[0].instruction.c = 1;
    let windows = vec![None; entries.len()];
    let cfg = crate::stencil_cfg::ControlFlowFacts::new(&entries, &windows);
    assert!(!super::region_admission_matches(&entries, &cfg, 0, record));
}

#[test]
fn region_admission_rejects_external_backedge_into_interior() {
    let instructions = [
        crate::ir::Instruction::move_(0, 1),
        crate::ir::Instruction::move_(1, 2),
        crate::ir::Instruction::ret(1),
        crate::ir::Instruction::jump(1),
    ];
    let entries = instructions
        .into_iter()
        .map(|instruction| super::BaselineEntry {
            instruction,
            control: instruction.opcode.control_operands(instruction),
        })
        .collect::<Vec<_>>();
    let cfg = crate::stencil_cfg::ControlFlowFacts::new(&entries, &[None; 4]);
    assert!(!cfg.region_entry_is_legal(0, 3));
}

#[test]
fn generated_scalar_and_array_rows_route_through_declared_abis() {
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let scalar_add = super::FunctionCode::from_ops(vec![
        super::Op::Binary {
            dst: 0,
            operator: crate::ops::BinaryOp::Add,
            lhs: 1,
            rhs: 2,
        },
        super::Op::Return { src: 0 },
    ]);
    let scalar_plan =
        super::BaselinePlan::compile_for_test(scalar_add.code().expect("scalar add code"), policy);
    assert!(scalar_plan.native_binary_at(0).is_some());
    assert!(scalar_plan.native_region_at(0).is_none());
    for ops in [
        vec![super::Op::Move { dst: 0, src: 1 }],
        vec![super::Op::GetProperty {
            dst: 0,
            object: 1,
            key: "x".into(),
        }],
    ] {
        let function = super::FunctionCode::from_ops(ops);
        let code = function.code().expect("compact code");
        let plan = super::BaselinePlan::compile_for_test(code, policy);
        // Move and GetN bytes use scalar/raw-word entry ABIs.  They must not
        // be called as NativeRegionContext bridges merely because their
        // opcode appears in the generated catalog.
        assert!(plan.native_region_at(0).is_none());
    }

    // The array rows are generated with a distinct ABI.  Their executable
    // entry is reached only by the residual lowering/admission tests, while
    // this check ensures the catalog itself cannot silently classify them as
    // scalar entries.
    let array_record =
        crate::stencil_select::select_region(crate::stencil_select::array_loop_body_region_key())
            .expect("array row");
    assert_eq!(array_record.name, "array_loop_body");
    assert_eq!(array_record.entry, 0);
    assert_eq!(array_record.external_entries, &[0]);
    assert!(matches!(
        array_record.abi,
        crate::stencil_select::RegionAbi::ArrayKernel | crate::stencil_select::RegionAbi::Bridge
    ));
}

#[test]
fn hot_function_builds_one_reusable_baseline_plan() {
    let function = super::FunctionCode::from_ops(vec![super::Op::Move { dst: 0, src: 0 }]);
    function.set_tier_threshold_for_test(2);
    assert_eq!(function.tier(), super::ExecutionTier::Interpreter);
    assert_eq!(function.enter_invocation(), super::TierTransition::Cold);
    function.retire(2);
    assert_eq!(
        function.enter_invocation(),
        super::TierTransition::CompileBaseline
    );
    let first = function.baseline_plan().expect("baseline plan");
    assert_eq!(first.len(), 1);
    assert_eq!(function.enter_invocation(), super::TierTransition::Baseline);
    let second = function.baseline_plan().expect("baseline plan remains");
    assert!(std::rc::Rc::ptr_eq(&first, &second));
    assert_eq!(function.tier_counts(), (3, 2));
    let profile = function.tier_profile();
    assert_eq!(profile.tier, super::ExecutionTier::Baseline);
    assert_eq!(profile.invocations, 3);
    assert_eq!(profile.baseline_instructions, 1);
    assert_eq!(profile.osr_entries, 0);
}

#[test]
fn eager_baseline_plan_includes_composed_binary_series() {
    crate::stencil_policy::with_policy_for_test(
        crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
        || {
            let function = super::FunctionCode::from_ops(vec![
                super::Op::Binary {
                    dst: 3,
                    operator: crate::ops::BinaryOp::Add,
                    lhs: 1,
                    rhs: 2,
                },
                super::Op::Binary {
                    dst: 4,
                    operator: crate::ops::BinaryOp::Multiply,
                    lhs: 3,
                    rhs: 2,
                },
                super::Op::Return { src: 4 },
            ]);
            assert_eq!(
                function.enter_invocation(),
                super::TierTransition::CompileBaseline
            );
            let plan = function.baseline_plan().expect("eager baseline plan");
            assert!(plan.native_binary_series_at(0).is_some());
        },
    );
}

#[test]
fn warm_baseline_function_admits_optimizing_plan() {
    let function = super::FunctionCode::from_ops(vec![
        super::Op::Binary {
            dst: 0,
            operator: crate::ops::BinaryOp::Add,
            lhs: 1,
            rhs: 2,
        },
        super::Op::Return { src: 0 },
    ]);
    function.set_tier_threshold_for_test(1);
    function.retire(1);
    assert_eq!(
        function.enter_invocation(),
        super::TierTransition::CompileBaseline
    );
    for _ in 0..6 {
        assert_eq!(function.enter_invocation(), super::TierTransition::Baseline);
    }
    assert_eq!(
        function.enter_invocation(),
        super::TierTransition::CompileOptimizing
    );
    let profile = function.tier_profile();
    assert_eq!(profile.tier, super::ExecutionTier::Optimizing);
    assert_eq!(profile.optimizing_instructions, 2);
    assert!(function.optimizing_plan().is_some());
}

#[test]
fn invocation_count_alone_does_not_promote_a_cold_function() {
    let function = super::FunctionCode::from_ops(vec![super::Op::Move { dst: 0, src: 0 }]);
    function.set_tier_threshold_for_test(2);
    assert_eq!(function.enter_invocation(), super::TierTransition::Cold);
    assert_eq!(function.enter_invocation(), super::TierTransition::Cold);
    assert_eq!(function.tier(), super::ExecutionTier::Interpreter);
    assert_eq!(function.tier_counts(), (2, 0));
}

#[test]
fn hot_back_edge_osr_transfers_live_frame_into_baseline() {
    // Build a tiny compact loop directly so the test observes the exact
    // branch/back-edge admission used by the interpreter dispatcher.  The
    // first pass enters the loop with r0=true; the body flips it to false,
    // then the backward jump reaches the OSR candidate.  Baseline execution
    // resumes at pc=0 with the same register file and exits through pc=3.
    let store = std::rc::Rc::new(super::CodeStore {
        instructions: vec![
            crate::ir::Instruction::jump_if_false(0, 3),
            crate::ir::Instruction::load_const(0, 0),
            crate::ir::Instruction::jump(0),
            crate::ir::Instruction::ret(0),
        ]
        .into(),
        cold: Vec::<super::Op>::new().into(),
        ranges: vec![(0, 4)].into(),
        constants: vec![super::ConstantPool::new(vec![super::Constant::Boolean(
            false,
        )])]
        .into(),
        metadata: vec![vec![super::InstructionMeta::empty(); 4]].into(),
        layouts: vec![super::FunctionLayout {
            register_count: 1,
            frame_register_count: 1,
            parameter_end: None,
        }]
        .into(),
        quickening_sites: vec![
            Vec::<std::cell::RefCell<crate::quickening::QuickeningSite<4>>>::new()
                .into_boxed_slice(),
        ]
        .into(),
        operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
        catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
    });
    let range = super::CodeRange::new(super::CodeId(0), 0, 4).expect("valid test range");
    let owner = super::FunctionCode::new(store, range);
    owner.set_tier_threshold_for_test(1);
    let code = owner.code().expect("compact test code");
    assert!(!owner.is_osr_entry(2), "no plan before the back-edge");

    let mut registers =
        crate::register_file::RegisterFile::from_values(vec![crate::value::Value::Boolean(true)]);
    let context = crate::vm::current_context_or_default();
    let (completion, next) = crate::stencil_policy::with_policy_for_test(
        crate::stencil_policy::ExecutionPolicy::bridge_opt_in_for_test(),
        || {
            crate::vm::execute_function_code_step_from(code, &owner, 0, &mut registers, &context)
                .expect("hot loop executes")
        },
    );

    assert_eq!(
        completion,
        crate::completion::Completion::Return(crate::value::Value::Boolean(false))
    );
    assert_eq!(next, 4);
    assert_eq!(owner.tier(), super::ExecutionTier::Baseline);
    let profile = owner.tier_profile();
    assert_eq!(profile.osr_entries, 1);
    assert_eq!(profile.osr_transfers, 1);
    // The interpreter retires pc=0, pc=1 and the compiling back-edge pc=2;
    // the baseline continuation then retires its exact committed prefix
    // (pc=0 and pc=3).  OSR must not lose or double-count either view.
    assert_eq!(profile.retired, 5);
    let baseline = owner.baseline_plan().expect("baseline plan after OSR");
    assert_eq!(baseline.osr_backedge_target_at(2), Some(0));
    // OSR must enter the same admitted native region that a baseline start at
    // the loop header would use.  The region may still report a canonical
    // miss on an unavailable artifact, but this host has the validated Bridge
    // artifact and therefore must execute its generated branch/return leaves.
    let region = baseline
        .native_region_at(0)
        .expect("OSR target has a native region");
    let region = region.borrow();
    assert!(region.last_native_execution(), "OSR did not reach native CFG");
    assert!(region.branch_entry_count_for_test() > 0);
    assert!(region.return_entry_count_for_test() > 0);
    assert!(owner.is_osr_entry(2));

    // The same canonical compact code must remain correct when admission is
    // disabled: this is the differential guard that proves OSR is an
    // execution shortcut, not an alternate semantic path.
    let cold_owner = super::FunctionCode::new(owner.store().expect("test code store"), range);
    cold_owner.set_tier_threshold_for_test(100);
    let mut cold_registers =
        crate::register_file::RegisterFile::from_values(vec![crate::value::Value::Boolean(true)]);
    let (cold_completion, cold_next) = crate::vm::execute_function_code_step_from(
        cold_owner.code().expect("compact test code"),
        &cold_owner,
        0,
        &mut cold_registers,
        &context,
    )
    .expect("cold loop executes");
    assert_eq!(cold_completion, completion);
    assert_eq!(cold_next, next);
    assert_eq!(cold_owner.tier(), super::ExecutionTier::Interpreter);
    assert_eq!(cold_owner.tier_profile().osr_transfers, 0);
}

#[test]
fn hot_conditional_backedge_osr_uses_the_same_cfg_target() {
    // The branch itself is the resident backedge (rather than an
    // unconditional Jump).  OSR must compile at that edge and resume at the
    // CFG target without treating the fallthrough arm as the loop body.
    let store = std::rc::Rc::new(super::CodeStore {
        instructions: vec![
            crate::ir::Instruction::jump_if_false(0, 4),
            crate::ir::Instruction::load_const(0, 0),
            crate::ir::Instruction::jump_if_false(0, 0),
            crate::ir::Instruction::load_const(0, 1),
            crate::ir::Instruction::ret(0),
        ]
        .into(),
        cold: Vec::<super::Op>::new().into(),
        ranges: vec![(0, 5)].into(),
        constants: vec![super::ConstantPool::new(vec![
            super::Constant::Boolean(false),
            super::Constant::Boolean(true),
        ])]
        .into(),
        metadata: vec![vec![super::InstructionMeta::empty(); 5]].into(),
        layouts: vec![super::FunctionLayout {
            register_count: 1,
            frame_register_count: 1,
            parameter_end: None,
        }]
        .into(),
        quickening_sites: vec![
            Vec::<std::cell::RefCell<crate::quickening::QuickeningSite<4>>>::new()
                .into_boxed_slice(),
        ]
        .into(),
        operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
        catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
    });
    let range = super::CodeRange::new(super::CodeId(0), 0, 5).expect("valid branch range");
    let owner = super::FunctionCode::new(store, range);
    owner.set_tier_threshold_for_test(1);
    let mut registers = crate::register_file::RegisterFile::from_values(vec![
        crate::value::Value::Boolean(true),
    ]);
    let context = crate::vm::current_context_or_default();
    let (completion, next) = crate::stencil_policy::with_policy_for_test(
        crate::stencil_policy::ExecutionPolicy::bridge_opt_in_for_test(),
        || {
            crate::vm::execute_function_code_step_from(
                owner.code().expect("compact branch code"),
                &owner,
                0,
                &mut registers,
                &context,
            )
            .expect("conditional backedge executes")
        },
    );
    assert_eq!(
        completion,
        crate::completion::Completion::Return(crate::value::Value::Boolean(false))
    );
    assert_eq!(next, 5);
    assert_eq!(owner.tier(), super::ExecutionTier::Baseline);
    let profile = owner.tier_profile();
    assert_eq!(profile.osr_entries, 1);
    assert_eq!(profile.osr_transfers, 1);
    assert_eq!(profile.retired, 5);
    assert_eq!(owner.osr_backedge_target_at(2), Some(0));
    let plan = owner
        .baseline_plan()
        .expect("baseline plan after conditional OSR");
    let region = plan
        .native_region_at(0)
        .expect("conditional OSR target has a region");
    assert!(region.borrow().last_native_execution());
    assert!(region.borrow().branch_entry_count_for_test() > 0);
}

#[test]
fn bridge_accepts_branch_to_code_end_as_normal_exit() {
    // The exclusive code-end PC is a legal external edge.  A bridge may
    // execute the truthiness leaf or fall back to the canonical branch
    // handler, but either view must complete normally without a synthetic
    // instruction at the sentinel.
    let store = std::rc::Rc::new(super::CodeStore {
        instructions: vec![
            crate::ir::Instruction::jump_if_false(0, 3),
            crate::ir::Instruction::load_const(0, 0),
            crate::ir::Instruction::ret(0),
        ]
        .into(),
        cold: Vec::<super::Op>::new().into(),
        ranges: vec![(0, 3)].into(),
        constants: vec![super::ConstantPool::new(vec![super::Constant::Boolean(
            true,
        )])]
        .into(),
        metadata: vec![vec![super::InstructionMeta::empty(); 3]].into(),
        layouts: vec![super::FunctionLayout {
            register_count: 1,
            frame_register_count: 1,
            parameter_end: None,
        }]
        .into(),
        quickening_sites: vec![
            Vec::<std::cell::RefCell<crate::quickening::QuickeningSite<4>>>::new()
                .into_boxed_slice(),
        ]
        .into(),
        operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
        catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
    });
    let range = super::CodeRange::new(super::CodeId(0), 0, 3).expect("valid branch range");
    let owner = super::FunctionCode::new(store, range);
    owner.set_tier_threshold_for_test(100);
    let code = owner.code().expect("compact branch code");
    let bridge_policy = crate::stencil_policy::ExecutionPolicy::bridge_opt_in_for_test();
    let baseline = super::BaselinePlan::compile_for_test(code, bridge_policy);
    assert!(baseline
        .control_facts()
        .region_control(0, 3)
        .is_some());
    let mut registers = crate::register_file::RegisterFile::from_values(vec![
        crate::value::Value::Boolean(false),
    ]);
    let context = crate::vm::current_context_or_default();
    let (completion, next) = crate::stencil_policy::with_policy_for_test(
        bridge_policy,
        || {
            crate::vm::execute_baseline_code_step_from(
                code,
                &baseline,
                0,
                &mut registers,
                &context,
            )
            .expect("branch-to-end executes")
        },
    );
    assert_eq!(completion, crate::completion::Completion::Normal);
    assert_eq!(next, 3);
    let region = baseline
        .native_region_at(0)
        .expect("branch-to-end bridge region");
    let region = region.borrow();
    assert!(region.last_native_execution());
    assert_eq!(region.branch_entry_count_for_test(), 1);
}

#[test]
fn admits_native_constant_branch_only_for_verified_cfg_shape() {
    let store = std::rc::Rc::new(super::CodeStore {
        instructions: vec![
            crate::ir::Instruction::jump_if_false(0, 3),
            crate::ir::Instruction::load_const(0, 0),
            crate::ir::Instruction::ret(0),
            crate::ir::Instruction::load_const(0, 1),
            crate::ir::Instruction::ret(0),
        ]
        .into(),
        cold: Vec::<super::Op>::new().into(),
        ranges: vec![(0, 5)].into(),
        constants: vec![super::ConstantPool::new(vec![
            super::Constant::Number(1.0),
            super::Constant::Number(2.0),
        ])]
        .into(),
        metadata: vec![vec![super::InstructionMeta::empty(); 5]].into(),
        layouts: vec![super::FunctionLayout {
            register_count: 1,
            frame_register_count: 1,
            parameter_end: None,
        }]
        .into(),
        quickening_sites: vec![
            Vec::<std::cell::RefCell<crate::quickening::QuickeningSite<4>>>::new()
                .into_boxed_slice(),
        ]
        .into(),
        operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
        catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
    });
    let range = super::CodeRange::new(super::CodeId(0), 0, 5).expect("valid test range");
    let owner = super::FunctionCode::new(store, range);
    let code = owner.code().expect("compact test code");
    let plan = super::BaselinePlan::compile_for_test(
        code,
        crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
    );
    assert!(plan.native_constant_branch_at(0).is_some());
    assert!(plan.native_constant_branch_at(1).is_none());

    for (condition, expected) in [
        (crate::value::Value::Boolean(true), crate::value::Value::Number(1.0)),
        (crate::value::Value::Boolean(false), crate::value::Value::Number(2.0)),
        (crate::value::Value::Number(0.0), crate::value::Value::Number(2.0)),
        (crate::value::Value::Number(3.0), crate::value::Value::Number(1.0)),
    ] {
        let mut registers = crate::register_file::RegisterFile::from_values(vec![condition]);
        let context = crate::vm::current_context_or_default();
        let (completion, _) = crate::vm::execute_baseline_code_step_from(
            code, &plan, 0, &mut registers, &context,
        )
        .expect("constant branch executes");
        assert_eq!(completion, crate::completion::Completion::Return(expected));
    }
}

#[test]
fn admits_boolean_branch_only_cover_for_nonterminal_arms() {
    let store = std::rc::Rc::new(super::CodeStore {
        instructions: vec![
            crate::ir::Instruction::load_const(0, 0),
            crate::ir::Instruction::jump_if_false(0, 4),
            crate::ir::Instruction::ret(1),
            crate::ir::Instruction::ret(1),
            crate::ir::Instruction::ret(2),
        ]
        .into(),
        cold: Vec::<super::Op>::new().into(),
        ranges: vec![(0, 5)].into(),
        constants: vec![super::ConstantPool::new(vec![super::Constant::Boolean(true)])].into(),
        metadata: vec![vec![super::InstructionMeta::empty(); 5]].into(),
        layouts: vec![super::FunctionLayout {
            register_count: 3,
            frame_register_count: 3,
            parameter_end: None,
        }]
        .into(),
        quickening_sites: vec![
            Vec::<std::cell::RefCell<crate::quickening::QuickeningSite<4>>>::new()
                .into_boxed_slice(),
        ]
        .into(),
        operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
        catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
    });
    let range = super::CodeRange::new(super::CodeId(0), 0, 5).expect("valid branch range");
    let owner = super::FunctionCode::new(store, range);
    let code = owner.code().expect("compact branch code");
    let plan = super::BaselinePlan::compile_for_test(
        code,
        crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
    );
    if let Some(native) = plan.native_word_branch_at(1) {
        assert_eq!(
            native.borrow().continuation(),
            crate::stencil_word_composition::NativeWordBranchContinuation::Jump {
                truthy_pc: 2,
                falsy_pc: 4,
            }
        );
    }
    let context = crate::vm::current_context_or_default();
    for (condition, expected_pc) in [
        (crate::value::Value::Boolean(true), 2),
        (crate::value::Value::Boolean(false), 4),
    ] {
        let expected_value = if expected_pc == 2 { 11.0 } else { 22.0 };
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            condition,
            crate::value::Value::Number(11.0),
            crate::value::Value::Number(22.0),
        ]);
        let (completion, next) = crate::vm::execute_baseline_code_step_from(
            code, &plan, 1, &mut registers, &context,
        )
        .expect("boolean branch chooses canonical successor");
        assert_eq!(
            completion,
            crate::completion::Completion::Return(crate::value::Value::Number(expected_value))
        );
        assert_eq!(next, expected_pc + 1);
    }
}

#[test]
fn admits_constant_branch_with_cfg_derived_arm_gap() {
    let store = std::rc::Rc::new(super::CodeStore {
        instructions: vec![
            crate::ir::Instruction::jump_if_false(0, 5),
            crate::ir::Instruction::load_const(0, 0),
            crate::ir::Instruction::ret(0),
            // This straight-line padding is unreachable from the branch arms.
            crate::ir::Instruction::move_(0, 0),
            crate::ir::Instruction::move_(0, 0),
            crate::ir::Instruction::load_const(0, 1),
            crate::ir::Instruction::ret(0),
        ]
        .into(),
        cold: Vec::<super::Op>::new().into(),
        ranges: vec![(0, 7)].into(),
        constants: vec![super::ConstantPool::new(vec![
            super::Constant::Number(11.0),
            super::Constant::Number(22.0),
        ])]
        .into(),
        metadata: vec![vec![super::InstructionMeta::empty(); 7]].into(),
        layouts: vec![super::FunctionLayout {
            register_count: 1,
            frame_register_count: 1,
            parameter_end: None,
        }]
        .into(),
        quickening_sites: vec![
            Vec::<std::cell::RefCell<crate::quickening::QuickeningSite<4>>>::new()
                .into_boxed_slice(),
        ]
        .into(),
        operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
        catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
    });
    let range = super::CodeRange::new(super::CodeId(0), 0, 7).expect("valid test range");
    let owner = super::FunctionCode::new(store, range);
    let code = owner.code().expect("compact test code");
    let plan = super::BaselinePlan::compile_for_test(
        code,
        crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
    );
    assert!(plan.native_constant_branch_at(0).is_some());

    for (condition, expected) in [
        (crate::value::Value::Boolean(true), 11.0),
        (crate::value::Value::Boolean(false), 22.0),
    ] {
        let mut registers = crate::register_file::RegisterFile::from_values(vec![condition]);
        let context = crate::vm::current_context_or_default();
        let (completion, _) = crate::vm::execute_baseline_code_step_from(
            code, &plan, 0, &mut registers, &context,
        )
        .expect("gapped constant branch executes");
        assert_eq!(
            completion,
            crate::completion::Completion::Return(crate::value::Value::Number(expected))
        );
    }
}

#[test]
fn word_branch_shape_preserves_return_value_identity() {
    let store = std::rc::Rc::new(super::CodeStore {
        instructions: vec![
            crate::ir::Instruction::jump_if_false(0, 2),
            crate::ir::Instruction::ret(1),
            crate::ir::Instruction::ret(2),
        ]
        .into(),
        cold: Vec::<super::Op>::new().into(),
        ranges: vec![(0, 3)].into(),
        constants: vec![super::ConstantPool::empty()].into(),
        metadata: vec![vec![super::InstructionMeta::empty(); 3]].into(),
        layouts: vec![super::FunctionLayout {
            register_count: 3,
            frame_register_count: 3,
            parameter_end: None,
        }]
        .into(),
        quickening_sites: vec![
            Vec::<std::cell::RefCell<crate::quickening::QuickeningSite<4>>>::new()
                .into_boxed_slice(),
        ]
        .into(),
        operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
        catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
    });
    let range = super::CodeRange::new(super::CodeId(0), 0, 3).expect("valid branch range");
    let owner = super::FunctionCode::new(store, range);
    let code = owner.code().expect("compact branch code");
    let plan = super::BaselinePlan::compile_for_test(
        code,
        crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
    );
    let native_available = plan.native_word_branch_at(0).is_some();
    #[cfg(quench_generated_stencil_artifacts)]
    assert!(
        native_available,
        "generated word-branch artifacts must admit the canonical witness"
    );
    for (condition, expected) in [
        (crate::value::Value::Boolean(true), 11.0),
        (crate::value::Value::Boolean(false), 22.0),
        (crate::value::Value::Number(0.0), 22.0),
        (crate::value::Value::Number(3.0), 11.0),
    ] {
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            condition,
            crate::value::Value::Number(11.0),
            crate::value::Value::Number(22.0),
        ]);
        let context = crate::vm::current_context_or_default();
        let (completion, _) = crate::vm::execute_baseline_code_step_from(
            code, &plan, 0, &mut registers, &context,
        )
        .expect("word branch executes");
        assert_eq!(
            completion,
            crate::completion::Completion::Return(crate::value::Value::Number(expected))
        );
    }
    if native_available {
        assert!(plan
            .native_word_branch_at(0)
            .is_some_and(|branch| branch.borrow().native_entry_count() >= 2));
    }
}

#[test]
fn structured_fori_is_not_an_osr_back_edge() {
    let instruction = crate::ir::Instruction {
        opcode: crate::ir::Opcode::ForI,
        flags: 0,
        a: 0,
        b: 0,
        c: 0,
    };
    assert!(matches!(
        instruction.opcode.control_operands(instruction),
        crate::ir::ControlOperands::Loop { .. }
    ));
    assert!(!super::is_osr_candidate(3, instruction));
}

#[test]
fn code_arena_lowers_constant_add_as_one_specialized_instruction() {
    let function = super::FunctionCode::from_ops(vec![
        super::Op::Const {
            dst: 1,
            value: crate::ops::Constant::Number(2.5),
        },
        super::Op::Binary {
            dst: 2,
            operator: crate::ops::BinaryOp::Add,
            lhs: 1,
            rhs: 0,
        },
        super::Op::Return { src: 2 },
    ]);
    let code = function.code().expect("materialized code");
    assert_eq!(code.len(), 2);
    let add = code.instruction(0).expect("specialized add");
    assert_eq!(add.opcode, crate::ir::Opcode::AddConst);
    assert_eq!(add.a, 2);
    assert_eq!(add.b, 0);
    assert!(add.add_const_is_left());
    assert_eq!(
        code.constant(add.c),
        Some(&crate::ops::Constant::Number(2.5))
    );
    assert_eq!(
        code.instruction(1).map(|instruction| instruction.opcode),
        Some(crate::ir::Opcode::Return)
    );
}

#[test]
fn code_arena_lowers_constant_add_in_either_operand_position() {
    let function = super::FunctionCode::from_ops(vec![
        super::Op::Const {
            dst: 1,
            value: crate::ops::Constant::Number(2.5),
        },
        super::Op::Binary {
            dst: 2,
            operator: crate::ops::BinaryOp::Add,
            lhs: 0,
            rhs: 1,
        },
        super::Op::Return { src: 2 },
    ]);
    let code = function.code().expect("materialized code");
    let add = code.instruction(0).expect("specialized add");
    assert_eq!(add.opcode, crate::ir::Opcode::AddConst);
    assert_eq!((add.a, add.b), (2, 0));
    assert!(!add.add_const_is_left());
}

#[test]
fn code_arena_serializes_structured_loop_as_typed_fori_marker() {
    let empty = || super::FunctionCode::from_ops(Vec::new());
    let function = super::FunctionCode::from_ops(vec![super::Op::Loop {
        label: Some("labeled".into()),
        init: empty(),
        test: empty(),
        body: empty(),
        update: empty(),
        post_test: false,
        dst: 0,
        per_iteration: Vec::new(),
    }]);
    let code = function.code().expect("structured loop code");
    let instruction = code.instruction(0).expect("ForI marker");
    assert_eq!(instruction.opcode, crate::ir::Opcode::ForI);
    assert!(instruction.opcode.is_typed_cold_marker());
    assert!(matches!(code.cold(instruction), Some(super::Op::Loop { .. })));
    let mut policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    policy.fused_regions = true;
    let baseline = super::BaselinePlan::compile_for_test(code, policy);
    let region = baseline.native_region_at(0).expect("ForI gateway admission");
    assert_eq!(
        region.borrow().key_for_test(),
        crate::stencil_select::for_i_region_key()
    );
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
#[test]
fn non_x86_baseline_does_not_admit_native_numeric_plan() {
    let function = super::FunctionCode::from_ops(vec![
        super::Op::Binary {
            dst: 2,
            operator: crate::ops::BinaryOp::Add,
            lhs: 0,
            rhs: 1,
        },
        super::Op::Return { src: 2 },
    ]);
    function.set_tier_threshold_for_test(1);
    function.retire(1);
    assert_eq!(
        function.enter_invocation(),
        super::TierTransition::CompileBaseline
    );
    let plan = function.baseline_plan().expect("baseline plan");
    assert!(plan.native_binary_at(0).is_none());
}

#[test]
fn native_add_const_rejects_constant_left_for_signed_zero_order() {
    let function = super::FunctionCode::from_ops(vec![
        super::Op::Const {
            dst: 1,
            value: crate::ops::Constant::Number(-0.0),
        },
        super::Op::Binary {
            dst: 2,
            operator: crate::ops::BinaryOp::Add,
            lhs: 1,
            rhs: 0,
        },
        super::Op::Return { src: 2 },
    ]);
    function.set_tier_threshold_for_test(1);
    function.retire(1);
    assert_eq!(
        function.enter_invocation(),
        super::TierTransition::CompileBaseline
    );
    let plan = function.baseline_plan().expect("baseline plan");
    assert!(plan.native_binary_at(0).is_none());
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
#[test]
fn non_x86_native_execution_rejects_before_mapping() {
    let mut plan = super::NativeBinaryPlan {
        physical: super::PhysicalInstallation::local(super::InstalledBinaryEntry::Unpublished),
        site: crate::quickening::QuickeningSite::new(crate::ir::Opcode::Add),
        opcode: crate::ir::Opcode::Add,
        key: crate::stencil_select::numeric_region_key(crate::ir::Opcode::Add).unwrap(),
        tagged_key: None,
        semantic: super::BinarySemantic::Numeric {
            returns_boolean: false,
        },
        compare_branch: None,
        native_entry_count: 0,
        last_native_view: None,
    };
    assert!(plan.execute(1.0, 2.0).is_err());
    assert!(plan.physical.storage.local().is_none());
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_numeric_entry_pointer_is_cached_after_first_render() {
    let mut plan = super::NativeBinaryPlan {
        physical: super::PhysicalInstallation::local(super::InstalledBinaryEntry::Unpublished),
        site: crate::quickening::QuickeningSite::new(crate::ir::Opcode::Add),
        opcode: crate::ir::Opcode::Add,
        key: crate::stencil_select::numeric_region_key(crate::ir::Opcode::Add).unwrap(),
        tagged_key: None,
        semantic: super::BinarySemantic::Numeric {
            returns_boolean: false,
        },
        compare_branch: None,
        native_entry_count: 0,
        last_native_view: None,
    };
    assert_eq!(plan.execute(1.5, 2.25), Ok(3.75));
    assert!(matches!(
        plan.physical.installed(),
        super::InstalledBinaryEntry::F64Local(_)
    ));
    let used = plan.physical.storage.used();
    assert_eq!(plan.execute(4.0, 5.0), Ok(9.0));
    assert_eq!(plan.physical.storage.used(), used);
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_numeric_shared_entry_reuses_live_owner_and_recovers_after_eviction() {
    let instruction = crate::ir::Instruction {
        opcode: crate::ir::Opcode::Add,
        flags: crate::ir::compact_binary_id(crate::ops::BinaryOp::Add),
        a: 0,
        b: 1,
        c: 2,
    };
    let shared = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("slab"),
    ));
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut plan = super::NativeBinaryPlan::new_with_shared(instruction, policy, shared.clone())
        .expect("shared numeric plan");
    assert_eq!(plan.execute(1.5, 2.25), Ok(3.75));
    let used = shared.borrow().used();
    assert!(matches!(
        plan.physical.installed(),
        super::InstalledBinaryEntry::F64Shared(_)
    ));
    assert_eq!(plan.execute(4.0, 5.0), Ok(9.0));
    assert_eq!(shared.borrow().used(), used);
    assert_eq!(shared.borrow_mut().evict_idle(0), 1);
    assert_eq!(plan.execute(4.0, 5.0), Ok(9.0));
    assert!(
        matches!(
            plan.physical.installed(),
            super::InstalledBinaryEntry::F64Shared(_)
        ),
        "eviction must rebuild the entry"
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_strict_numeric_equality_uses_typed_scalar_entry() {
    let instruction = crate::ir::Instruction {
        opcode: crate::ir::Opcode::Binary,
        flags: crate::ir::compact_binary_id(crate::ops::BinaryOp::StrictEqual),
        a: 0,
        b: 1,
        c: 2,
    };
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut plan = super::NativeBinaryPlan::new(instruction, policy).expect("scalar compare");
    assert!(plan.returns_boolean());
    assert_eq!(plan.execute(2.0, 2.0), Ok(1.0));
    assert_eq!(plan.execute(2.0, 3.0), Ok(0.0));
    let mut not_equal = super::NativeBinaryPlan::new(
        crate::ir::Instruction {
            flags: crate::ir::compact_binary_id(crate::ops::BinaryOp::StrictNotEqual),
            ..instruction
        },
        policy,
    )
    .expect("scalar inequality");
    assert!(not_equal.returns_boolean());
    assert_eq!(not_equal.execute(2.0, 2.0), Ok(0.0));
    assert_eq!(not_equal.execute(f64::NAN, f64::NAN), Ok(1.0));
    for (operator, lhs, rhs, expected) in [
        (crate::ops::BinaryOp::LessThan, 1.0, 2.0, 1.0),
        (crate::ops::BinaryOp::LessEqual, 2.0, 2.0, 1.0),
        (crate::ops::BinaryOp::GreaterThan, 3.0, 2.0, 1.0),
        (crate::ops::BinaryOp::GreaterEqual, 2.0, 2.0, 1.0),
    ] {
        let mut ordered = super::NativeBinaryPlan::new(
            crate::ir::Instruction {
                flags: crate::ir::compact_binary_id(operator),
                ..instruction
            },
            policy,
        )
        .expect("ordered scalar compare");
        assert_eq!(ordered.execute(lhs, rhs), Ok(expected));
        assert_eq!(
            ordered.execute(f64::NAN, rhs),
            Ok(0.0),
            "operator {operator:?}"
        );
    }

    // Dedicated comparison rows use the same generated physical mapping as
    // legacy flagged Binary, so future lowering can switch representations
    // without losing native admission.
    for opcode in [crate::ir::Opcode::Equal, crate::ir::Opcode::StrictEqual] {
        let mut dedicated = super::NativeBinaryPlan::new(
            crate::ir::Instruction {
                opcode,
                flags: 0,
                ..instruction
            },
            policy,
        )
        .expect("dedicated comparison row");
        assert!(dedicated.returns_boolean());
        assert_eq!(dedicated.execute(2.0, 2.0), Ok(1.0));
        assert_eq!(dedicated.execute(2.0, 3.0), Ok(0.0));
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_shared_boolean_entry_rebuilds_through_typed_owner() {
    let instruction = crate::ir::Instruction {
        opcode: crate::ir::Opcode::Binary,
        flags: crate::ir::compact_binary_id(crate::ops::BinaryOp::LessThan),
        a: 0,
        b: 1,
        c: 2,
    };
    let shared = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("slab"),
    ));
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut plan = super::NativeBinaryPlan::new_with_shared(instruction, policy, shared.clone())
        .expect("shared boolean plan");
    assert_eq!(plan.execute(1.0, 2.0), Ok(1.0));
    assert!(matches!(
        plan.physical.installed(),
        super::InstalledBinaryEntry::BoolShared(_)
    ));
    let used = shared.borrow().used();
    assert_eq!(plan.execute(2.0, 3.0), Ok(1.0));
    assert_eq!(
        shared.borrow().used(),
        used,
        "owner hit must not render again"
    );
    assert_eq!(shared.borrow_mut().evict_idle(0), 1);
    assert_eq!(plan.execute(2.0, 1.0), Ok(0.0));
    assert!(matches!(
        plan.physical.installed(),
        super::InstalledBinaryEntry::BoolShared(_)
    ));
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_numeric_equality_covers_loose_numbers_and_falls_back_for_coercion() {
    let program = crate::reduce::reduce_source("var left = 7; var right = 7; left == right;")
        .expect("loose equality source lowers");
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut executed = false;
    let mut inspect = |view: crate::machine::CodeView<'_>| {
        if executed {
            return;
        }
        let Some(pc) = (0..view.len()).find(|pc| {
            view.instruction(*pc).is_some_and(|instruction| {
                instruction.opcode.binary_operator(instruction.flags)
                    == Some(crate::ops::BinaryOp::Equal)
            })
        }) else {
            return;
        };
        let plan = super::BaselinePlan::compile_for_test(view, policy);
        let native = plan.native_binary_at(pc).expect("numeric equality leaf");
        let instruction = view.instruction(pc).expect("equality instruction");
        let mut registers = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        registers.write(usize::from(instruction.b), crate::value::Value::Number(7.0));
        registers.write(usize::from(instruction.c), crate::value::Value::Number(7.0));
        let context = crate::vm::current_context_or_default();
        let environment = crate::environment::Environment::new();
        crate::vm::execute_baseline_code_from(
            view,
            &plan,
            pc,
            &mut registers,
            &context,
            std::rc::Rc::clone(&environment),
        )
        .expect("numeric equality execution");
        assert_eq!(
            registers.read(usize::from(instruction.a)),
            Some(crate::value::Value::Boolean(true))
        );
        let before = native.borrow().native_entry_count;
        assert!(before > 0, "numeric equality must execute emitted bytes");

        let mut hostile = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        hostile.write(
            usize::from(instruction.b),
            crate::value::Value::String("7".into()),
        );
        hostile.write(usize::from(instruction.c), crate::value::Value::Number(7.0));
        crate::vm::execute_baseline_code_from(view, &plan, pc, &mut hostile, &context, environment)
            .expect("coercive equality fallback");
        assert_eq!(
            hostile.read(usize::from(instruction.a)),
            Some(crate::value::Value::Boolean(true))
        );
        assert_eq!(
            native.borrow().native_entry_count,
            before,
            "string coercion must remain on the canonical path"
        );
        let mut not_equal = super::NativeBinaryPlan::new(
            crate::ir::Instruction {
                opcode: crate::ir::Opcode::Binary,
                flags: crate::ir::compact_binary_id(crate::ops::BinaryOp::NotEqual),
                a: 0,
                b: 1,
                c: 2,
            },
            policy,
        )
        .expect("numeric inequality leaf");
        assert_eq!(not_equal.execute(7.0, 8.0), Ok(1.0));
        assert_eq!(not_equal.execute(f64::NAN, 7.0), Ok(1.0));
        executed = true;
    };
    inspect(program.code());
    program.code().cold_ops().for_each(|(_, op)| {
        op.visit_bodies(&mut |body| {
            if let Some(view) = body.code() {
                inspect(view);
            }
        });
    });
    assert!(
        executed,
        "ordinary source must reach loose numeric equality"
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_bitwise_i32_regions_guard_number_conversion() {
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let instruction = crate::ir::Instruction {
        opcode: crate::ir::Opcode::Binary,
        flags: 0,
        a: 0,
        b: 1,
        c: 2,
    };
    for (operator, lhs, rhs, expected) in [
        (
            crate::ops::BinaryOp::BitwiseAnd,
            0xF0F0_i32,
            0x0FF0_i32,
            0x00F0_i32,
        ),
        (
            crate::ops::BinaryOp::BitwiseOr,
            0xF000_i32,
            0x00F0_i32,
            0xF0F0_i32,
        ),
        (
            crate::ops::BinaryOp::BitwiseXor,
            -1_i32,
            0x0F0F_i32,
            !0x0F0F_i32,
        ),
        (crate::ops::BinaryOp::ShiftLeft, 1_i32, 32_i32, 1_i32),
        (crate::ops::BinaryOp::ShiftRight, -8_i32, 1_i32, -4_i32),
        (
            crate::ops::BinaryOp::ShiftRightZeroFill,
            -1_i32,
            1_i32,
            2_147_483_647_i32,
        ),
    ] {
        let mut plan = super::NativeBinaryPlan::new(
            crate::ir::Instruction {
                flags: crate::ir::compact_binary_id(operator),
                ..instruction
            },
            policy,
        )
        .expect("i32 bitwise region");
        assert_eq!(
            plan.execute(f64::from(lhs), f64::from(rhs)),
            Ok(f64::from(expected))
        );
        let converted = crate::intl::tolocale::value::to_int32(1.5);
        let right = crate::intl::tolocale::value::to_int32(f64::from(rhs));
        let expected_fraction = match operator {
            crate::ops::BinaryOp::BitwiseAnd => converted & right,
            crate::ops::BinaryOp::BitwiseOr => converted | right,
            crate::ops::BinaryOp::BitwiseXor => converted ^ right,
            crate::ops::BinaryOp::ShiftLeft => converted.wrapping_shl((right as u32) & 31),
            crate::ops::BinaryOp::ShiftRight => converted.wrapping_shr((right as u32) & 31),
            crate::ops::BinaryOp::ShiftRightZeroFill => {
                ((converted as u32) >> ((right as u32) & 31)) as i32
            }
            _ => unreachable!(),
        };
        assert_eq!(
            plan.execute(1.5, f64::from(rhs)),
            Ok(f64::from(expected_fraction))
        );
        assert_eq!(
            plan.execute(f64::NAN, f64::from(rhs)),
            Ok(f64::from(match operator {
                crate::ops::BinaryOp::BitwiseAnd => 0,
                crate::ops::BinaryOp::BitwiseOr => right,
                crate::ops::BinaryOp::BitwiseXor => right,
                crate::ops::BinaryOp::ShiftLeft => 0,
                crate::ops::BinaryOp::ShiftRight => 0,
                crate::ops::BinaryOp::ShiftRightZeroFill => 0,
                _ => unreachable!(),
            }))
        );
        assert!(
            matches!(
                plan.physical.installed(),
                super::InstalledBinaryEntry::I32Local(_) | super::InstalledBinaryEntry::U32Local(_)
            ),
            "conversion cases must reach the rendered typed entry"
        );
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_unsigned_shift_preserves_uint32_number_representation() {
    let instruction = crate::ir::Instruction {
        opcode: crate::ir::Opcode::Binary,
        flags: crate::ir::compact_binary_id(crate::ops::BinaryOp::ShiftRightZeroFill),
        a: 0,
        b: 1,
        c: 2,
    };
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut plan = super::NativeBinaryPlan::new(instruction, policy).expect("unsigned shift");
    assert_eq!(plan.execute(-1.0, 0.0), Ok(4_294_967_295.0));
    assert_eq!(plan.execute(-1.0, 1.0), Ok(2_147_483_647.0));
    assert_eq!(plan.execute(-1.0, 33.5), Ok(2_147_483_647.0));
    assert!(plan.native_entry_count >= 3);
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_shared_integer_entry_rebuilds_through_typed_owner() {
    let instruction = crate::ir::Instruction {
        opcode: crate::ir::Opcode::Binary,
        flags: crate::ir::compact_binary_id(crate::ops::BinaryOp::ShiftRightZeroFill),
        a: 0,
        b: 1,
        c: 2,
    };
    let shared = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("slab"),
    ));
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut plan = super::NativeBinaryPlan::new_with_shared(instruction, policy, shared.clone())
        .expect("shared integer plan");
    assert_eq!(plan.execute(-1.0, 1.0), Ok(2_147_483_647.0));
    assert!(matches!(
        plan.physical.installed(),
        super::InstalledBinaryEntry::U32Shared(_)
    ));
    assert_eq!(shared.borrow_mut().evict_idle(0), 1);
    assert_eq!(plan.execute(-1.0, 33.5), Ok(2_147_483_647.0));
    assert!(matches!(
        plan.physical.installed(),
        super::InstalledBinaryEntry::U32Shared(_)
    ));
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_bitwise_not_i32_entry_preserves_to_int32_rules() {
    let instruction = crate::ir::Instruction {
        opcode: crate::ir::Opcode::Unary,
        flags: crate::ir::compact_unary_id(crate::ops::UnaryOp::BitwiseNot),
        a: 0,
        b: 1,
        c: 0,
    };
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut plan = super::NativeUnaryPlan::new(instruction, policy).expect("bitwise-not leaf");
    for (input, expected) in [
        (0.0, -1.0),
        (1.5, -2.0),
        (f64::NAN, -1.0),
        (4_294_967_297.0, -2.0),
    ] {
        assert_eq!(plan.execute(input), Ok(expected));
    }
    assert!(plan.native_entry_count >= 4);
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_unary_shared_entry_reuses_live_owner_and_recovers_after_eviction() {
    let instruction = crate::ir::Instruction {
        opcode: crate::ir::Opcode::Unary,
        flags: crate::ir::compact_unary_id(crate::ops::UnaryOp::BitwiseNot),
        a: 0,
        b: 1,
        c: 0,
    };
    let shared = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("slab"),
    ));
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut plan = super::NativeUnaryPlan::new_with_shared(instruction, policy, shared.clone())
        .expect("shared unary plan");
    assert_eq!(plan.execute(1.5), Ok(-2.0));
    let used = shared.borrow().used();
    assert!(matches!(
        plan.physical.installed(),
        super::InstalledUnaryEntry::NumberShared(_) | super::InstalledUnaryEntry::IntegerShared(_)
    ));
    assert_eq!(plan.execute(1.5), Ok(-2.0));
    assert_eq!(shared.borrow().used(), used);
    assert_eq!(shared.borrow_mut().evict_idle(0), 1);
    assert_eq!(plan.execute(1.5), Ok(-2.0));
    assert!(
        matches!(
            plan.physical.installed(),
            super::InstalledUnaryEntry::NumberShared(_)
                | super::InstalledUnaryEntry::IntegerShared(_)
        ),
        "eviction must rebuild the entry"
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn ordinary_source_lowering_executes_bitwise_not_and_falls_back_for_string() {
    let program =
        crate::reduce::reduce_source("var x = 1; x = ~x; x;").expect("bitwise-not source lowers");
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut executed = false;
    let mut inspect = |view: crate::machine::CodeView<'_>| {
        if executed {
            return;
        }
        let Some(pc) = (0..view.len()).find(|pc| {
            view.instruction(*pc).is_some_and(|instruction| {
                instruction.opcode == crate::ir::Opcode::Unary
                    && crate::ir::compact_unary_operator(instruction.flags)
                        == Some(crate::ops::UnaryOp::BitwiseNot)
            })
        }) else {
            return;
        };
        let plan = super::BaselinePlan::compile_for_test(view, policy);
        let Some(native) = plan.native_unary_at(pc) else {
            return;
        };
        let instruction = view.instruction(pc).expect("unary instruction");
        let mut registers = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        registers.write(usize::from(instruction.b), crate::value::Value::Number(1.0));
        let context = crate::vm::current_context_or_default();
        let environment = crate::environment::Environment::new();
        assert!(crate::vm::execute_baseline_code_from(
            view,
            &plan,
            pc,
            &mut registers,
            &context,
            std::rc::Rc::clone(&environment),
        )
        .is_ok());
        assert_eq!(
            registers.read(usize::from(instruction.a)),
            Some(crate::value::Value::Number(-2.0))
        );
        let before = native.borrow().native_entry_count;
        let mut hostile = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        hostile.write(
            usize::from(instruction.b),
            crate::value::Value::String("1".into()),
        );
        assert!(crate::vm::execute_baseline_code_from(
            view,
            &plan,
            pc,
            &mut hostile,
            &context,
            environment,
        )
        .is_ok());
        assert_eq!(
            hostile.read(usize::from(instruction.a)),
            Some(crate::value::Value::Number(-2.0))
        );
        assert_eq!(native.borrow().native_entry_count, before);
        executed = true;
    };
    inspect(program.code());
    program.code().cold_ops().for_each(|(_, op)| {
        op.visit_bodies(&mut |body| {
            if let Some(view) = body.code() {
                inspect(view);
            }
        });
    });
    assert!(executed, "ordinary source must execute the unary stencil");
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn ordinary_source_lowering_executes_numeric_negate_and_preserves_signed_zero() {
    let program = crate::reduce::reduce_source("var x = 3; x = -x; x;")
        .expect("numeric negate source lowers");
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut executed = false;
    let mut inspect = |view: crate::machine::CodeView<'_>| {
        if executed {
            return;
        }
        let Some(pc) = (0..view.len()).find(|pc| {
            view.instruction(*pc).is_some_and(|instruction| {
                instruction.opcode == crate::ir::Opcode::Unary
                    && crate::ir::compact_unary_operator(instruction.flags)
                        == Some(crate::ops::UnaryOp::Minus)
            })
        }) else {
            return;
        };
        let plan = super::BaselinePlan::compile_for_test(view, policy);
        let native = plan.native_unary_at(pc).expect("numeric negate leaf");
        let instruction = view.instruction(pc).expect("negate instruction");
        let mut registers = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        registers.write(usize::from(instruction.b), crate::value::Value::Number(3.0));
        let context = crate::vm::current_context_or_default();
        let environment = crate::environment::Environment::new();
        crate::vm::execute_baseline_code_from(
            view,
            &plan,
            pc,
            &mut registers,
            &context,
            std::rc::Rc::clone(&environment),
        )
        .expect("numeric negate execution");
        assert_eq!(
            registers.read(usize::from(instruction.a)),
            Some(crate::value::Value::Number(-3.0))
        );
        let before = native.borrow().native_entry_count;
        assert!(before > 0, "numeric negate must execute emitted bytes");

        let mut hostile = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        hostile.write(
            usize::from(instruction.b),
            crate::value::Value::String("3".into()),
        );
        crate::vm::execute_baseline_code_from(view, &plan, pc, &mut hostile, &context, environment)
            .expect("coercive negate fallback");
        assert_eq!(
            hostile.read(usize::from(instruction.a)),
            Some(crate::value::Value::Number(-3.0))
        );
        assert_eq!(native.borrow().native_entry_count, before);

        let mut signed_zero = super::NativeUnaryPlan::new(
            crate::ir::Instruction {
                opcode: crate::ir::Opcode::Unary,
                flags: crate::ir::compact_unary_id(crate::ops::UnaryOp::Minus),
                a: 0,
                b: 1,
                c: 0,
            },
            policy,
        )
        .expect("direct numeric negate leaf");
        let negated_zero = signed_zero.execute(0.0).expect("negate +0");
        let restored_zero = signed_zero.execute(-0.0).expect("negate -0");
        assert_eq!(negated_zero.to_bits(), (-0.0f64).to_bits());
        assert_eq!(restored_zero.to_bits(), 0.0f64.to_bits());
        executed = true;
    };
    inspect(program.code());
    program.code().cold_ops().for_each(|(_, op)| {
        op.visit_bodies(&mut |body| {
            if let Some(view) = body.code() {
                inspect(view);
            }
        });
    });
    assert!(executed, "ordinary source must reach numeric negate");
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn ordinary_source_lowering_executes_logical_not_with_complete_fallback() {
    let program =
        crate::reduce::reduce_source("var x = 0; x = !x; x;").expect("logical-not source lowers");
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut executed = false;
    let mut inspect = |view: crate::machine::CodeView<'_>| {
        if executed {
            return;
        }
        let Some(pc) = (0..view.len()).find(|pc| {
            view.instruction(*pc).is_some_and(|instruction| {
                instruction.opcode == crate::ir::Opcode::Unary
                    && crate::ir::compact_unary_operator(instruction.flags)
                        == Some(crate::ops::UnaryOp::Not)
            })
        }) else {
            return;
        };
        let plan = super::BaselinePlan::compile_for_test(view, policy);
        let native = plan
            .native_truthiness_at(pc)
            .expect("logical-not truthiness plan");
        let instruction = view.instruction(pc).expect("logical-not instruction");
        let mut registers = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        registers.write(usize::from(instruction.b), crate::value::Value::Number(0.0));
        let context = crate::vm::current_context_or_default();
        let environment = crate::environment::Environment::new();
        crate::vm::execute_baseline_code_from(
            view,
            &plan,
            pc,
            &mut registers,
            &context,
            std::rc::Rc::clone(&environment),
        )
        .expect("logical-not numeric execution");
        assert_eq!(
            registers.read(usize::from(instruction.a)),
            Some(crate::value::Value::Boolean(true))
        );
        assert!(
            native.borrow().native_entry_count > 0,
            "logical-not number must execute emitted bytes"
        );
        assert_eq!(native.borrow_mut().execute(2.0), Ok(true));
        assert_eq!(native.borrow_mut().execute(f64::NAN), Ok(false));
        let before = native.borrow().native_entry_count;

        let mut hostile = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        hostile.write(
            usize::from(instruction.b),
            crate::value::Value::String(String::new()),
        );
        crate::vm::execute_baseline_code_from(view, &plan, pc, &mut hostile, &context, environment)
            .expect("logical-not coercive fallback");
        assert_eq!(
            hostile.read(usize::from(instruction.a)),
            Some(crate::value::Value::Boolean(true))
        );
        assert_eq!(native.borrow().native_entry_count, before);
        let mut boolean_input = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        boolean_input.write(
            usize::from(instruction.b),
            crate::value::Value::Boolean(true),
        );
        crate::vm::execute_baseline_code_from(
            view,
            &plan,
            pc,
            &mut boolean_input,
            &context,
            crate::environment::Environment::new(),
        )
        .expect("logical-not boolean execution");
        assert_eq!(
            boolean_input.read(usize::from(instruction.a)),
            Some(crate::value::Value::Boolean(false))
        );
        assert!(native.borrow().native_entry_count > before);
        executed = true;
    };
    inspect(program.code());
    program.code().cold_ops().for_each(|(_, op)| {
        op.visit_bodies(&mut |body| {
            if let Some(view) = body.code() {
                inspect(view);
            }
        });
    });
    assert!(executed, "ordinary source must reach logical-not");
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn ordinary_source_lowering_executes_numeric_truthiness_and_falls_back_for_string() {
    let program = crate::reduce::reduce_source("var x = 0; if (x) { x = 1; } x;")
        .expect("conditional source lowers");
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut checked = false;
    let mut inspect = |view: crate::machine::CodeView<'_>| {
        if checked {
            return;
        }
        let Some(pc) = (0..view.len()).find(|pc| {
            view.instruction(*pc)
                .is_some_and(|instruction| instruction.opcode == crate::ir::Opcode::JumpIfFalse)
        }) else {
            return;
        };
        let plan = super::BaselinePlan::compile_for_test(view, policy);
        let native = plan.native_truthiness_at(pc).expect("truthiness leaf");
        let instruction = view.instruction(pc).expect("branch instruction");
        let mut registers = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        registers.write(usize::from(instruction.a), crate::value::Value::Number(0.0));
        let context = crate::vm::current_context_or_default();
        assert!(crate::vm::execute_baseline_code_from(
            view,
            &plan,
            pc,
            &mut registers,
            &context,
            crate::environment::Environment::new(),
        )
        .is_ok());
        let before = native.borrow().native_entry_count;
        assert!(
            before > 0,
            "numeric branch must execute emitted truthiness bytes"
        );
        let mut hostile = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        hostile.write(
            usize::from(instruction.a),
            crate::value::Value::String("truthy".into()),
        );
        assert!(crate::vm::execute_baseline_code_from(
            view,
            &plan,
            pc,
            &mut hostile,
            &context,
            crate::environment::Environment::new(),
        )
        .is_ok());
        assert_eq!(native.borrow().native_entry_count, before);
        checked = true;
    };
    inspect(program.code());
    program.code().cold_ops().for_each(|(_, op)| {
        op.visit_bodies(&mut |body| {
            if let Some(view) = body.code() {
                inspect(view);
            }
        });
    });
    assert!(
        checked,
        "ordinary source must execute the numeric truthiness leaf"
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn ordinary_source_lowering_executes_tagged_truthiness_for_boolean() {
    let program = crate::reduce::reduce_source("var x = true; if (x) { x = false; } x;")
        .expect("boolean conditional source lowers");
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut executed = false;
    let mut inspect = |view: crate::machine::CodeView<'_>| {
        if executed {
            return;
        }
        let Some(pc) = (0..view.len()).find(|pc| {
            view.instruction(*pc)
                .is_some_and(|instruction| instruction.opcode == crate::ir::Opcode::JumpIfFalse)
        }) else {
            return;
        };
        let plan = super::BaselinePlan::compile_for_test(view, policy);
        let native = plan.native_truthiness_at(pc).expect("word truthiness leaf");
        let instruction = view.instruction(pc).expect("branch instruction");
        let mut registers = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        registers.write(
            usize::from(instruction.a),
            crate::value::Value::Boolean(true),
        );
        let context = crate::vm::current_context_or_default();
        assert!(crate::vm::execute_baseline_code_from(
            view,
            &plan,
            pc,
            &mut registers,
            &context,
            crate::environment::Environment::new(),
        )
        .is_ok());
        assert!(native.borrow().native_entry_count > 0);
        let mut hostile = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        hostile.write(
            usize::from(instruction.a),
            crate::value::Value::String("truthy".into()),
        );
        let before = native.borrow().native_entry_count;
        assert!(crate::vm::execute_baseline_code_from(
            view,
            &plan,
            pc,
            &mut hostile,
            &context,
            crate::environment::Environment::new(),
        )
        .is_ok());
        assert_eq!(native.borrow().native_entry_count, before);
        executed = true;
    };
    inspect(program.code());
    program.code().cold_ops().for_each(|(_, op)| {
        op.visit_bodies(&mut |body| {
            if let Some(view) = body.code() {
                inspect(view);
            }
        });
    });
    assert!(
        executed,
        "ordinary source must execute tagged truthiness bytes"
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn ordinary_source_lowering_executes_pointer_truthiness_for_object() {
    let program = crate::reduce::reduce_source("var x = {}; if (x) x = 1; x;")
        .expect("object conditional source lowers");
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut executed = false;
    let mut inspect = |view: crate::machine::CodeView<'_>| {
        if executed {
            return;
        }
        let Some(pc) = (0..view.len()).find(|pc| {
            view.instruction(*pc)
                .is_some_and(|instruction| instruction.opcode == crate::ir::Opcode::JumpIfFalse)
        }) else {
            return;
        };
        let plan = super::BaselinePlan::compile_for_test(view, policy);
        let native = plan
            .native_truthiness_at(pc)
            .expect("pointer truthiness leaf");
        let instruction = view.instruction(pc).expect("object branch instruction");
        let object = std::rc::Rc::new(crate::value::ObjectData::new(Vec::new()));
        let mut registers = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        registers.write(
            usize::from(instruction.a),
            crate::value::Value::Object(object),
        );
        let context = crate::vm::current_context_or_default();
        assert!(crate::vm::execute_baseline_code_from(
            view,
            &plan,
            pc,
            &mut registers,
            &context,
            crate::environment::Environment::new(),
        )
        .is_ok());
        assert!(native.borrow().native_entry_count > 0);
        let before = native.borrow().native_entry_count;
        let mut hostile = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        hostile.write(
            usize::from(instruction.a),
            crate::value::Value::String("truthy".into()),
        );
        assert!(crate::vm::execute_baseline_code_from(
            view,
            &plan,
            pc,
            &mut hostile,
            &context,
            crate::environment::Environment::new(),
        )
        .is_ok());
        assert_eq!(native.borrow().native_entry_count, before);
        executed = true;
    };
    inspect(program.code());
    program.code().cold_ops().for_each(|(_, op)| {
        op.visit_bodies(&mut |body| {
            if let Some(view) = body.code() {
                inspect(view);
            }
        });
    });
    assert!(
        executed,
        "ordinary source must execute pointer truthiness bytes"
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn primitive_load_const_uses_rendered_machine_word_and_preserves_value() {
    let code = crate::machine::ExecutableCode::from_ops(vec![
        crate::ops::Op::Const {
            dst: 0,
            value: crate::ops::Constant::Number(42.5),
        },
        crate::ops::Op::Return { src: 0 },
    ]);
    let view = code.code();
    let load = view
        .instruction(0)
        .expect("constant lowering emits LoadConst");
    assert_eq!(load.opcode, crate::ir::Opcode::LoadConst);
    let plan = super::BaselinePlan::compile_for_test(
        view,
        crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
    );
    let native = plan
        .native_load_const_at(0)
        .expect("primitive constant leaf");
    let mut registers = crate::register_file::RegisterFile::with_undefined(4);
    let context = crate::vm::current_context_or_default();
    assert!(crate::vm::execute_baseline_code_from(
        view,
        &plan,
        0,
        &mut registers,
        &context,
        crate::environment::Environment::new(),
    )
    .is_ok());
    assert_eq!(registers.read(0), Some(crate::value::Value::Number(42.5)));
    assert!(native.borrow_mut().execute().is_ok());
    assert!(
        native.borrow().native_entry_count > 0,
        "constant bytes must execute"
    );

    let string_code = crate::machine::ExecutableCode::from_ops(vec![
        crate::ops::Op::Const {
            dst: 0,
            value: crate::ops::Constant::String("heap-owned".into()),
        },
        crate::ops::Op::Return { src: 0 },
    ]);
    let string_plan = super::BaselinePlan::compile_for_test(
        string_code.code(),
        crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
    );
    assert!(
        string_plan.native_load_const_at(0).is_none(),
        "heap-owning constants must use the complete canonical loader"
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn shared_constant_entry_recovers_after_owner_eviction() {
    let shared = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("slab"),
    ));
    let mut plan = super::NativeLoadConstPlan::new_with_shared(
        crate::native_core::value_word::TaggedValue::number(42.5).bits(),
        crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
        shared.clone(),
    )
    .expect("shared constant plan");
    assert_eq!(
        plan.execute(),
        Ok(crate::native_core::value_word::TaggedValue::number(42.5).bits())
    );
    assert!(matches!(
        plan.physical.installed(),
        super::InstalledConstantEntry::Shared(_)
    ));
    assert_eq!(shared.borrow_mut().evict_idle(0), 1);
    assert_eq!(
        plan.execute(),
        Ok(crate::native_core::value_word::TaggedValue::number(42.5).bits())
    );
    assert!(
        matches!(
            plan.physical.installed(),
            super::InstalledConstantEntry::Shared(_)
        ),
        "eviction must force a fresh owner"
    );
}

#[test]
fn region_verifier_rejects_physical_call_for_raw_abi() {
    #[cfg(target_arch = "aarch64")]
    assert!(crate::stencil_physical::contains_call(&[
        0x00, 0x00, 0x00, 0x94
    ]));
    #[cfg(target_arch = "aarch64")]
    assert!(crate::stencil_physical::contains_call(&[
        0x00, 0x00, 0x3F, 0xD6
    ]));
    #[cfg(target_arch = "x86_64")]
    assert!(crate::stencil_physical::contains_call(&[0xE8, 0, 0, 0, 0]));
    #[cfg(target_arch = "x86_64")]
    assert!(crate::stencil_physical::contains_call(&[
        0x90, 0xE8, 0, 0, 0, 0
    ]));
    #[cfg(target_arch = "x86_64")]
    assert!(crate::stencil_physical::contains_call(&[0x41, 0xFF, 0xD2]));
    #[cfg(target_arch = "x86_64")]
    assert!(!crate::stencil_physical::contains_call(&[0xFF, 0xE0]));
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    assert!(!crate::stencil_physical::contains_call(&[0xE8]));
    for key in [
        crate::stencil_select::array_loop_body_region_key(),
        crate::stencil_select::array_numeric_loop_region_key(),
    ] {
        let record = crate::stencil_select::select_region(key).expect("raw declaration");
        assert!(!crate::stencil_physical::contains_call(
            record.stencil.bytes
        ));
    }
}

#[test]
fn ordinary_source_lowering_admits_guarded_bitwise_region() {
    let program = crate::reduce::reduce_source("var x = 240; x = (x & 15) << 1; x;")
        .expect("bitwise source lowers");
    let code = program.code();
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let has_plan = |view: crate::machine::CodeView<'_>| {
        let plan = super::BaselinePlan::compile_for_test(view, policy);
        (0..view.len()).any(|pc| {
            view.instruction(pc).is_some_and(|instruction| {
                matches!(
                        instruction.opcode.binary_operator(instruction.flags),
                        Some(crate::ops::BinaryOp::BitwiseAnd)
                    )
                    && plan.native_binary_at(pc).is_some()
            })
        })
    };
    let mut admitted = has_plan(code);
    code.cold_ops().for_each(|(_, op)| {
        op.visit_bodies(&mut |body| {
            let Some(view) = body.code() else { return };
            admitted |= has_plan(view);
        });
    });
    assert!(
        admitted,
        "ordinary source must reach the guarded bitwise plan"
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn ordinary_source_lowering_executes_bitwise_region_and_falls_back_on_conversion() {
    let program = crate::reduce::reduce_source("var x = 240; x = (x & 15) << 1; x;")
        .expect("bitwise source lowers");
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut executed = false;
    let mut inspect = |view: crate::machine::CodeView<'_>| {
        if executed {
            return;
        }
        let Some(binary_pc) = (0..view.len()).find(|pc| {
            view.instruction(*pc).is_some_and(|instruction| {
                instruction.opcode.binary_operator(instruction.flags)
                    == Some(crate::ops::BinaryOp::BitwiseAnd)
            })
        }) else {
            return;
        };
        let Some(shift_pc) = (0..view.len()).find(|pc| {
            view.instruction(*pc).is_some_and(|instruction| {
                instruction.opcode.binary_operator(instruction.flags)
                    == Some(crate::ops::BinaryOp::ShiftLeft)
            })
        }) else {
            return;
        };
        let plan = super::BaselinePlan::compile_for_test(view, policy);
        let bitwise = plan.native_binary_at(binary_pc).expect("bitwise plan");
        let shift = plan.native_binary_at(shift_pc).expect("shift plan");
        let fused_bitwise = plan.native_local_binary_at(binary_pc.saturating_sub(2));
        let run = |input: f64, expected: f64| {
            let mut registers = crate::register_file::RegisterFile::with_undefined(
                usize::from(view.register_count()).max(8),
            );
            let environment = crate::environment::Environment::new();
            environment.set(5, crate::value::Value::Number(input));
            let context = crate::vm::current_context_or_default();
            let result = crate::vm::execute_baseline_code_from(
                view,
                &plan,
                binary_pc.saturating_sub(2),
                &mut registers,
                &context,
                std::rc::Rc::clone(&environment),
            );
            assert!(result.is_ok(), "ordinary driver rejected bitwise fallback");
            assert_eq!(environment.get(5), crate::value::Value::Number(expected));
        };
        run(240.0, 0.0);
        run(1.5, 2.0);
        run(f64::NAN, 0.0);
        run(f64::INFINITY, 0.0);
        run(4_294_967_297.0, 2.0);
        assert!(
            bitwise.borrow().native_entry_count > 0
                || fused_bitwise
                    .as_ref()
                    .is_some_and(|plan| plan.borrow().native_entry_count() > 0)
        );
        assert!(shift.borrow().native_entry_count > 0);

        let before = bitwise.borrow().native_entry_count;
        let mut hostile = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        hostile.write(
            usize::from(view.instruction(binary_pc).expect("bitwise").b),
            crate::value::Value::String("240".into()),
        );
        hostile.write(
            usize::from(view.instruction(binary_pc).expect("bitwise").c),
            crate::value::Value::Number(15.0),
        );
        let environment = crate::environment::Environment::new();
        let context = crate::vm::current_context_or_default();
        crate::vm::execute_baseline_code_from(
            view,
            &plan,
            binary_pc,
            &mut hostile,
            &context,
            environment,
        )
        .expect("hostile conversion fallback");
        assert_eq!(bitwise.borrow().native_entry_count, before);
        executed = true;
    };
    inspect(program.code());
    program.code().cold_ops().for_each(|(_, op)| {
        op.visit_bodies(&mut |body| {
            if let Some(view) = body.code() {
                inspect(view);
            }
        });
    });
    assert!(
        executed,
        "ordinary driver must execute lowered bitwise bytes"
    );
}

#[cfg(target_arch = "aarch64")]
#[test]
fn ordinary_source_lowering_executes_fused_indexed_numeric_update() {
    let program = crate::reduce::reduce_source("var a = [3]; a[0] = a[0] + 2; a;")
        .expect("indexed update source lowers");
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut executed = false;
    let mut inspect = |view: crate::machine::CodeView<'_>| {
        if executed {
            return;
        }
        let Some(pc) = (0..view.len()).find(|pc| {
            view.instruction(*pc)
                .is_some_and(|instruction| instruction.opcode == crate::ir::Opcode::AGetI)
                && view.instruction(*pc + 1).is_some_and(|instruction| {
                    matches!(
                        instruction.opcode,
                        crate::ir::Opcode::Add | crate::ir::Opcode::AddConst
                    )
                })
                && view
                    .instruction(*pc + 2)
                    .is_some_and(|instruction| instruction.opcode == crate::ir::Opcode::ASetI)
        }) else {
            return;
        };
        let load = view.instruction(pc).expect("indexed load");
        let add = view.instruction(pc + 1).expect("indexed add");
        let store = view.instruction(pc + 2).expect("indexed store");
        let cold_plan = super::BaselinePlan::compile_for_test(view, policy);
        assert_holey_indexed_update_falls_back(view, &cold_plan, pc, load, add, store);
        assert!(!cold_plan
            .native_region_at(pc)
            .expect("cold region")
            .borrow()
            .physical_is_published_for_test());
        let plan = super::BaselinePlan::compile_for_test(view, policy);
        let region = plan.native_region_at(pc).expect("fused update admission");
        let expected_key = if add.opcode == crate::ir::Opcode::Add {
            crate::stencil_select::array_numeric_update_region_key()
        } else {
            crate::stencil_select::array_numeric_update_const_region_key()
        };
        assert_eq!(region.borrow().key_for_test(), expected_key);
        let control = region
            .borrow()
            .admitted_control_for_test()
            .expect("normal admission retains CFG control");
        assert_eq!((control.start(), control.end()), (pc, pc + 3));
        let mut registers = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        let array =
            crate::value::Value::Array(std::rc::Rc::new(crate::value::ArrayData::new(vec![
                crate::value::Value::Number(3.0),
            ])));
        registers.write(usize::from(load.b), array);
        registers.write(usize::from(load.c), crate::value::Value::Number(0.0));
        let array_word = registers.read(usize::from(load.b)).unwrap();
        registers.write(usize::from(store.a), array_word);
        registers.write(usize::from(store.b), crate::value::Value::Number(0.0));
        if add.opcode == crate::ir::Opcode::Add {
            registers.write(usize::from(add.c), crate::value::Value::Number(2.0));
        }
        let context = crate::vm::current_context_or_default();
        crate::vm::execute_baseline_code_from(
            view,
            &plan,
            pc,
            &mut registers,
            &context,
            crate::environment::Environment::new(),
        )
        .expect("ordinary fused update");
        assert_eq!(
            registers
                .read_array(usize::from(store.a))
                .and_then(|array| array.dense_number_at(0)),
            Some(5.0)
        );
        assert!(region.borrow().last_native_execution());
        let published = {
            let slab = plan.shared_region_arena.borrow();
            (slab.slab_count(), slab.used(), slab.capacity())
        };
        crate::vm::execute_baseline_code_from(
            view,
            &plan,
            pc,
            &mut registers,
            &context,
            crate::environment::Environment::new(),
        )
        .expect("warm composed update");
        assert_eq!(
            registers
                .read_array(usize::from(store.a))
                .and_then(|array| array.dense_number_at(0)),
            Some(7.0)
        );
        assert!(region.borrow().last_native_execution());
        let slab = plan.shared_region_arena.borrow();
        assert_eq!(
            (slab.slab_count(), slab.used(), slab.capacity()),
            published,
            "warm region must not render or allocate again"
        );
        drop(slab);
        assert!(
            plan.shared_region_arena.borrow_mut().evict_idle(0) >= 1,
            "composed owner should expose an evictable published slab"
        );
        crate::vm::execute_baseline_code_from(
            view,
            &plan,
            pc,
            &mut registers,
            &context,
            crate::environment::Environment::new(),
        )
        .expect("composed update after owner eviction");
        assert_eq!(
            registers
                .read_array(usize::from(store.a))
                .and_then(|array| array.dense_number_at(0)),
            Some(9.0)
        );
        assert!(region.borrow().last_native_execution());
        assert_holey_indexed_update_falls_back(view, &plan, pc, load, add, store);
        executed = true;
    };
    inspect(program.code());
    program.code().cold_ops().for_each(|(_, op)| {
        op.visit_bodies(&mut |body| {
            if let Some(view) = body.code() {
                inspect(view);
            }
        });
    });
    assert!(
        executed,
        "ordinary lowering must expose the fused update shape"
    );
}

#[cfg(target_arch = "aarch64")]
fn affine_i32_source_body(source: &str) -> (super::FunctionCode, usize) {
    let program = crate::reduce::reduce_source(source).expect("affine source lowers");
    let record =
        crate::stencil_select::select_region(crate::stencil_select::affine_i32_loop_region_key())
            .expect("affine declaration");
    let mut pending = Vec::new();
    program.code().cold_ops().for_each(|(_, op)| {
        op.visit_bodies(&mut |body| pending.push(body.clone()));
    });
    while let Some(body) = pending.pop() {
        let Some(code) = body.code() else { continue };
        let pc = (0..code.len()).find(|pc| {
            record
                .operations
                .iter()
                .enumerate()
                .all(|(offset, expected)| {
                    code.instruction(*pc + offset).is_some_and(|instruction| {
                        expected.matches_physical_contract(instruction.opcode)
                    })
                })
        });
        if let Some(pc) = pc {
            return (body, pc);
        }
        code.cold_ops().for_each(|(_, op)| {
            op.visit_bodies(&mut |nested| pending.push(nested.clone()));
        });
    }
    panic!("ordinary source contains affine loop window")
}

#[cfg(target_arch = "aarch64")]
fn affine_i32_environment(
    code: super::CodeView<'_>,
    pc: usize,
    value: f64,
    end: f64,
) -> std::rc::Rc<crate::environment::Environment> {
    let captures = crate::environment::Environment::new();
    let frame = crate::register_file::RegisterFile::with_undefined(usize::from(
        code.frame_register_count(),
    ));
    let environment = crate::environment::Environment::child_registers(&captures, frame);
    environment.set(code.instruction(pc).unwrap().b, super::Value::Number(0.0));
    environment.set(
        code.instruction(pc + 5).unwrap().b,
        super::Value::Number(value),
    );
    let current = std::rc::Rc::new(crate::value::ObjectData::new(vec![
        ("seed".into(), super::Value::Number(value)),
        ("n".into(), super::Value::Number(end)),
    ]));
    let object = crate::value::ObjectData::new(Vec::new());
    object.replace_with(current);
    environment.set(
        code.instruction(pc + 1).unwrap().b,
        super::Value::Object(std::rc::Rc::new(object)),
    );
    environment
}

#[cfg(all(target_arch = "aarch64", quench_generated_stencil_artifacts))]
#[test]
fn ordinary_source_executes_generated_affine_i32_loop_region() {
    let case = crate::test_execution_profile::ExecutionCase::load("affine_i32_loop");
    case.assert_standalone();
    let key = crate::stencil_select::affine_i32_loop_region_key();
    let record = crate::stencil_select::select_region(key).expect("affine declaration");
    let (function, pc) = affine_i32_source_body(case.source());
    let code = function.code().expect("linked affine function");
    let entries = super::baseline_entries(code);
    let windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = super::ControlFlowFacts::new(&entries, &windows);
    assert!(
        record.bindings_match_entries(&entries, pc),
        "affine operand bindings"
    );
    assert!(
        cfg.region_plan(&entries, pc, record.operations).is_some(),
        "affine control plan"
    );
    assert!(
        super::region_outputs_cover_exit(&entries, &cfg, pc, record),
        "affine live outputs"
    );
    let view = crate::stencil_select::select_physical(key).expect("affine physical view");
    assert!(view.generated, "generated affine view required");
    assert!(
        crate::stencil_physical::contains_interrupt_checkpoint(view.stencil.bytes),
        "generated affine checkpoint bytes: {:02x?}",
        view.stencil.bytes
    );
    assert_eq!(super::validate_physical_view(record, view.stencil), Ok(()));
    let plan = super::BaselinePlan::compile_for_test(
        code,
        crate::stencil_policy::ExecutionPolicy::arm_composed_opt_in_for_test(),
    );
    let region = plan.native_region_at(pc).expect("affine loop admission");
    assert_eq!(region.borrow().key_for_test(), key);
    let mut registers =
        crate::register_file::RegisterFile::with_undefined(usize::from(code.register_count()));
    assert_eq!(case.warmup(), 1, "this runner performs one declared warmup");
    crate::vm::execute_baseline_code_from(
        code,
        &plan,
        pc,
        &mut registers,
        &crate::vm::current_context_or_default(),
        affine_i32_environment(code, pc, 1.0, 4.0),
    )
    .expect("affine loop warmup");
    let completion = crate::vm::execute_baseline_code_from(
        code,
        &plan,
        pc,
        &mut registers,
        &crate::vm::current_context_or_default(),
        affine_i32_environment(code, pc, 1.0, 4.0),
    )
    .expect("normal driver executes affine loop")
    .0;
    assert_eq!(
        completion,
        crate::completion::Completion::Return(super::Value::Number(1_445_341.0))
    );
    case.assert(&super::Value::Number(1_445_341.0));
    assert!(region.borrow().last_native_execution());
    assert!(region
        .borrow()
        .last_native_view_for_test()
        .is_some_and(|view| view.generated));

    let completion = crate::vm::execute_baseline_code_from(
        code,
        &plan,
        pc,
        &mut registers,
        &crate::vm::current_context_or_default(),
        affine_i32_environment(code, pc, -0.0, 0.0),
    )
    .expect("negative zero falls back")
    .0;
    assert!(
        matches!(completion, crate::completion::Completion::Return(super::Value::Number(value)) if value == 0.0 && value.is_sign_negative())
    );
    assert!(!region.borrow().last_native_execution());
}

#[cfg(target_arch = "aarch64")]
fn assert_holey_indexed_update_falls_back(
    view: crate::machine::CodeView<'_>,
    plan: &super::BaselinePlan,
    pc: usize,
    load: crate::ir::Instruction,
    add: crate::ir::Instruction,
    store: crate::ir::Instruction,
) {
    let mut data = crate::value::ArrayData::new(vec![crate::value::Value::Number(4.0)]);
    data.delete_property("0");
    let array = std::rc::Rc::new(data);
    let mut registers = crate::register_file::RegisterFile::with_undefined(
        usize::from(view.register_count()).max(8),
    );
    registers.write(
        usize::from(load.b),
        crate::value::Value::Array(array.clone()),
    );
    registers.write(usize::from(load.c), crate::value::Value::Number(0.0));
    registers.write(
        usize::from(store.a),
        crate::value::Value::Array(array.clone()),
    );
    registers.write(usize::from(store.b), crate::value::Value::Number(0.0));
    if add.opcode == crate::ir::Opcode::Add {
        registers.write(usize::from(add.c), crate::value::Value::Number(2.0));
    }
    crate::vm::execute_baseline_code_from(
        view,
        plan,
        pc,
        &mut registers,
        &crate::vm::current_context_or_default(),
        crate::environment::Environment::new(),
    )
    .expect("holey indexed fallback");
    let value = crate::vm::get_property_result(&crate::value::Value::Array(array), "0")
        .expect("ordinary indexed result");
    assert!(matches!(value, crate::value::Value::Number(number) if number.is_nan()));
    assert!(!plan
        .native_region_at(pc)
        .unwrap()
        .borrow()
        .last_native_execution());
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn ordinary_source_lowering_executes_numeric_comparison_and_falls_back_on_conversion() {
    let program =
        crate::reduce::reduce_source("var x = 1; x = x < 2; x;").expect("comparison source lowers");
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut executed = false;
    let mut inspect = |view: crate::machine::CodeView<'_>| {
        if executed {
            return;
        }
        let Some(pc) = (0..view.len()).find(|pc| {
            view.instruction(*pc).is_some_and(|instruction| {
                instruction.opcode.binary_operator(instruction.flags)
                    == Some(crate::ops::BinaryOp::LessThan)
            })
        }) else {
            return;
        };
        let plan = super::BaselinePlan::compile_for_test(view, policy);
        let Some(native) = plan.native_binary_at(pc) else {
            return;
        };
        let instruction = view.instruction(pc).expect("comparison instruction");
        let mut registers = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        registers.write(usize::from(instruction.b), crate::value::Value::Number(1.0));
        registers.write(usize::from(instruction.c), crate::value::Value::Number(2.0));
        let environment = crate::environment::Environment::new();
        let context = crate::vm::current_context_or_default();
        let result = crate::vm::execute_baseline_code_from(
            view,
            &plan,
            pc,
            &mut registers,
            &context,
            std::rc::Rc::clone(&environment),
        );
        assert!(result.is_ok(), "numeric comparison should execute");
        assert_eq!(
            registers.read(usize::from(instruction.a)),
            Some(crate::value::Value::Boolean(true))
        );
        assert!(
            native.borrow().native_entry_count > 0,
            "comparison bytes must execute"
        );

        let before = native.borrow().native_entry_count;
        let mut hostile = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        hostile.write(
            usize::from(instruction.b),
            crate::value::Value::String("1".into()),
        );
        hostile.write(usize::from(instruction.c), crate::value::Value::Number(2.0));
        let fallback = crate::vm::execute_baseline_code_from(
            view,
            &plan,
            pc,
            &mut hostile,
            &context,
            environment,
        );
        assert!(fallback.is_ok(), "conversion must use complete fallback");
        assert_eq!(
            hostile.read(usize::from(instruction.a)),
            Some(crate::value::Value::Boolean(true))
        );
        assert_eq!(
            native.borrow().native_entry_count,
            before,
            "fallback must not enter native bytes"
        );
        executed = true;
    };
    inspect(program.code());
    program.code().cold_ops().for_each(|(_, op)| {
        op.visit_bodies(&mut |body| {
            if let Some(view) = body.code() {
                inspect(view);
            }
        });
    });
    assert!(
        executed,
        "ordinary source must reach and execute comparison bytes"
    );
}

#[cfg(quench_generated_stencil_artifacts)]
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn ordinary_source_lowering_executes_generated_add_const_view() {
    let program = crate::reduce::reduce_source("var x = 3.5; x = x + 2.25; x;")
        .expect("ordinary source lowers constant add");
    let mut executed = false;
    let mut inspect = |view: crate::machine::CodeView<'_>| {
        let Some(pc) = (0..view.len()).find(|pc| {
            view.instruction(*pc)
                .is_some_and(|instruction| instruction.opcode == crate::ir::Opcode::AddConst)
        }) else {
            return;
        };
        let plan = super::BaselinePlan::compile_for_test(
            view,
            crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
        );
        let native = plan.native_binary_at(pc).expect("AddConst plan");
        let instruction = view.instruction(pc).expect("AddConst instruction");
        let physical =
            crate::stencil_select::select_physical(crate::stencil_select::add_const_region_key())
                .expect("generated AddConst view");
        assert!(physical.generated);
        assert_eq!(physical.key, crate::stencil_select::add_const_region_key());
        let mut registers = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        registers.write_number(usize::from(instruction.b), 3.5);
        let context = crate::vm::current_context_or_default();
        crate::vm::execute_baseline_code_from(
            view,
            &plan,
            pc,
            &mut registers,
            &context,
            crate::environment::Environment::new(),
        )
        .expect("generated AddConst execution");
        assert_eq!(
            registers.read(usize::from(instruction.a)),
            Some(crate::value::Value::Number(5.75))
        );
        assert!(native.borrow().native_entry_count > 0);
        let witness = native
            .borrow()
            .last_native_view()
            .expect("invocation-local physical view witness");
        assert!(witness.generated);
        assert_eq!(witness.key, physical.key);
        assert_eq!(witness.abi, physical.abi);
        assert_eq!(witness.entry, physical.entry);
        assert_eq!(witness.artifact_id, physical.artifact_id);
        assert_eq!(witness.data, physical.data);
        assert_eq!(witness.compiler, physical.compiler);
        assert_eq!(witness.stencil.bytes, physical.stencil.bytes);
        assert_eq!(witness.stencil.holes, physical.stencil.holes);
        assert_eq!(witness.fingerprint, physical.fingerprint);
        executed = true;
    };
    inspect(program.code());
    program.code().cold_ops().for_each(|(_, op)| {
        op.visit_bodies(&mut |body| {
            if let Some(view) = body.code() {
                inspect(view);
            }
        });
    });
    assert!(
        executed,
        "ordinary source must execute generated AddConst bytes"
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn ordinary_source_lowering_executes_tagged_identity_comparison() {
    let program = crate::reduce::reduce_source("var x = true; x === true;")
        .expect("identity comparison source lowers");
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut executed = false;
    let mut inspect = |view: crate::machine::CodeView<'_>| {
        if executed {
            return;
        }
        let Some(pc) = (0..view.len()).find(|pc| {
            view.instruction(*pc).is_some_and(|instruction| {
                instruction.opcode.binary_operator(instruction.flags)
                    == Some(crate::ops::BinaryOp::StrictEqual)
            })
        }) else {
            return;
        };
        let plan = super::BaselinePlan::compile_for_test(view, policy);
        let Some(native) = plan.native_binary_at(pc) else {
            return;
        };
        let instruction = view.instruction(pc).expect("identity instruction");
        let mut registers = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        registers.write(
            usize::from(instruction.b),
            crate::value::Value::Boolean(true),
        );
        registers.write(
            usize::from(instruction.c),
            crate::value::Value::Boolean(true),
        );
        let environment = crate::environment::Environment::new();
        let context = crate::vm::current_context_or_default();
        let result = crate::vm::execute_baseline_code_from(
            view,
            &plan,
            pc,
            &mut registers,
            &context,
            std::rc::Rc::clone(&environment),
        );
        assert!(result.is_ok(), "tagged identity comparison should execute");
        assert_eq!(
            registers.read(usize::from(instruction.a)),
            Some(crate::value::Value::Boolean(true))
        );
        assert!(
            native.borrow().native_entry_count > 0,
            "identity bytes must execute"
        );

        let object = std::rc::Rc::new(crate::value::ObjectData::new(Vec::new()));
        let mut object_registers = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        object_registers.write(
            usize::from(instruction.b),
            crate::value::Value::Object(std::rc::Rc::clone(&object)),
        );
        object_registers.write(
            usize::from(instruction.c),
            crate::value::Value::Object(object),
        );
        let object_result = crate::vm::execute_baseline_code_from(
            view,
            &plan,
            pc,
            &mut object_registers,
            &context,
            crate::environment::Environment::new(),
        );
        assert!(
            object_result.is_ok(),
            "object identity comparison should execute"
        );
        assert_eq!(
            object_registers.read(usize::from(instruction.a)),
            Some(crate::value::Value::Boolean(true))
        );

        let before = native.borrow().native_entry_count;
        let mut hostile = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        hostile.write(
            usize::from(instruction.b),
            crate::value::Value::Boolean(true),
        );
        hostile.write(usize::from(instruction.c), crate::value::Value::Number(1.0));
        let fallback = crate::vm::execute_baseline_code_from(
            view,
            &plan,
            pc,
            &mut hostile,
            &context,
            environment,
        );
        assert!(fallback.is_ok(), "non-identity operand must use fallback");
        assert_eq!(
            hostile.read(usize::from(instruction.a)),
            Some(crate::value::Value::Boolean(false))
        );
        assert_eq!(native.borrow().native_entry_count, before);
        executed = true;
    };
    inspect(program.code());
    program.code().cold_ops().for_each(|(_, op)| {
        op.visit_bodies(&mut |body| {
            if let Some(view) = body.code() {
                inspect(view);
            }
        });
    });
    assert!(
        executed,
        "ordinary source must reach and execute identity bytes"
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn ordinary_source_lowering_executes_tagged_identity_inequality() {
    let program = crate::reduce::reduce_source("var x = true; x !== false;")
        .expect("identity inequality source lowers");
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut executed = false;
    let mut inspect = |view: crate::machine::CodeView<'_>| {
        if executed {
            return;
        }
        let Some(pc) = (0..view.len()).find(|pc| {
            view.instruction(*pc).is_some_and(|instruction| {
                instruction.opcode.binary_operator(instruction.flags)
                    == Some(crate::ops::BinaryOp::StrictNotEqual)
            })
        }) else {
            return;
        };
        let plan = super::BaselinePlan::compile_for_test(view, policy);
        let Some(native) = plan.native_binary_at(pc) else {
            return;
        };
        let instruction = view
            .instruction(pc)
            .expect("identity inequality instruction");
        let mut registers = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        registers.write(
            usize::from(instruction.b),
            crate::value::Value::Boolean(true),
        );
        registers.write(
            usize::from(instruction.c),
            crate::value::Value::Boolean(false),
        );
        let environment = crate::environment::Environment::new();
        let context = crate::vm::current_context_or_default();
        let result = crate::vm::execute_baseline_code_from(
            view,
            &plan,
            pc,
            &mut registers,
            &context,
            std::rc::Rc::clone(&environment),
        );
        assert!(result.is_ok(), "tagged identity inequality should execute");
        assert_eq!(
            registers.read(usize::from(instruction.a)),
            Some(crate::value::Value::Boolean(true))
        );
        assert!(
            native.borrow().native_entry_count > 0,
            "inequality bytes must execute"
        );
        executed = true;
    };
    inspect(program.code());
    program.code().cold_ops().for_each(|(_, op)| {
        op.visit_bodies(&mut |body| {
            if let Some(view) = body.code() {
                inspect(view);
            }
        });
    });
    assert!(
        executed,
        "ordinary source must reach and execute inequality bytes"
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_add_chain_executes_two_ops_with_one_entry() {
    let mut plan = super::NativeAddChainPlan {
        physical: super::PhysicalInstallation::local(super::InstalledF64x3Entry::Unpublished),
        bindings: crate::stencil_plan::F64x3Bindings {
            inputs: [0, 1, 2],
            output: 3,
        },
        control: crate::stencil_cfg::RegionControlPlan::linear(0, 2).expect("linear control"),
        site: crate::quickening::QuickeningSite::new(crate::ir::Opcode::Add),
        last_native_view: None,
        native_entry_count: 0,
    };
    assert_eq!(plan.execute(1.5, 2.25, 4.0), Ok(7.75));
    assert!(matches!(
        plan.physical.installed(),
        super::InstalledF64x3Entry::Local(_)
    ));
    assert_eq!(plan.native_entry_count(), 1);
    let used = plan.physical.storage.used();
    let view =
        crate::stencil_select::select_physical(crate::stencil_select::add_chain_region_key())
            .expect("selected chain");
    let tail = view.fallthrough.expect("declared native successor");
    assert_eq!(used, view.stencil.bytes.len() + tail.stencil.bytes.len());
    assert_eq!(plan.execute(-2.0, 3.0, 5.0), Ok(6.0));
    assert_eq!(plan.physical.storage.used(), used);
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_increment_uses_add_const_template_for_number_subset() {
    let instruction = crate::ir::Instruction::inc_i(0, 1, false);
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut plan =
        super::NativeBinaryPlan::new(instruction, policy).expect("declared increment body");
    assert_eq!(plan.execute(4.5, 1.0), Ok(5.5));
    assert_eq!(plan.execute(-0.0, 1.0), Ok(1.0));
    assert!(plan.execute(f64::NAN, 1.0).unwrap().is_nan());
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_nullish_word_matches_canonical_tagged_values() {
    let instruction = crate::ir::Instruction::unary_operator(0, crate::ops::UnaryOp::IsNullish, 1);
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut plan =
        super::NativeNullishPlan::new(instruction, policy).expect("declared nullish word body");
    assert_eq!(
        plan.execute(crate::native_core::value_word::TaggedValue::null().bits()),
        Ok(true)
    );
    assert_eq!(
        plan.execute(crate::native_core::value_word::TaggedValue::undefined().bits()),
        Ok(true)
    );
    assert_eq!(
        plan.execute(crate::native_core::value_word::TaggedValue::bool(true).bits()),
        Ok(false)
    );
    assert_eq!(
        plan.execute(crate::native_core::value_word::TaggedValue::number(0.0).bits()),
        Ok(false)
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_add_chain_shared_entry_reuses_owner_after_eviction() {
    let shared = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("slab"),
    ));
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let bindings = crate::stencil_plan::F64x3Bindings {
        inputs: [0, 1, 2],
        output: 3,
    };
    let control = crate::stencil_cfg::RegionControlPlan::linear(0, 2).expect("linear control");
    let mut plan =
        super::NativeAddChainPlan::new_with_arena(policy, shared.clone(), bindings, control)
            .expect("shared add chain");
    assert_eq!(plan.execute(1.0, 2.0, 3.0), Ok(6.0));
    let used = shared.borrow().used();
    assert!(matches!(
        plan.physical.installed(),
        super::InstalledF64x3Entry::Shared(_)
    ));
    assert_eq!(plan.execute(2.0, 4.0, 8.0), Ok(14.0));
    assert_eq!(shared.borrow().used(), used);
    assert_eq!(shared.borrow_mut().evict_idle(0), 1);
    assert_eq!(plan.execute(2.0, 4.0, 8.0), Ok(14.0));
    assert!(matches!(
        plan.physical.installed(),
        super::InstalledF64x3Entry::Shared(_)
    ));
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_add_chain_rejects_mismatched_control_before_render() {
    let shared = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("slab"),
    ));
    let bindings = crate::stencil_plan::F64x3Bindings {
        inputs: [0, 1, 2],
        output: 3,
    };
    let control = crate::stencil_cfg::RegionControlPlan::linear(0, 1).expect("short control");
    let plan = super::NativeAddChainPlan::new_with_arena(
        crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
        shared.clone(),
        bindings,
        control,
    );
    assert!(plan.is_none());
    assert_eq!(shared.borrow().used(), 0);
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_move_uses_rendered_address_without_remapping() {
    let mut plan = super::NativeMovePlan {
        physical: super::PhysicalInstallation::local(super::InstalledWordEntry::Unpublished),
        site: crate::quickening::QuickeningSite::new(crate::ir::Opcode::Move),
        opcode: crate::ir::Opcode::Move,
        native_entry_count: 0,
        last_native_view: None,
    };
    let source = crate::native_core::value_word::TaggedValue::from_bits(0x1234_5678_9ABC_DEF0);
    assert_eq!(plan.execute(&source), Ok(source.bits()));
    #[cfg(quench_generated_stencil_artifacts)]
    assert!(plan.last_native_view.expect("invoked move view").generated);
    let used = plan.physical.storage.used();
    assert!(used > 0);
    assert_eq!(plan.execute(&source), Ok(source.bits()));
    assert_eq!(plan.physical.storage.used(), used);
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_move_shared_entry_reuses_owner_and_recovers_after_eviction() {
    let shared = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("slab"),
    ));
    let instruction = crate::ir::Instruction {
        opcode: crate::ir::Opcode::Move,
        flags: 0,
        a: 0,
        b: 1,
        c: 0,
    };
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut plan = super::NativeMovePlan::new_with_arena(instruction, policy, shared.clone())
        .expect("shared move plan");
    let source = crate::native_core::value_word::TaggedValue::from_bits(0xCAFE_BABE);
    assert_eq!(plan.execute(&source), Ok(source.bits()));
    let used = shared.borrow().used();
    assert!(matches!(
        plan.physical.installed(),
        super::InstalledWordEntry::Shared(_)
    ));
    assert_eq!(plan.execute(&source), Ok(source.bits()));
    assert_eq!(shared.borrow().used(), used);
    assert_eq!(shared.borrow_mut().evict_idle(0), 1);
    assert_eq!(plan.execute(&source), Ok(source.bits()));
    assert!(matches!(
        plan.physical.installed(),
        super::InstalledWordEntry::Shared(_)
    ));
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_load_local_uses_declared_tagged_word_entry() {
    let instruction = crate::ir::Instruction {
        opcode: crate::ir::Opcode::LoadLocal,
        flags: 0,
        a: 0,
        b: 1,
        c: 0,
    };
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let shared = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("slab"),
    ));
    let mut plan = super::NativeMovePlan::new_with_arena(instruction, policy, shared)
        .expect("declared LoadLocal body");
    let source = crate::native_core::value_word::TaggedValue::from_bits(0x1357_9BDF);
    assert_eq!(plan.execute(&source), Ok(source.bits()));
    #[cfg(quench_generated_stencil_artifacts)]
    assert!(
        plan.last_native_view
            .expect("invoked load-local view")
            .generated
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_store_local_uses_declared_tagged_word_entry() {
    let instruction = crate::ir::Instruction {
        opcode: crate::ir::Opcode::StoreLocal,
        flags: 0,
        a: 1,
        b: 0,
        c: 0,
    };
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let shared = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("slab"),
    ));
    let mut plan = super::NativeMovePlan::new_with_arena(instruction, policy, shared)
        .expect("declared StoreLocal body");
    let source = crate::native_core::value_word::TaggedValue::from_bits(0x2468_ACED);
    assert_eq!(plan.execute(&source), Ok(source.bits()));
    #[cfg(quench_generated_stencil_artifacts)]
    assert!(
        plan.last_native_view
            .expect("invoked store-local view")
            .generated
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_property_uses_rendered_address_without_remapping() {
    let mut plan = super::NativePropertyPlan {
        physical: super::PhysicalInstallation::local(super::InstalledPropertyEntry::Unpublished),
        opcode: crate::ir::Opcode::GetN,
        returns: false,
        native_entry_count: 0,
        last_native_view: None,
    };
    let site = crate::quickening::QuickeningSite::<4>::new(crate::ir::Opcode::GetN);
    let object = crate::value::ObjectData::new(vec![("value".into(), super::Value::Number(42.5))]);
    let access = object
        .guarded_plain_slot(object.semantic_layout_id(), 0, "value")
        .expect("plain guarded slot");
    assert_eq!(
        plan.execute(access, &site),
        Ok(crate::native_core::value_word::TaggedValue::number(42.5).bits())
    );
    assert_eq!(plan.native_entry_count, 1);
    let used = plan.physical.storage.used();
    assert!(used > 0);
    assert_eq!(
        plan.execute(access, &site),
        Ok(crate::native_core::value_word::TaggedValue::number(42.5).bits())
    );
    assert_eq!(plan.native_entry_count, 2);
    assert_eq!(plan.physical.storage.used(), used);
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_property_shared_entry_reuses_live_owner_and_recovers_after_eviction() {
    let shared = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("slab"),
    ));
    let instruction = crate::ir::Instruction {
        opcode: crate::ir::Opcode::GetN,
        flags: 0,
        a: 0,
        b: 1,
        c: 0,
    };
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut plan = super::NativePropertyPlan::new_with_arena(instruction, policy, shared.clone())
        .expect("shared property plan");
    let site = crate::quickening::QuickeningSite::<4>::new(crate::ir::Opcode::GetN);
    let object = crate::value::ObjectData::new(vec![("value".into(), super::Value::Number(42.5))]);
    let access = object
        .guarded_plain_slot(object.semantic_layout_id(), 0, "value")
        .expect("plain guarded slot");
    assert!(plan.execute(access, &site).is_ok());
    let used = shared.borrow().used();
    assert!(matches!(
        plan.physical.installed(),
        super::InstalledPropertyEntry::ReadShared { .. }
    ));
    assert!(plan.execute(access, &site).is_ok());
    assert_eq!(shared.borrow().used(), used);
    assert_eq!(shared.borrow_mut().evict_idle(0), 1);
    assert!(plan.execute(access, &site).is_ok());
    assert!(
        matches!(
            plan.physical.installed(),
            super::InstalledPropertyEntry::ReadShared { .. }
        ),
        "eviction must rebuild the entry"
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_property_rejects_stale_layout_before_loading_slot() {
    let mut plan = super::NativePropertyPlan {
        physical: super::PhysicalInstallation::local(super::InstalledPropertyEntry::Unpublished),
        opcode: crate::ir::Opcode::GetN,
        returns: false,
        native_entry_count: 0,
        last_native_view: None,
    };
    let site = crate::quickening::QuickeningSite::<4>::new(crate::ir::Opcode::GetN);
    let mut object =
        crate::value::ObjectData::new(vec![("value".into(), crate::value::Value::Number(42.5))]);
    let stale = object
        .guarded_plain_slot(object.semantic_layout_id(), 0, "value")
        .expect("plain guarded slot");
    object.set_property_in_place("other", crate::value::Value::Number(1.0));
    assert!(plan.execute(stale, &site).is_err());
    assert_eq!(object.hot_properties().position_rev("value"), Some(0));
}

#[test]
fn guarded_property_slot_rejects_descriptor_metadata() {
    let object = crate::value::ObjectData::new(vec![
        ("value".into(), crate::value::Value::Number(42.5)),
        (
            crate::builtins::descriptor_key("value"),
            crate::value::Value::Undefined,
        ),
    ]);
    assert!(object
        .guarded_plain_slot(object.semantic_layout_id(), 0, "value")
        .is_none());
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_property_write_commits_only_after_live_guards() {
    let instruction = crate::ir::Instruction {
        opcode: crate::ir::Opcode::SetN,
        flags: 0,
        a: 0,
        b: 1,
        c: 0,
    };
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut plan = super::NativePropertyPlan::new(instruction, policy).expect("write plan");
    let site = crate::quickening::QuickeningSite::<4>::new(crate::ir::Opcode::SetN);
    let mut object =
        crate::value::ObjectData::new(vec![("value".into(), crate::value::Value::Number(1.0))]);
    let access = object
        .guarded_plain_slot(object.semantic_layout_id(), 0, "value")
        .expect("guarded write slot");
    assert!(access.accepts_non_owning_store());
    plan.execute_write(
        access,
        crate::native_core::value_word::TaggedValue::number(5.0).bits(),
        &site,
    )
    .expect("native write");
    assert_eq!(
        object.hot_properties().slot_word(0).unwrap().load(),
        crate::value::Value::Number(5.0)
    );

    let stale = object
        .guarded_plain_slot(object.semantic_layout_id(), 0, "value")
        .expect("stale candidate");
    object.set_property_in_place("other", crate::value::Value::Number(2.0));
    assert!(plan
        .execute_write(
            stale,
            crate::native_core::value_word::TaggedValue::number(9.0).bits(),
            &site,
        )
        .is_err());
    assert_eq!(
        object.hot_properties().slot_word(0).unwrap().load(),
        crate::value::Value::Number(5.0)
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_property_entries_reject_crossed_read_write_abis() {
    let read_instruction = crate::ir::Instruction {
        opcode: crate::ir::Opcode::GetN,
        flags: 0,
        a: 0,
        b: 1,
        c: 0,
    };
    let write_instruction = crate::ir::Instruction {
        opcode: crate::ir::Opcode::SetN,
        ..read_instruction
    };
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut read = super::NativePropertyPlan::new(read_instruction, policy).unwrap();
    let mut write = super::NativePropertyPlan::new(write_instruction, policy).unwrap();
    let read_site = crate::quickening::QuickeningSite::<4>::new(crate::ir::Opcode::GetN);
    let write_site = crate::quickening::QuickeningSite::<4>::new(crate::ir::Opcode::SetN);
    let object =
        crate::value::ObjectData::new(vec![("value".into(), crate::value::Value::Number(1.0))]);
    let access = object
        .guarded_plain_slot(object.semantic_layout_id(), 0, "value")
        .unwrap();
    let bits = crate::native_core::value_word::TaggedValue::number(2.0).bits();

    assert!(read.execute_write(access, bits, &write_site).is_err());
    assert!(write.execute(access, &read_site).is_err());
    assert_eq!(
        object.hot_properties().slot_word(0).unwrap().load(),
        crate::value::Value::Number(1.0)
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn ordinary_residual_named_get_executes_guarded_property_stencil() {
    let function = crate::machine::FunctionCode::from_ops(vec![
        crate::ops::Op::GetProperty {
            dst: 0,
            object: 1,
            key: "value".into(),
        },
        crate::ops::Op::Return { src: 0 },
    ]);
    let code = function.code().expect("lowered named get");
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let plan = super::BaselinePlan::compile_for_test(code, policy);
    let object_data = std::rc::Rc::new(crate::value::ObjectData::new(vec![(
        "value".into(),
        crate::value::Value::Number(7.0),
    )]));
    let object = crate::value::Value::Object(std::rc::Rc::clone(&object_data));
    let mut registers = crate::register_file::RegisterFile::from_values(vec![
        crate::value::Value::Undefined,
        object,
    ]);
    let context = crate::vm::current_context_or_default();
    let (completion, _) = crate::vm::execute_baseline_code_from(
        code,
        &plan,
        0,
        &mut registers,
        &context,
        crate::environment::Environment::new(),
    )
    .expect("ordinary named get execution");
    assert_eq!(
        completion,
        crate::completion::Completion::Return(crate::value::Value::Number(7.0))
    );
    registers.write(0, crate::value::Value::Undefined);
    let (completion, _) = crate::vm::execute_baseline_code_from(
        code,
        &plan,
        0,
        &mut registers,
        &context,
        crate::environment::Environment::new(),
    )
    .expect("guarded named get execution");
    assert_eq!(
        completion,
        crate::completion::Completion::Return(crate::value::Value::Number(7.0))
    );
    let native_count = plan
        .native_property_at(0)
        .map(|native| native.borrow().native_entry_count)
        .unwrap_or(0);
    assert!(native_count > 0);
    #[cfg(quench_generated_stencil_artifacts)]
    {
        let expected =
            crate::stencil_select::select_physical(crate::stencil_select::property_region_key())
                .expect("generated own-property view");
        let witness = plan
            .native_property_at(0)
            .and_then(|native| native.borrow().last_native_view())
            .expect("invoked own-property view");
        assert!(expected.generated && witness.generated);
        assert!(witness.matches(&expected));
    }
    assert!(crate::execute::set_property_in_place(
        &crate::value::Value::Object(object_data),
        "other",
        crate::value::Value::Number(1.0),
    ));
    registers.write(0, crate::value::Value::Undefined);
    let (completion, _) = crate::vm::execute_baseline_code_from(
        code,
        &plan,
        0,
        &mut registers,
        &context,
        crate::environment::Environment::new(),
    )
    .expect("shape-change fallback");
    assert_eq!(
        completion,
        crate::completion::Completion::Return(crate::value::Value::Number(7.0))
    );
    assert_eq!(
        plan.native_property_at(0)
            .map(|native| native.borrow().native_entry_count),
        Some(native_count),
        "shape mutation must not re-enter the stale native slot"
    );
}

#[cfg(target_arch = "aarch64")]
#[test]
fn ordinary_source_named_store_executes_guarded_property_stencil() {
    let program = crate::reduce::reduce_source("var o = { value: 1 }; o.value = 5;")
        .expect("ordinary named store lowers");
    let mut executed = false;
    let mut inspect = |code: crate::machine::CodeView<'_>| {
        let Some(pc) = (0..code.len()).find(|pc| {
            code.instruction(*pc).is_some_and(|instruction| {
                instruction.opcode == crate::ir::Opcode::SetN && instruction.flags == 0
            })
        }) else {
            return;
        };
        execute_generated_property_store(code, pc);
        executed = true;
    };
    inspect(program.code());
    program.code().cold_ops().for_each(|(_, op)| {
        op.visit_bodies(&mut |body| {
            if let Some(code) = body.code() {
                inspect(code);
            }
        });
    });
    assert!(
        executed,
        "ordinary source must lower a non-strict named store"
    );
}

#[cfg(target_arch = "aarch64")]
fn execute_generated_property_store(code: crate::machine::CodeView<'_>, pc: usize) {
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let plan = super::BaselinePlan::compile_for_test(code, policy);
    let instruction = code.instruction(pc).expect("SetN instruction");
    let object = std::rc::Rc::new(crate::value::ObjectData::new(vec![(
        "value".into(),
        crate::value::Value::Number(1.0),
    )]));
    let mut registers = property_store_registers(code, instruction, &object);
    let context = crate::vm::current_context_or_default();
    run_property_store_and_assert(code, &plan, pc, &mut registers, &context, 5.0);
    run_property_store_and_assert(code, &plan, pc, &mut registers, &context, 7.0);
    let native = plan
        .native_store_property_at(pc)
        .expect("native store plan");
    assert!(native.borrow().native_entry_count() > 0);
    #[cfg(quench_generated_stencil_artifacts)]
    assert_generated_property_store(&native.borrow());
    assert_readonly_property_store_falls_back(
        code,
        &plan,
        pc,
        &mut registers,
        &context,
        &object,
        &native,
    );
}

#[cfg(target_arch = "aarch64")]
fn property_store_registers(
    code: crate::machine::CodeView<'_>,
    instruction: crate::ir::Instruction,
    object: &std::rc::Rc<crate::value::ObjectData>,
) -> crate::register_file::RegisterFile {
    let mut registers = crate::register_file::RegisterFile::with_undefined(
        usize::from(code.register_count()).max(8),
    );
    registers.write(
        usize::from(instruction.a),
        crate::value::Value::Object(std::rc::Rc::clone(object)),
    );
    registers
}

#[cfg(target_arch = "aarch64")]
fn run_property_store_and_assert(
    code: crate::machine::CodeView<'_>,
    plan: &super::BaselinePlan,
    pc: usize,
    registers: &mut crate::register_file::RegisterFile,
    context: &crate::vm::VmContext,
    number: f64,
) {
    let instruction = code.instruction(pc).expect("SetN instruction");
    run_property_store(code, plan, pc, registers, context, instruction.b, number);
    assert_eq!(
        named_value(&registers, instruction.a),
        crate::value::Value::Number(number)
    );
}

#[cfg(target_arch = "aarch64")]
fn assert_readonly_property_store_falls_back(
    code: crate::machine::CodeView<'_>,
    plan: &super::BaselinePlan,
    pc: usize,
    registers: &mut crate::register_file::RegisterFile,
    context: &crate::vm::VmContext,
    object: &std::rc::Rc<crate::value::ObjectData>,
    native: &std::cell::RefCell<super::NativePropertyPlan>,
) {
    let entries = native.borrow().native_entry_count();
    let descriptor = crate::value::Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(
        vec![("writable".into(), crate::value::Value::Boolean(false))],
    )));
    assert!(crate::execute::set_property_in_place(
        &crate::value::Value::Object(std::rc::Rc::clone(&object)),
        &crate::builtins::descriptor_key("value"),
        descriptor,
    ));
    let source = code.instruction(pc).expect("SetN instruction").b;
    run_property_store(code, plan, pc, registers, context, source, 9.0);
    let object_register = code.instruction(pc).expect("SetN instruction").a;
    assert_eq!(
        named_value(registers, object_register),
        crate::value::Value::Number(7.0)
    );
    assert_eq!(native.borrow().native_entry_count(), entries);
}

#[cfg(target_arch = "aarch64")]
fn run_property_store(
    code: crate::machine::CodeView<'_>,
    plan: &super::BaselinePlan,
    pc: usize,
    registers: &mut crate::register_file::RegisterFile,
    context: &crate::vm::VmContext,
    source: u16,
    number: f64,
) {
    registers.write(usize::from(source), crate::value::Value::Number(number));
    crate::vm::execute_baseline_code_from(
        code,
        plan,
        pc,
        registers,
        context,
        crate::environment::Environment::new(),
    )
    .expect("named store execution");
}

#[cfg(target_arch = "aarch64")]
fn named_value(registers: &crate::register_file::RegisterFile, object: u16) -> crate::value::Value {
    let receiver = registers
        .read(usize::from(object))
        .expect("stored receiver");
    crate::vm::get_named_property_result(&receiver, "value", &std::cell::Cell::new(0))
        .expect("read stored value")
}

#[cfg(all(target_arch = "aarch64", quench_generated_stencil_artifacts))]
fn assert_generated_property_store(plan: &super::NativePropertyPlan) {
    let expected =
        crate::stencil_select::select_physical(crate::stencil_select::store_property_region_key())
            .expect("generated store-property view");
    let witness = plan
        .last_native_view()
        .expect("invoked store-property view");
    assert!(expected.generated && witness.generated);
    assert!(witness.matches(&expected));
}

#[cfg(target_arch = "aarch64")]
#[test]
fn ordinary_residual_prototype_get_executes_guarded_property_stencil() {
    let function = crate::machine::FunctionCode::from_ops(vec![
        crate::ops::Op::GetProperty {
            dst: 0,
            object: 1,
            key: "value".into(),
        },
        crate::ops::Op::Return { src: 0 },
    ]);
    let code = function.code().expect("lowered prototype get");
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let plan = super::BaselinePlan::compile_for_test(code, policy);
    let owner = std::rc::Rc::new(crate::value::ObjectData::new(vec![(
        "value".into(),
        crate::value::Value::Number(11.0),
    )]));
    let prototype = std::rc::Rc::new(crate::value::ObjectData::new(vec![(
        "\0prototype".into(),
        crate::value::Value::Object(std::rc::Rc::clone(&owner)),
    )]));
    let receiver = std::rc::Rc::new(crate::value::ObjectData::new(vec![(
        "\0prototype".into(),
        crate::value::Value::Object(std::rc::Rc::clone(&prototype)),
    )]));
    let mut registers = crate::register_file::RegisterFile::from_values(vec![
        crate::value::Value::Undefined,
        crate::value::Value::Object(std::rc::Rc::clone(&receiver)),
    ]);
    let context = crate::vm::current_context_or_default();
    let (completion, _) = crate::vm::execute_baseline_code_from(
        code,
        &plan,
        0,
        &mut registers,
        &context,
        crate::environment::Environment::new(),
    )
    .expect("prototype get execution");
    assert_eq!(
        completion,
        crate::completion::Completion::Return(crate::value::Value::Number(11.0))
    );
    assert_eq!(
        plan.native_property_at(0)
            .map(|native| native.borrow().native_entry_count),
        Some(0),
        "cold prototype lookup installs the canonical IC before native entry"
    );
    registers.write(0, crate::value::Value::Undefined);
    let (completion, _) = crate::vm::execute_baseline_code_from(
        code,
        &plan,
        0,
        &mut registers,
        &context,
        crate::environment::Environment::new(),
    )
    .expect("warm prototype get execution");
    assert_eq!(
        completion,
        crate::completion::Completion::Return(crate::value::Value::Number(11.0))
    );
    assert!(plan
        .native_property_at(0)
        .is_some_and(|native| native.borrow().native_entry_count > 0));
    assert!(plan.native_property_at(0).is_some_and(|native| matches!(
        native.borrow().physical.installed(),
        super::InstalledPropertyEntry::ReadShared { key, .. }
            if key == crate::stencil_select::prototype_property_region_key()
    )));
    #[cfg(quench_generated_stencil_artifacts)]
    {
        let expected = crate::stencil_select::select_physical(
            crate::stencil_select::prototype_property_region_key(),
        )
        .expect("generated prototype property view");
        let witness = plan
            .native_property_at(0)
            .and_then(|native| native.borrow().last_native_view())
            .expect("invoked prototype property view");
        assert!(expected.generated && witness.generated);
        assert!(witness.matches(&expected));
    }
    let mut native_count = plan
        .native_property_at(0)
        .map(|native| native.borrow().native_entry_count)
        .unwrap_or(0);
    let replacement_owner = std::rc::Rc::new(crate::value::ObjectData::new(vec![(
        "value".into(),
        crate::value::Value::Number(13.0),
    )]));
    let prototype_slot = prototype
        .hot_properties()
        .position_rev("\0prototype")
        .and_then(|slot| prototype.hot_properties().slot_word(slot))
        .expect("intermediate prototype slot");
    prototype_slot.store_object_or_null(Some(&replacement_owner));
    registers.write(0, crate::value::Value::Undefined);
    let (completion, _) = crate::vm::execute_baseline_code_from(
        code,
        &plan,
        0,
        &mut registers,
        &context,
        crate::environment::Environment::new(),
    )
    .expect("intermediate prototype mutation fallback");
    assert_eq!(
        completion,
        crate::completion::Completion::Return(crate::value::Value::Number(13.0))
    );
    assert_eq!(
        plan.native_property_at(0)
            .map(|native| native.borrow().native_entry_count),
        Some(native_count + 1),
        "native chain guard must observe and reject the changed identity"
    );
    native_count += 1;
    let replacement = std::rc::Rc::new(crate::value::ObjectData::new(Vec::new()));
    let receiver_slot = receiver
        .hot_properties()
        .position_rev("\0prototype")
        .and_then(|slot| receiver.hot_properties().slot_word(slot))
        .expect("receiver prototype slot");
    receiver_slot.store_object_or_null(Some(&replacement));
    registers.write(0, crate::value::Value::Undefined);
    let (completion, _) = crate::vm::execute_baseline_code_from(
        code,
        &plan,
        0,
        &mut registers,
        &context,
        crate::environment::Environment::new(),
    )
    .expect("prototype mutation fallback");
    assert_eq!(
        completion,
        crate::completion::Completion::Return(crate::value::Value::Undefined)
    );
    assert_eq!(
        plan.native_property_at(0)
            .map(|native| native.borrow().native_entry_count),
        Some(native_count + 1),
        "receiver-chain guard must reject the stale cached identity"
    );
}

#[test]
fn osr_admission_only_accepts_back_edges() {
    assert!(!super::is_osr_candidate(3, crate::ir::Instruction::ret(0)));
    assert!(super::is_osr_candidate(3, crate::ir::Instruction::jump(2)));
    assert!(!super::is_osr_candidate(3, crate::ir::Instruction::jump(4)));
    assert!(super::is_osr_candidate(
        3,
        crate::ir::Instruction::jump_if_false(1, 2)
    ));
    assert!(!super::is_osr_candidate(
        3,
        crate::ir::Instruction {
            opcode: crate::ir::Opcode::ForI,
            flags: 0,
            a: 0,
            b: 0,
            c: 0,
        }
    ));
}

#[test]
fn machine_rejects_frame_ranges_not_owned_by_its_code_store() {
    let function = super::FunctionCode::pending(vec![super::Op::ParameterEnd]);
    let mut machine = Machine::with_function(&function, EnvironmentRef(0), 1);
    let invalid = super::CodeRange::new(super::CodeId(99), 0, 1).unwrap();
    let frame = super::Frame::Await {
        phase: 0,
        resume: invalid,
        destination: 0,
    };
    assert!(machine.try_push_frame(frame).is_err());
    assert_eq!(machine.frame_count(), 0);
}

#[test]
fn machine_accepts_frame_ranges_from_its_immutable_store() {
    let function = super::FunctionCode::from_ops(vec![super::Op::ParameterEnd]);
    let mut machine = Machine::with_function(&function, EnvironmentRef(0), 1);
    let frame = super::Frame::Await {
        phase: 0,
        resume: function.range,
        destination: 0,
    };
    machine.try_push_frame(frame).unwrap();
    assert_eq!(machine.frame_count(), 1);
}

#[test]
fn code_arena_can_import_existing_function_ranges() {
    let function = super::FunctionCode::pending(vec![super::Op::ParameterEnd]);
    let mut arena = super::CodeArena::new();
    let range = arena.append_function(&function).expect("function range");
    assert_eq!(arena.freeze().code(range).map(|code| code.len()), Some(0));
}

#[test]
fn linked_nested_bodies_share_one_immutable_code_store() {
    let child = super::FunctionCode::pending(vec![super::Op::ParameterEnd]);
    let root = super::FunctionCode::from_ops(vec![super::Op::IteratorBinding {
        iterator: 0,
        body: child,
        close_normal: false,
    }]);
    let Some(super::Op::IteratorBinding { body, .. }) =
        root.code().and_then(|code| code.cold_at(0))
    else {
        panic!("iterator binding");
    };
    assert!(root.store.same_store(&body.store));
}

#[test]
fn register_window_is_pre_sized_from_code_metadata() {
    let window = RegisterWindow::with_count(3);
    assert_eq!(window.values, vec![Value::Undefined; 3]);
}

#[test]
fn frame_stack_grows_geometrically_before_hitting_hard_limit() {
    let range = super::CodeRange::new(super::CodeId(0), 0, 1).unwrap();
    let frame = || super::Frame::Await {
        phase: 0,
        resume: range,
        destination: 0,
    };
    let mut stack = super::FrameStack::with_capacity_and_limit(1, 3);
    assert_eq!(stack.capacity(), 1);
    stack.try_push(frame()).unwrap();
    stack.try_push(frame()).unwrap();
    assert!(stack.capacity() >= 2);
    stack.try_push(frame()).unwrap();
    assert!(stack.try_push(frame()).is_err());
    assert_eq!(stack.as_slice().len(), 3);
}

#[test]
fn frame_stack_reserves_without_crossing_limit() {
    let mut stack = super::FrameStack::with_capacity_and_limit(1, 4);
    assert!(stack.try_reserve_for(3));
    assert!(stack.capacity() >= 3);
    assert!(!stack.try_reserve_for(10));
    assert!(stack.capacity() <= 4);
}

#[test]
fn frame_stack_depth_is_the_single_source_of_active_frames() {
    let range = super::CodeRange::new(super::CodeId(0), 0, 1).unwrap();
    let frame = || super::Frame::Await {
        phase: 0,
        resume: range,
        destination: 0,
    };
    let mut stack = super::FrameStack::with_capacity_and_limit(1, 2);
    assert!(stack.is_empty());
    assert_eq!(stack.depth(), 0);
    stack.try_push(frame()).unwrap();
    assert!(!stack.is_empty());
    assert_eq!(stack.depth(), 1);
    stack.try_push(frame()).unwrap();
    assert_eq!(stack.depth(), 2);
    stack.pop();
    assert_eq!(stack.depth(), 1);
    stack.pop();
    assert!(stack.is_empty());
    assert_eq!(stack.depth(), 0);
}

#[test]
fn frame_stack_rejects_at_limit_without_growing() {
    let range = super::CodeRange::new(super::CodeId(0), 0, 1).unwrap();
    let mut stack = super::FrameStack::with_capacity_and_limit(2, 2);
    stack
        .try_push(super::Frame::Await {
            phase: 0,
            resume: range,
            destination: 0,
        })
        .unwrap();
    stack
        .try_push(super::Frame::Await {
            phase: 0,
            resume: range,
            destination: 0,
        })
        .unwrap();
    let capacity = stack.capacity();
    assert!(stack
        .try_push(super::Frame::Await {
            phase: 0,
            resume: range,
            destination: 0,
        })
        .is_err());
    assert_eq!(stack.capacity(), capacity);
    assert_eq!(stack.depth(), 2);
}

#[test]
fn frame_stack_reports_remaining_hard_limit() {
    let mut stack = super::FrameStack::with_capacity_and_limit(1, 3);
    assert_eq!(stack.remaining(), 3);
    let range = super::CodeRange::new(super::CodeId(0), 0, 1).unwrap();
    stack
        .try_push(super::Frame::Await {
            phase: 0,
            resume: range,
            destination: 0,
        })
        .unwrap();
    assert_eq!(stack.remaining(), 2);
}

#[test]
fn frame_stack_handles_deep_js_continuations_without_native_recursion() {
    let range = super::CodeRange::new(super::CodeId(0), 0, 1).unwrap();
    let mut stack = super::FrameStack::with_capacity_and_limit(1, 10_000);
    for _ in 0..10_000 {
        stack
            .try_push(super::Frame::Await {
                phase: 0,
                resume: range,
                destination: 0,
            })
            .expect("explicit VM stack should accept configured depth");
    }
    assert_eq!(stack.depth(), 10_000);
    for _ in 0..10_000 {
        stack.pop().expect("every continuation must be recoverable");
    }
    assert!(stack.is_empty());
    assert!(stack.invariant_holds());
}

#[test]
fn frame_offsets_survive_contiguous_storage_growth() {
    let range = super::CodeRange::new(super::CodeId(0), 0, 1).unwrap();
    let mut stack = super::FrameStack::with_capacity_and_limit(1, 4);
    stack
        .try_push(super::Frame::Await {
            phase: 7,
            resume: range,
            destination: 0,
        })
        .unwrap();
    let first = stack.top_offset().expect("first frame offset");
    let first_ptr = stack.frame_at(first).expect("first frame");
    assert!(matches!(first_ptr, super::Frame::Await { phase: 7, .. }));

    // Force Vec growth/reallocation. The offset, rather than a borrowed
    // reference, is the continuation identity and must still resolve.
    stack
        .try_push(super::Frame::Await {
            phase: 9,
            resume: range,
            destination: 0,
        })
        .unwrap();
    assert!(matches!(
        stack.frame_at(first),
        Some(super::Frame::Await { phase: 7, .. })
    ));
    assert_eq!(stack.top_offset(), Some(1));
}
#[test]
fn constant_pool_deduplicates_instruction_constants() {
    let code = super::ExecutableCode::from_ops(vec![
        super::Op::Const {
            dst: 0,
            value: super::Constant::Number(2.0),
        },
        super::Op::Const {
            dst: 1,
            value: super::Constant::Number(2.0),
        },
    ]);
    let pool = code.store().constant_pool(code.entry());
    assert_eq!(pool.len(), 1);
    assert_eq!(pool.get(0), Some(&super::Constant::Number(2.0)));
}

#[test]
fn constant_pool_assigns_canonical_first_use_ids() {
    let pool = super::ConstantPool::new(vec![
        super::Constant::String("a".into()),
        super::Constant::Number(1.0),
        super::Constant::String("a".into()),
        super::Constant::Boolean(true),
    ]);
    assert_eq!(pool.len(), 3);
    assert_eq!(pool.id(&super::Constant::String("a".into())), Some(0));
    assert_eq!(pool.id(&super::Constant::Number(1.0)), Some(1));
    assert_eq!(pool.id(&super::Constant::Boolean(true)), Some(2));
    assert_eq!(pool.get(2), Some(&super::Constant::Boolean(true)));
}

#[test]
fn frame_continuation_register_contract_uses_integer_ids() {
    let range = super::CodeRange::new(super::CodeId(0), 0, 1).unwrap();
    let frame = super::Frame::Branch {
        phase: super::BranchPhase::Body,
        branch_resume: range,
        resume: range,
        dst: Some(2),
        yield_dst: 4,
    };
    assert_eq!(frame.register_ids(), vec![2, 4]);
    assert!(frame.has_valid_register_ids(5));
    assert!(!frame.has_valid_register_ids(4));
}

#[test]
fn frames_without_register_destinations_have_empty_contract() {
    let range = super::CodeRange::new(super::CodeId(0), 0, 1).unwrap();
    let frame = super::Frame::Await {
        phase: 0,
        resume: range,
        destination: 0,
    };
    assert!(frame.register_ids().is_empty());
    assert!(frame.has_valid_register_ids(0));
}
