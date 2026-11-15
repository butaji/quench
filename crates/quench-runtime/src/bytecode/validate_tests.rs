use crate::bytecode::{AtomTable, DispatchClass, Function, Instr, Op, ResidualProgram};

#[test]
fn rest_list_closure_layouts_are_validated() {
    // The compiler splits a rest list that binds directly; the validator accepts what it emits.
    let program = crate::Engine::specialize(
        "function outer() { function log(first, ...rest) { let hidden = rest.length; return () => first + rest[0] + hidden; } return log(1, 2)(); } outer();",
        "rest-layout.js",
    )
    .unwrap();
    // Validation compiles every released function first.
    assert_eq!(program.validate(), Ok(()));
    assert!(program
        .functions
        .iter()
        .any(|function| function.rest && function.selective_capture_slots.is_some()));
}

#[test]
fn non_suspending_async_promoted_locals_are_validated() {
    // A non-suspending async body promotes its locals as an ordinary one does; one that
    // awaits keeps them in frame slots. The validator accepts what the compiler emits, for
    // default and trailing-comma parameter lists too (Test262 async dflt-params-*,
    // params-trailing-comma-*).
    let program = crate::Engine::specialize(
        "async function trailing(a,) { return a; } async function defaults(a, b = a + 1) { let c = b; return c; } \
         const arrow = async (x = 1, y,) => x + y; async function waits(a) { const b = await a; return b; } \
         trailing(1); defaults(1); arrow(2, 3); waits(4);",
        "async-promoted-locals.js",
    )
    .unwrap();
    // Validation compiles every released function first.
    assert_eq!(program.validate(), Ok(()));
    let promoted = |name: &str| {
        program.functions.iter().any(|function| {
            function.is_async
                && !function.local_registers.is_empty()
                && function.header().name.is_some_and(|atom| &program.atoms[atom as usize] == name)
        })
    };
    assert!(promoted("trailing"));
    assert!(
        program.functions.iter().any(|function| function.is_async && function.is_arrow && !function.local_registers.is_empty()),
        "a non-suspending async arrow with defaults and a trailing comma promotes its locals"
    );
    assert!(!promoted("waits"), "an awaiting body keeps its locals in frame slots");
}

#[test]
fn binding_site_domains_are_validated() {
    let program = crate::Engine::specialize(
        "{let value = 1; with ({}) {value; value = 2; typeof value; delete value;}}",
        "binding-sites.js",
    )
    .unwrap();
    assert!(program.validate().is_ok());
    assert!(program.functions[0].binding_sites.len() >= 2);
    for pc in [0, u32::MAX] {
        let mut invalid = program.clone();
        invalid.functions[0].binding_sites[0].resume_pc = pc;
        assert!(invalid.validate().is_err());
    }
    let mut invalid = program.clone();
    invalid.functions[0].binding_sites.reverse();
    assert!(invalid.validate().is_err());
    let mut invalid = program.clone();
    invalid.functions[0].binding_sites[0].bindings[0].atom = u32::MAX;
    assert!(invalid.validate().is_err());
    let mut invalid = program.clone();
    invalid.functions[0].binding_sites[0].bindings[0].location =
        super::super::EvalBindingLocation::Local(u16::MAX);
    assert!(invalid.validate().is_err());
    let mut invalid = program;
    let binding = invalid.functions[0].binding_sites[0].bindings[0];
    invalid.functions[0].binding_sites[0].bindings.push(binding);
    assert!(invalid.validate().is_err());
}

#[test]
fn name_binding_domains_are_validated() {
    use crate::bytecode::EvalBindingLocation;
    let program = crate::Engine::specialize(
        "function outer() { const captured = 1; return function inner() { eval('0'); return captured; }; }",
        "name-bindings.js",
    ).unwrap();
    assert!(program.validate().is_ok());
    let owner = program
        .functions
        .iter()
        .position(|function| function.name_bindings.resolved().len() >= 2)
        .unwrap();
    let with_bindings = |edit: &dyn Fn(&mut Vec<crate::bytecode::EvalBinding>)| {
        let mut invalid = program.clone();
        let mut bindings = invalid.functions[owner].name_bindings.resolved();
        edit(&mut bindings);
        invalid.functions[owner].name_bindings = crate::bytecode::NameBindings::flat(bindings);
        invalid
    };
    assert!(with_bindings(&|_| {}).validate().is_ok());
    for location in [
        EvalBindingLocation::Local(u16::MAX),
        EvalBindingLocation::Capture {
            depth: u16::MAX,
            slot: 0,
        },
        EvalBindingLocation::Capture {
            depth: 0,
            slot: u16::MAX,
        },
    ] {
        assert!(
            with_bindings(&|bindings| bindings[0].location = location)
                .validate()
                .is_err()
        );
    }
    assert!(
        with_bindings(&|bindings| bindings.reverse())
            .validate()
            .is_err()
    );
    assert!(
        with_bindings(&|bindings| bindings[1] = bindings[0])
            .validate()
            .is_err()
    );
    assert!(
        with_bindings(&|bindings| bindings[0].atom = u32::MAX)
            .validate()
            .is_err()
    );
}

#[test]
fn environment_clone_domains_are_validated() {
    let program = crate::Engine::specialize(
        "var readers = []; for (let i = 0; i < 2; i++) readers.push(() => i);",
        "clones.js",
    )
    .unwrap();
    assert!(program.validate().is_ok());
    for slots in [vec![u16::MAX], vec![0, 0], vec![1, 0]] {
        let mut invalid = program.clone();
        invalid.functions[0].environment_clones[0].fresh = slots;
        assert!(invalid.validate().is_err());
    }
    let mut invalid = program;
    invalid.functions[0].environment_clones = Default::default();
    assert!(invalid.validate().is_err());
}

fn function(code: Vec<Instr>, registers: u16, root: u32) -> Function {
    Function {
        parent: None,
        name: None,
        inferred_name: None,
        is_arrow: false,
        self_binding_slot: None,
        source_span: None,
        params: 0,
        length: 0,
        parameter_end_pc: 0,
        parameter_atoms: vec![].into(),
        rest: false,
        is_async: false,
        is_generator: false,
        is_class_constructor: false,
        derived_constructor: false,
        instance_initializer: None,
        super_home_atom: None,
        constructible: true,
        class_initializer: false,
        parameter_eval_arguments_error: false,
        unaliased_arguments: false,
        prototype_unobservable: false,
        arguments_slot: None,
        simple_parameters: true,
        strict: false,
        locals: 0,
        local_atoms: vec![].into(),
        environment_atoms: vec![].into(),
        selective_capture_slots: None,
        local_registers: vec![].into(),
        inherited_with_scope: false,
        lexical_atoms: vec![].into(),
        global_declarations: None,
        name_bindings: Default::default(),
        binding_sites: vec![].into(),
        source_positions: Default::default(),
        environment_clones: vec![].into(),
        code,
        wide: vec![],
        registers,
        dispatch: DispatchClass::General,
        decoded: Default::default(),
        plain_locals: Default::default(),
        declared_atom_sets: Default::default(),
        activation_bindings: Default::default(),
        live_parameter_reflection: Default::default(),
        static_name_scope: Default::default(),
        dynamic_scope_access: Default::default(),
        js_tier_candidate: Default::default(),
        call_plan: Default::default(),
        initializer: Default::default(),
        handlers: vec![].into(),
        register_roots: crate::bytecode::RegisterRoots::new(root != u32::MAX),
        undefined_register: None,
        parameter_registers: None,
        initial_register: Default::default(),
    }
}

fn program(function: Function) -> ResidualProgram {
    ResidualProgram {
        specialized: true,
        kind: crate::bytecode::ProgramKind::Script,
        module_requests: Vec::new(),
        module_imports: Vec::new(),
        module_link_plan: None,
        source_name: String::new(),
        source: None,
        atoms: AtomTable::default(),
        constants: vec![].into(),
        functions: vec![function.into()].into(),
        cache_sites: Default::default(),
        method_sites: vec![].into(),
        method_arguments: vec![].into(),
        field_sites: vec![].into(),
        object_sites: vec![].into(),
        regexp_literal_sites: vec![].into(),
        literal_templates: Vec::new().into(),
        superinstructions: vec![].into(), rewrite_ranges: Default::default(),
        replay: None, first_call_scopes: Default::default(),
        encoded_records: None,
        validated: Default::default(),
        undefined_constant: Default::default(),
        load_encoding: Default::default(),
        closure_reads: None,
        wasm_lazy: None,
    }
}

#[test]
fn table_and_operand_references_are_checked_before_execution() {
    for (op, minimum) in [
        (
            Op::WasmExceptionNew,
            crate::wasm::tag::ExceptionInput::MIN_COUNT,
        ),
        (
            Op::WasmStructNewDesc,
            u16::from(crate::wasm::gc::StructConstruction::Described.described()),
        ),
        (
            Op::WasmStructNewDefaultDesc,
            u16::from(crate::wasm::gc::StructConstruction::DefaultDescribed.described()),
        ),
    ] {
        for count in [0, minimum] {
            let constructor = program(
                function(
                    vec![
                        Instr::new(op, 0, 0, count, 0),
                        Instr::new(Op::Return, 0, 0, 0, 0),
                    ],
                    1,
                    u32::MAX,
                ));
            assert_eq!(constructor.validate().is_ok(), count != 0);
        }
    }
    for operator in [
        crate::wasm::atomic::AtomicOperator::I32AtomicLoad,
        crate::wasm::atomic::AtomicOperator::I64AtomicStore,
        crate::wasm::atomic::AtomicOperator::I32AtomicRmwAdd,
        crate::wasm::atomic::AtomicOperator::I64AtomicRmw32CmpxchgU,
        crate::wasm::atomic::AtomicOperator::MemoryAtomicNotify,
        crate::wasm::atomic::AtomicOperator::MemoryAtomicWait32,
        crate::wasm::atomic::AtomicOperator::MemoryAtomicWait64,
    ] {
        let expected = operator.input_count();
        for count in [0, expected - 1, expected, expected + 1] {
            let access = program(
                function(
                    vec![
                        Instr::new(Op::WasmAtomicAccess, 0, 0, count, operator as u32),
                        Instr::new(Op::Return, 0, 0, 0, 0),
                    ],
                    expected + 1,
                    u32::MAX,
                ));
            assert_eq!(access.validate().is_ok(), count == expected);
        }
    }
    let invalid_atomic = program(
        function(
            vec![
                Instr::new(Op::WasmAtomicAccess, 0, 0, 2, u32::from(u8::MAX)),
                Instr::new(Op::Return, 0, 0, 0, 0),
            ],
            4,
            u32::MAX,
        ));
    assert!(invalid_atomic.validate().is_err());
    let invalid_constant = program(
        function(vec![Instr::new(Op::LoadConst, 0, 0, 0, 0)], 1, u32::MAX));
    assert!(invalid_constant.validate().is_err());

    let mut invalid_shuffle = program(
        function(
            vec![
                Instr::new(Op::WasmSimdShuffle, 0, 0, 0, 0),
                Instr::new(Op::Return, 0, 0, 0, 0),
            ],
            1,
            u32::MAX,
        ));
    invalid_shuffle
        .constants
        .push(super::super::Constant::WasmV128(
            [u8::MAX; crate::wasm::V128_BYTES],
        ));
    assert!(invalid_shuffle.validate().is_err());
    invalid_shuffle.constants[0] =
        super::super::Constant::WasmV128([0; crate::wasm::V128_BYTES]);
    assert!(invalid_shuffle.validate().is_ok());
    let mut invalid_selector = program(
        function(
            vec![
                Instr::new(Op::WasmSimd, 0, 0, 0, u32::from(u8::MAX)),
                Instr::new(Op::Return, 0, 0, 0, 0),
            ],
            1,
            u32::MAX,
        ));
    assert!(invalid_selector.validate().is_err());
    invalid_selector.functions[0].code[0] =
        Instr::new(Op::WasmI31, 0, 0, 0, u32::from(u8::MAX));
    assert!(invalid_selector.validate().is_err());
    invalid_selector.functions[0].code[0] = Instr::new(
        Op::WasmI31,
        0,
        0,
        0,
        crate::wasm::i31::I31Operator::New as u32,
    );
    assert!(invalid_selector.validate().is_ok());
    invalid_selector.functions[0].code[0] =
        Instr::new(Op::WasmRefAsNonNull, 0, 0, 0, u32::from(u8::MAX));
    assert!(invalid_selector.validate().is_err());
    for check in [
        crate::wasm::reference::NonNullCheck::Reference,
        crate::wasm::reference::NonNullCheck::Function,
    ] {
        invalid_selector.functions[0].code[0] =
            Instr::new(Op::WasmRefAsNonNull, 0, 0, 0, check as u32);
        assert!(invalid_selector.validate().is_ok());
    }
    invalid_selector.functions[0].code[0] =
        Instr::new(Op::WasmRefCast, 0, 0, 0, crate::wasm::reference::EXACT);
    assert!(invalid_selector.validate().is_err());
    invalid_selector.functions[0].code[0] = Instr::new(
        Op::WasmRefCast,
        0,
        0,
        0,
        crate::wasm::reference::ReferenceTarget::from_type(wasmparser::RefType::I31REF)
            .unwrap()
            .tag(),
    );
    assert!(invalid_selector.validate().is_ok());
    invalid_selector.functions[0].code[0] = Instr::new(Op::WasmSimd, 0, 0, 0, 0);
    assert!(invalid_selector.validate().is_ok());

    let invalid_field = program(
        function(vec![Instr::new(Op::GetField, 0, 0, 0, 0)], 1, u32::MAX));
    assert!(invalid_field.validate().is_err());

    let invalid_call = program(
        function(
            vec![Instr::new(
                Op::Call,
                0,
                0,
                0,
                super::super::ImmediateLayout::call_immediate(1, 1, false, false),
            )],
            1,
            u32::MAX,
        ));
    assert!(invalid_call.validate().is_err());
}

#[test]
fn reachable_control_flow_cannot_fall_off_code() {
    let fallthrough = program(
        function(vec![Instr::new(Op::Nop, 0, 0, 0, 0)], 1, u32::MAX));
    assert!(fallthrough.validate().is_err());

    let branch_fallthrough = program(
        function(vec![Instr::new(Op::JumpFalse, 0, 0, 0, 0)], 1, u32::MAX));
    assert!(branch_fallthrough.validate().is_err());

    let loop_forever = program(
        function(vec![Instr::new(Op::Jump, 0, 0, 0, 0)], 1, u32::MAX));
    assert!(loop_forever.validate().is_ok());
}
