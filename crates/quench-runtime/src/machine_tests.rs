#[test]
fn call_frame_suspend_and_resume_restores_caller_state() {
    let function = super::FunctionCode::from_ops(vec![
        super::Op::Move { dst: 0, src: 0 },
        super::Op::Move { dst: 1, src: 1 },
    ]);
    let mut machine = super::Machine::with_function(&function, super::EnvironmentRef(7), 2);
    machine.set_program_counter(1);
    machine.registers_mut().write(0, super::Value::Number(11.0));
    machine.suspend_call(
        super::Value::Undefined,
        super::Value::Undefined,
        vec![super::Value::Number(3.0)],
        1,
        crate::completion::ContinuationGuards::new(9),
    );
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
fn machine_rejects_call_continuation_from_unknown_code_source() {
    let function = super::FunctionCode::from_ops(vec![super::Op::ParameterEnd]);
    let mut machine = Machine::with_function(&function, EnvironmentRef(0), 1);
    machine.push_call_frame(crate::completion::CallContinuation {
        callee: super::Value::Undefined,
        receiver: super::Value::Undefined,
        arguments: Vec::new().into(),
        caller_code: super::CodeId(99),
        caller_pc: 0,
        caller_registers: crate::register_file::RegisterFile::new(),
        caller_environment: EnvironmentRef(0),
        destination: 0,
        guards: crate::completion::ContinuationGuards::default(),
    });
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
    crate::vm::execute_baseline_code_from(
        code,
        &plan,
        0,
        &mut registers,
        &context,
        environment,
    )
    .expect("scalar execution after owner eviction");
    assert_eq!(native.borrow().native_entry_count, 2);
    assert_eq!(registers.read(0), Some(crate::value::Value::Number(5.0)));
}

#[test]
fn composed_plan_retires_cached_bytes_after_committed_failure() {
    let mut plan = super::NativeRegionPlan::new_for_test(
        crate::stencil_select::array_loop_body_region_key(),
    )
    .expect("array region plan");
    plan.cache.insert(
        crate::stencil_fact::RegionKey(900),
        0,
        0x1000,
    );
    let committed: Result<crate::vm::DispatchTransition, super::NativeDispatchError> =
        Err(super::NativeDispatchError::Committed("post-entry".into()));
    plan.retire_on_failure(&committed);
    assert_eq!(plan.cache.len(), 0, "committed bytes must not remain callable");

    plan.cache.insert(
        crate::stencil_fact::RegionKey(901),
        0,
        0x2000,
    );
    let semantic: Result<crate::vm::DispatchTransition, super::NativeDispatchError> =
        Err(super::NativeDispatchError::Semantic(crate::vm::VmError::EvalError(
            "ordinary throw".into(),
        )));
    plan.retire_on_failure(&semantic);
    assert_eq!(plan.cache.len(), 1, "semantic errors do not invalidate physical code");
}

#[test]
fn region_admission_rejects_noncanonical_operands_before_publication() {
    let record = crate::stencil_select::select_region(
        crate::stencil_select::loop_body_region_key(),
    )
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
                handler: opcode.handler(),
                control: opcode.control_operands(instruction),
            }
        })
        .collect::<Vec<_>>();
    entries[0].instruction.c = 1;
    assert!(!super::region_admission_matches(&entries, 0, record));
}

#[test]
fn generated_scalar_and_array_rows_route_through_declared_abis() {
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
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
    let array_record = crate::stencil_select::select_region(
        crate::stencil_select::array_loop_body_region_key(),
    )
    .expect("array row");
    assert_eq!(array_record.entry, 0);
    assert_eq!(array_record.external_entries, &[0]);
    assert!(matches!(
        array_record.abi,
        crate::stencil_select::RegionAbi::ArrayKernel
            | crate::stencil_select::RegionAbi::Bridge
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
        parameter_ends: vec![None].into(),
        constants: vec![super::ConstantPool::new(vec![
            super::Constant::Boolean(false),
        ])]
        .into(),
        metadata: vec![vec![super::InstructionMeta::empty(); 4]].into(),
        register_counts: vec![1].into(),
        quickening_sites: vec![Vec::<
            std::cell::RefCell<crate::quickening::QuickeningSite<4>>,
        >::new()
        .into_boxed_slice()]
        .into(),
        operand_windows: vec![Vec::<std::rc::Rc<[u16]>>::new()].into(),
        catch_ranges: vec![Vec::<super::CatchRange>::new()].into(),
    });
    let range = super::CodeRange::new(super::CodeId(0), 0, 4).expect("valid test range");
    let owner = super::FunctionCode::new(store, range);
    owner.set_tier_threshold_for_test(1);
    let code = owner.code().expect("compact test code");
    assert!(!owner.is_osr_entry(2), "no plan before the back-edge");

    let mut registers = crate::register_file::RegisterFile::from_values(vec![
        crate::value::Value::Boolean(true),
    ]);
    let context = crate::vm::current_context_or_default();
    let (completion, next) = crate::vm::execute_function_code_step_from(
        code,
        &owner,
        0,
        &mut registers,
        &context,
    )
    .expect("hot loop executes");

    assert_eq!(completion, crate::completion::Completion::Return(crate::value::Value::Boolean(false)));
    assert_eq!(next, 4);
    assert_eq!(owner.tier(), super::ExecutionTier::Baseline);
    let profile = owner.tier_profile();
    assert_eq!(profile.osr_entries, 1);
    assert_eq!(profile.osr_transfers, 1);
    assert!(owner.is_osr_entry(2));

    // The same canonical compact code must remain correct when admission is
    // disabled: this is the differential guard that proves OSR is an
    // execution shortcut, not an alternate semantic path.
    let cold_owner = super::FunctionCode::new(owner.store().expect("test code store"), range);
    cold_owner.set_tier_threshold_for_test(100);
    let mut cold_registers = crate::register_file::RegisterFile::from_values(vec![
        crate::value::Value::Boolean(true),
    ]);
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
        arena: None,
        shared_arena: None,
        cache: crate::stencil_select::RenderedRegionCache::new(),
        lifecycle: crate::stencil_lifecycle::StencilLifecycle::new(),
        site: crate::quickening::QuickeningSite::new(crate::ir::Opcode::Add),
        opcode: crate::ir::Opcode::Add,
        key: crate::stencil_select::numeric_region_key(crate::ir::Opcode::Add).unwrap(),
        tagged_key: None,
        returns_boolean: false,
        integer_op: None,
        integer_unsigned: false,
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        shared_entry_address: None,
        native_entry_count: 0,
    };
    assert!(plan.execute(1.0, 2.0).is_err());
    assert!(plan.arena.is_none());
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_numeric_entry_pointer_is_cached_after_first_render() {
    let mut plan = super::NativeBinaryPlan {
        arena: None,
        shared_arena: None,
        cache: crate::stencil_select::RenderedRegionCache::new(),
        lifecycle: crate::stencil_lifecycle::StencilLifecycle::new(),
        site: crate::quickening::QuickeningSite::new(crate::ir::Opcode::Add),
        opcode: crate::ir::Opcode::Add,
        key: crate::stencil_select::numeric_region_key(crate::ir::Opcode::Add).unwrap(),
        tagged_key: None,
        returns_boolean: false,
        integer_op: None,
        integer_unsigned: false,
        entry: None,
        int_entry: None,
        tagged_entry: None,
        uint_entry: None,
        shared_entry_address: None,
        tagged_shared_entry: None,
        native_entry_count: 0,
    };
    assert_eq!(plan.execute(1.5, 2.25), Ok(3.75));
    assert!(plan.entry.is_some());
    let used = plan.arena.as_ref().expect("rendered arena").used();
    assert_eq!(plan.execute(4.0, 5.0), Ok(9.0));
    assert_eq!(plan.arena.as_ref().expect("cached arena").used(), used);
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
    assert!(plan.entry.is_some());
    assert!(plan.shared_entry_address.is_some());
    assert_eq!(plan.execute(4.0, 5.0), Ok(9.0));
    assert_eq!(shared.borrow().used(), used);
    assert_eq!(shared.borrow_mut().evict_idle(0), 1);
    assert_eq!(plan.execute(4.0, 5.0), Ok(9.0));
    assert!(plan.shared_entry_address.is_some(), "eviction must rebuild the entry");
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
        assert_eq!(ordered.execute(f64::NAN, rhs), Ok(0.0), "operator {operator:?}");
    }
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
        (crate::ops::BinaryOp::BitwiseAnd, 0xF0F0_i32, 0x0FF0_i32, 0x00F0_i32),
        (crate::ops::BinaryOp::BitwiseOr, 0xF000_i32, 0x00F0_i32, 0xF0F0_i32),
        (crate::ops::BinaryOp::BitwiseXor, -1_i32, 0x0F0F_i32, !0x0F0F_i32),
        (crate::ops::BinaryOp::ShiftLeft, 1_i32, 32_i32, 1_i32),
        (crate::ops::BinaryOp::ShiftRight, -8_i32, 1_i32, -4_i32),
        (crate::ops::BinaryOp::ShiftRightZeroFill, -1_i32, 1_i32, 2_147_483_647_i32),
    ] {
        let mut plan = super::NativeBinaryPlan::new(
            crate::ir::Instruction {
                flags: crate::ir::compact_binary_id(operator),
                ..instruction
            },
            policy,
        )
        .expect("i32 bitwise region");
        assert_eq!(plan.execute(f64::from(lhs), f64::from(rhs)), Ok(f64::from(expected)));
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
        assert_eq!(plan.execute(1.5, f64::from(rhs)), Ok(f64::from(expected_fraction)));
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
            plan.int_entry.is_some() || plan.uint_entry.is_some(),
            "conversion cases must reach the rendered typed entry"
        );
    }
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
    for (input, expected) in [(0.0, -1.0), (1.5, -2.0), (f64::NAN, -1.0), (4_294_967_297.0, -2.0)] {
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
    assert!(plan.shared_entry.is_some());
    assert_eq!(plan.execute(1.5), Ok(-2.0));
    assert_eq!(shared.borrow().used(), used);
    assert_eq!(shared.borrow_mut().evict_idle(0), 1);
    assert_eq!(plan.execute(1.5), Ok(-2.0));
    assert!(plan.shared_entry.is_some(), "eviction must rebuild the entry");
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn ordinary_source_lowering_executes_bitwise_not_and_falls_back_for_string() {
    let program = crate::reduce::reduce_source("var x = 1; x = ~x; x;")
        .expect("bitwise-not source lowers");
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
        let Some(native) = plan.native_unary_at(pc) else { return };
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
        assert_eq!(registers.read(usize::from(instruction.a)), Some(crate::value::Value::Number(-2.0)));
        let before = native.borrow().native_entry_count;
        let mut hostile = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        hostile.write(usize::from(instruction.b), crate::value::Value::String("1".into()));
        assert!(crate::vm::execute_baseline_code_from(
            view,
            &plan,
            pc,
            &mut hostile,
            &context,
            environment,
        )
        .is_ok());
        assert_eq!(hostile.read(usize::from(instruction.a)), Some(crate::value::Value::Number(-2.0)));
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
        assert!(before > 0, "numeric branch must execute emitted truthiness bytes");
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
    assert!(checked, "ordinary source must execute the numeric truthiness leaf");
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
        registers.write(usize::from(instruction.a), crate::value::Value::Boolean(true));
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
    assert!(executed, "ordinary source must execute tagged truthiness bytes");
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
        let native = plan.native_truthiness_at(pc).expect("pointer truthiness leaf");
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
    assert!(executed, "ordinary source must execute pointer truthiness bytes");
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
    let native = plan.native_load_const_at(0).expect("primitive constant leaf");
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

#[test]
fn region_verifier_rejects_physical_call_for_raw_abi() {
    #[cfg(target_arch = "aarch64")]
    assert!(super::stencil_contains_call(&[0x00, 0x00, 0x00, 0x94]));
    #[cfg(target_arch = "aarch64")]
    assert!(super::stencil_contains_call(&[0x00, 0x00, 0x3F, 0xD6]));
    #[cfg(target_arch = "x86_64")]
    assert!(super::stencil_contains_call(&[0xE8, 0, 0, 0, 0]));
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    assert!(!super::stencil_contains_call(&[0xE8]));
    for key in [
        crate::stencil_select::array_loop_body_region_key(),
        crate::stencil_select::array_numeric_loop_region_key(),
    ] {
        let record = crate::stencil_select::select_region(key).expect("raw declaration");
        assert!(!super::stencil_contains_call(record.stencil.bytes));
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
                instruction.opcode == crate::ir::Opcode::Binary
                    && matches!(
                        crate::ir::compact_binary_operator(instruction.flags),
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
    assert!(admitted, "ordinary source must reach the guarded bitwise plan");
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
                instruction.opcode == crate::ir::Opcode::Binary
                    && crate::ir::compact_binary_operator(instruction.flags)
                        == Some(crate::ops::BinaryOp::BitwiseAnd)
            })
        }) else {
            return;
        };
        let Some(shift_pc) = (0..view.len()).find(|pc| {
            view.instruction(*pc).is_some_and(|instruction| {
                instruction.opcode == crate::ir::Opcode::Binary
                    && crate::ir::compact_binary_operator(instruction.flags)
                        == Some(crate::ops::BinaryOp::ShiftLeft)
            })
        }) else {
            return;
        };
        let plan = super::BaselinePlan::compile_for_test(view, policy);
        let bitwise = plan.native_binary_at(binary_pc).expect("bitwise plan");
        let shift = plan.native_binary_at(shift_pc).expect("shift plan");
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
        assert!(bitwise.borrow().native_entry_count > 0);
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
    assert!(executed, "ordinary driver must execute lowered bitwise bytes");
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
                && view.instruction(*pc + 2).is_some_and(|instruction| {
                    instruction.opcode == crate::ir::Opcode::ASetI
                })
        }) else {
            return;
        };
        let load = view.instruction(pc).expect("indexed load");
        let add = view.instruction(pc + 1).expect("indexed add");
        let store = view.instruction(pc + 2).expect("indexed store");
        let plan = super::BaselinePlan::compile_for_test(view, policy);
        let region = plan.native_region_at(pc).expect("fused update admission");
        let expected_key = if add.opcode == crate::ir::Opcode::Add {
            crate::stencil_select::array_numeric_update_region_key()
        } else {
            crate::stencil_select::array_numeric_update_const_region_key()
        };
        assert_eq!(region.borrow().key_for_test(), expected_key);
        let mut registers = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        let array = crate::value::Value::Array(std::rc::Rc::new(
            crate::value::ArrayData::new(vec![crate::value::Value::Number(3.0)]),
        ));
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
            Some(7.0)
        );
        assert!(region.borrow().last_native_execution());
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
    assert!(executed, "ordinary lowering must expose the fused update shape");
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn ordinary_source_lowering_executes_numeric_comparison_and_falls_back_on_conversion() {
    let program = crate::reduce::reduce_source("var x = 1; x = x < 2; x;")
        .expect("comparison source lowers");
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut executed = false;
    let mut inspect = |view: crate::machine::CodeView<'_>| {
        if executed {
            return;
        }
        let Some(pc) = (0..view.len()).find(|pc| {
            view.instruction(*pc).is_some_and(|instruction| {
                instruction.opcode == crate::ir::Opcode::Binary
                    && crate::ir::compact_binary_operator(instruction.flags)
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
        assert_eq!(registers.read(usize::from(instruction.a)), Some(crate::value::Value::Boolean(true)));
        assert!(native.borrow().native_entry_count > 0, "comparison bytes must execute");

        let before = native.borrow().native_entry_count;
        let mut hostile = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        hostile.write(usize::from(instruction.b), crate::value::Value::String("1".into()));
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
        assert_eq!(hostile.read(usize::from(instruction.a)), Some(crate::value::Value::Boolean(true)));
        assert_eq!(native.borrow().native_entry_count, before, "fallback must not enter native bytes");
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
    assert!(executed, "ordinary source must reach and execute comparison bytes");
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
                instruction.opcode == crate::ir::Opcode::Binary
                    && crate::ir::compact_binary_operator(instruction.flags)
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
        registers.write(usize::from(instruction.b), crate::value::Value::Boolean(true));
        registers.write(usize::from(instruction.c), crate::value::Value::Boolean(true));
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
        assert!(native.borrow().native_entry_count > 0, "identity bytes must execute");

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
        assert!(object_result.is_ok(), "object identity comparison should execute");
        assert_eq!(
            object_registers.read(usize::from(instruction.a)),
            Some(crate::value::Value::Boolean(true))
        );

        let before = native.borrow().native_entry_count;
        let mut hostile = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        hostile.write(usize::from(instruction.b), crate::value::Value::Boolean(true));
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
    assert!(executed, "ordinary source must reach and execute identity bytes");
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
                instruction.opcode == crate::ir::Opcode::Binary
                    && crate::ir::compact_binary_operator(instruction.flags)
                        == Some(crate::ops::BinaryOp::StrictNotEqual)
            })
        }) else {
            return;
        };
        let plan = super::BaselinePlan::compile_for_test(view, policy);
        let Some(native) = plan.native_binary_at(pc) else {
            return;
        };
        let instruction = view.instruction(pc).expect("identity inequality instruction");
        let mut registers = crate::register_file::RegisterFile::with_undefined(
            usize::from(view.register_count()).max(8),
        );
        registers.write(usize::from(instruction.b), crate::value::Value::Boolean(true));
        registers.write(usize::from(instruction.c), crate::value::Value::Boolean(false));
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
        assert!(native.borrow().native_entry_count > 0, "inequality bytes must execute");
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
    assert!(executed, "ordinary source must reach and execute inequality bytes");
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_add_chain_executes_two_ops_with_one_entry() {
    let mut plan = super::NativeAddChainPlan {
        arena: None,
        shared_arena: None,
        cache: crate::stencil_select::RenderedRegionCache::new(),
        lifecycle: crate::stencil_lifecycle::StencilLifecycle::new(),
        site: crate::quickening::QuickeningSite::new(crate::ir::Opcode::Add),
        entry: None,
        shared_entry: None,
    };
    assert_eq!(plan.execute(1.5, 2.25, 4.0), Ok(7.75));
    assert!(plan.entry.is_some());
    let used = plan.arena.as_ref().expect("rendered chain").used();
    assert_eq!(plan.execute(-2.0, 3.0, 5.0), Ok(6.0));
    assert_eq!(plan.arena.as_ref().expect("cached chain").used(), used);
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_increment_uses_add_const_template_for_number_subset() {
    let instruction = crate::ir::Instruction::inc_i(0, 1, false);
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut plan = super::NativeBinaryPlan::new(instruction, policy)
        .expect("declared increment body");
    assert_eq!(plan.execute(4.5, 1.0), Ok(5.5));
    assert_eq!(plan.execute(-0.0, 1.0), Ok(1.0));
    assert!(plan.execute(f64::NAN, 1.0).unwrap().is_nan());
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_nullish_word_matches_canonical_tagged_values() {
    let instruction = crate::ir::Instruction::unary_operator(
        0,
        crate::ops::UnaryOp::IsNullish,
        1,
    );
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut plan = super::NativeNullishPlan::new(instruction, policy)
        .expect("declared nullish word body");
    assert_eq!(plan.execute(crate::tagged_value::TaggedValue::null().bits()), Ok(true));
    assert_eq!(
        plan.execute(crate::tagged_value::TaggedValue::undefined().bits()),
        Ok(true)
    );
    assert_eq!(plan.execute(crate::tagged_value::TaggedValue::bool(true).bits()), Ok(false));
    assert_eq!(plan.execute(crate::tagged_value::TaggedValue::number(0.0).bits()), Ok(false));
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_add_chain_shared_entry_reuses_owner_after_eviction() {
    let shared = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("slab"),
    ));
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let mut plan = super::NativeAddChainPlan::new_with_arena(policy, shared.clone())
        .expect("shared add chain");
    assert_eq!(plan.execute(1.0, 2.0, 3.0), Ok(6.0));
    let used = shared.borrow().used();
    assert!(plan.shared_entry.is_some());
    assert_eq!(plan.execute(2.0, 4.0, 8.0), Ok(14.0));
    assert_eq!(shared.borrow().used(), used);
    assert_eq!(shared.borrow_mut().evict_idle(0), 1);
    assert_eq!(plan.execute(2.0, 4.0, 8.0), Ok(14.0));
    assert!(plan.shared_entry.is_some());
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_move_uses_rendered_address_without_remapping() {
    let mut plan = super::NativeMovePlan {
        arena: None,
        shared_arena: None,
        cache: crate::stencil_select::RenderedRegionCache::new(),
        lifecycle: crate::stencil_lifecycle::StencilLifecycle::new(),
        site: crate::quickening::QuickeningSite::new(crate::ir::Opcode::Move),
        opcode: crate::ir::Opcode::Move,
        entry: None,
        shared_entry: None,
        native_entry_count: 0,
    };
    let source = crate::tagged_value::TaggedValue::from_bits(0x1234_5678_9ABC_DEF0);
    assert_eq!(plan.execute(&source), Ok(source.bits()));
    let used = plan.arena.as_ref().expect("rendered arena").used();
    assert!(used > 0);
    assert_eq!(plan.execute(&source), Ok(source.bits()));
    assert_eq!(plan.arena.as_ref().expect("cached arena").used(), used);
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
    let source = crate::tagged_value::TaggedValue::from_bits(0xCAFE_BABE);
    assert_eq!(plan.execute(&source), Ok(source.bits()));
    let used = shared.borrow().used();
    assert!(plan.shared_entry.is_some());
    assert_eq!(plan.execute(&source), Ok(source.bits()));
    assert_eq!(shared.borrow().used(), used);
    assert_eq!(shared.borrow_mut().evict_idle(0), 1);
    assert_eq!(plan.execute(&source), Ok(source.bits()));
    assert!(plan.shared_entry.is_some());
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
    let source = crate::tagged_value::TaggedValue::from_bits(0x1357_9BDF);
    assert_eq!(plan.execute(&source), Ok(source.bits()));
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
    let source = crate::tagged_value::TaggedValue::from_bits(0x2468_ACED);
    assert_eq!(plan.execute(&source), Ok(source.bits()));
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_property_uses_rendered_address_without_remapping() {
    let mut plan = super::NativePropertyPlan {
        arena: None,
        shared_arena: None,
        cache: crate::stencil_select::RenderedRegionCache::new(),
        lifecycle: crate::stencil_lifecycle::StencilLifecycle::new(),
        opcode: crate::ir::Opcode::GetN,
        entry: None,
        shared_entry: None,
        native_entry_count: 0,
    };
    let site = crate::quickening::QuickeningSite::<4>::new(crate::ir::Opcode::GetN);
    let slot = crate::register_file::SlotWord::new(super::Value::Number(42.5));
    assert_eq!(plan.execute(&slot, &site), Ok(crate::tagged_value::TaggedValue::number(42.5).bits()));
    assert_eq!(plan.native_entry_count, 1);
    let used = plan.arena.as_ref().expect("rendered arena").used();
    assert!(used > 0);
    assert_eq!(plan.execute(&slot, &site), Ok(crate::tagged_value::TaggedValue::number(42.5).bits()));
    assert_eq!(plan.native_entry_count, 2);
    assert_eq!(plan.arena.as_ref().expect("cached arena").used(), used);
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
    let slot = crate::register_file::SlotWord::new(super::Value::Number(42.5));
    assert!(plan.execute(&slot, &site).is_ok());
    let used = shared.borrow().used();
    assert!(plan.shared_entry.is_some());
    assert!(plan.execute(&slot, &site).is_ok());
    assert_eq!(shared.borrow().used(), used);
    assert_eq!(shared.borrow_mut().evict_idle(0), 1);
    assert!(plan.execute(&slot, &site).is_ok());
    assert!(plan.shared_entry.is_some(), "eviction must rebuild the entry");
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
    let object_data = std::rc::Rc::new(crate::value::ObjectData::new(vec![
        ("value".into(), crate::value::Value::Number(7.0)),
    ]));
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
    assert_eq!(completion, crate::completion::Completion::Return(crate::value::Value::Number(7.0)));
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
    assert_eq!(completion, crate::completion::Completion::Return(crate::value::Value::Number(7.0)));
    let native_count = plan
        .native_property_at(0)
        .map(|native| native.borrow().native_entry_count)
        .unwrap_or(0);
    assert!(native_count > 0);
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
    assert_eq!(completion, crate::completion::Completion::Return(crate::value::Value::Number(7.0)));
    assert_eq!(
        plan.native_property_at(0).map(|native| native.borrow().native_entry_count),
        Some(native_count),
        "shape mutation must not re-enter the stale native slot"
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
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
    let prototype = std::rc::Rc::new(crate::value::ObjectData::new(vec![
        ("value".into(), crate::value::Value::Number(11.0)),
    ]));
    let receiver = std::rc::Rc::new(crate::value::ObjectData::new(vec![
        ("\0prototype".into(), crate::value::Value::Object(prototype)),
    ]));
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
    assert!(plan
        .native_property_at(0)
        .is_some_and(|native| native.borrow().native_entry_count > 0));
    let native_count = plan
        .native_property_at(0)
        .map(|native| native.borrow().native_entry_count)
        .unwrap_or(0);
    let replacement = std::rc::Rc::new(crate::value::ObjectData::new(Vec::new()));
    assert!(crate::execute::set_prototype_of(
        &crate::value::Value::Object(receiver),
        &crate::value::Value::Object(replacement),
    )
    .is_ok());
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
        Some(native_count),
        "prototype replacement must not reuse stale native slot"
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
    assert!(std::rc::Rc::ptr_eq(&root.store, &body.store));
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
        dst: 2,
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
