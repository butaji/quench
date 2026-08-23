#[test]
fn call_frame_suspend_and_resume_restores_caller_state() {
    let function = super::FunctionCode::from_ops(vec![
        super::Op::ParameterEnd,
        super::Op::ParameterEnd,
    ]);
    let mut machine = super::Machine::with_function(&function, super::EnvironmentRef(7), 2);
    machine.set_program_counter(1);
    machine.registers_mut()[0] = super::Value::Number(11.0);
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
    assert_eq!(machine.registers_mut()[0], super::Value::Number(11.0));
    assert_eq!(machine.registers_mut()[1], super::Value::Number(42.0));
    assert!(machine.call_frames.is_empty());
}

#[test]
fn machine_rejects_call_continuation_from_unknown_code_source() {
    let function = super::FunctionCode::from_ops(vec![super::Op::ParameterEnd]);
    let mut machine = Machine::with_function(&function, EnvironmentRef(0), 1);
    machine.push_call_frame(crate::completion::CallContinuation {
        callee: super::Value::Undefined,
        receiver: super::Value::Undefined,
        arguments: Vec::new(),
        caller_code: super::CodeId(99),
        caller_pc: 0,
        caller_registers: Vec::new(),
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
    assert_eq!(machine.store.as_ref().and_then(|store| store.code(function.range)).map(|code| code.len()), Some(1));
}
#[test]
fn machine_rejects_frame_ranges_not_owned_by_its_code_store() {
    let function = super::FunctionCode::pending(vec![super::Op::ParameterEnd]);
    let mut machine = Machine::with_function(&function, EnvironmentRef(0), 1);
    let invalid = super::CodeRange::new(super::CodeId(99), 0, 1).unwrap();
    let frame = super::Frame::Await { phase: 0, resume: invalid };
    assert!(machine.try_push_frame(frame).is_err());
    assert_eq!(machine.frame_count(), 0);
}

#[test]
fn machine_accepts_frame_ranges_from_its_immutable_store() {
    let function = super::FunctionCode::from_ops(vec![super::Op::ParameterEnd]);
    let mut machine = Machine::with_function(&function, EnvironmentRef(0), 1);
    let frame = super::Frame::Await { phase: 0, resume: function.range };
    machine.try_push_frame(frame).unwrap();
    assert_eq!(machine.frame_count(), 1);
}

#[test]
fn code_arena_can_import_existing_function_ranges() {
    let function = super::FunctionCode::pending(vec![super::Op::ParameterEnd]);
    let mut arena = super::CodeArena::new();
    let range = arena.append_function(&function).expect("function range");
    assert_eq!(arena.freeze().code(range).map(|code| code.len()), Some(1));
}

#[test]
fn linked_nested_bodies_share_one_immutable_code_store() {
    let child = super::FunctionCode::pending(vec![super::Op::ParameterEnd]);
    let root = super::FunctionCode::from_ops(vec![super::Op::IteratorBinding {
        iterator: 0,
        body: child,
        close_normal: false,
    }]);
    let Some(super::Op::IteratorBinding { body, .. }) = root.code().and_then(|code| code.cold_at(0)) else {
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
        })
        .unwrap();
    stack
        .try_push(super::Frame::Await {
            phase: 0,
            resume: range,
        })
        .unwrap();
    let capacity = stack.capacity();
    assert!(stack
        .try_push(super::Frame::Await {
            phase: 0,
            resume: range,
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
        })
        .unwrap();
    let first = stack.top_offset().expect("first frame offset");
    let first_ptr = stack.frame_at(first).expect("first frame");
    assert!(matches!(
        first_ptr,
        super::Frame::Await { phase: 7, .. }
    ));

    // Force Vec growth/reallocation. The offset, rather than a borrowed
    // reference, is the continuation identity and must still resolve.
    stack
        .try_push(super::Frame::Await {
            phase: 9,
            resume: range,
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
    let frame = super::Frame::Await { phase: 0, resume: range };
    assert!(frame.register_ids().is_empty());
    assert!(frame.has_valid_register_ids(0));
}
