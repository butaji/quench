use super::*;

#[test]
fn promoted_parameter_registers_survive_residual_round_trip() {
    let program = crate::Engine::specialize(
        "function read(value) { return value + 1; }",
        "promoted-registers.js",
    )
    .unwrap();
    let path = std::env::temp_dir().join(format!(
        "quench-promoted-registers-{}",
        std::process::id()
    ));
    program.write_binary(&path).unwrap();
    let decoded = ResidualProgram::read_binary(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(
        decoded.functions[1].local_registers,
        program.functions[1].local_registers
    );
    decoded.validate().unwrap();
}

#[test]
fn eval_binding_declarations_survive_residual_round_trip() {
    let source = r#"
    function outer(parameter) {
        try { throw 1; } catch (caught) {
            return function inner() { let lexical = 2; eval('parameter + caught + lexical'); };
        }
    }
    { let blocked = 3; with ({blocked: 4}) {
        blocked; blocked = 5; typeof blocked; delete blocked;
        let nested = 6; eval('nested');
    } }
"#;
    let program = crate::Engine::specialize(source, "eval-bindings.js").unwrap();
    let path = std::env::temp_dir().join(format!("quench-eval-bindings-{}", std::process::id()));
    program.write_binary(&path).unwrap();
    let encoded = std::fs::read(&path).unwrap();
    let decoded = ResidualProgram::read_binary(&path).unwrap();
    decoded.write_binary(&path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), encoded);
    std::fs::remove_file(path).unwrap();
    let declarations = decoded
        .functions
        .iter()
        .flat_map(|function| {
            function
                .name_bindings
                .iter()
                .chain(
                    function
                        .binding_sites
                        .iter()
                        .flat_map(|site| site.bindings.iter()),
                )
                .map(|binding| {
                    (
                        decoded.atoms[binding.atom as usize].as_ref(),
                        binding.declaration,
                    )
                })
        })
        .collect::<Vec<_>>();
    for (name, expected) in [
        ("parameter", EvalBindingDeclaration::Variable),
        ("caught", EvalBindingDeclaration::CatchParameter),
        ("lexical", EvalBindingDeclaration::Lexical),
        ("inner", EvalBindingDeclaration::Lexical),
        ("blocked", EvalBindingDeclaration::Lexical),
    ] {
        assert!(
            declarations.contains(&(name, expected)),
            "{name}: {declarations:?}"
        );
    }
    let nested = decoded
        .atoms
        .iter()
        .position(|atom| atom == "nested")
        .unwrap() as Atom;
    assert!(
        decoded
            .functions
            .iter()
            .flat_map(|function| {
                function
                    .binding_sites
                    .iter()
                    .flat_map(|site| &site.bindings)
            })
            .any(|binding| binding.atom == nested && binding.with_depth == 1)
    );
    let mut invalid = decoded.clone();
    let binding = invalid
        .functions
        .iter_mut()
        .flat_map(|function| {
            function
                .binding_sites
                .iter_mut()
                .flat_map(|site| &mut site.bindings)
        })
        .find(|binding| binding.atom == nested)
        .unwrap();
    binding.with_depth = u16::MAX;
    assert!(invalid.validate().is_err());
    assert!(EvalBindingDeclaration::from_binary_tag(u8::MAX).is_err());
}

#[test]
fn decoder_rejects_out_of_range_local_load() {
    let program = ResidualProgram {
        specialized: true,
        kind: crate::bytecode::ProgramKind::Script,
        module_requests: Vec::new(),
        module_imports: Vec::new(),
        module_link_plan: None,
        source_name: String::new(),
        atoms: AtomTable::default(),
        constants: vec![],
        functions: vec![Function {
            parent: None,
            name: None,
            is_arrow: false,
            self_binding_slot: None,
            source_text: None,
            params: 0,
            length: 0,
            parameter_end_pc: 0,
            parameter_atoms: vec![],
            rest: false,
            is_async: false,
            is_generator: false,
            is_class_constructor: false,
            derived_constructor: false,
            instance_initializer: None,
            super_home_atom: None,
            constructible: true,
            class_field_initializer: false,
            parameter_eval_arguments_error: false,
            arguments_slot: None,
            simple_parameters: true,
            strict: false,
            locals: 1,
            local_atoms: vec![],
            environment_atoms: vec![],
            selective_capture_slots: None,
            local_registers: Vec::new(),
            inherited_with_scope: false,
            lexical_atoms: vec![],
            global_lexical_atoms: vec![],
            global_var_atoms: vec![],
            global_function_atoms: vec![],
            global_annex_b_var_atoms: vec![],
            global_immutable_atoms: vec![],
            name_bindings: vec![],
            binding_sites: vec![],
            source_positions: vec![],
            environment_clones: vec![],
            code: vec![Instr::new(Op::LoadLocal, 0, 0, 0, 1)],
            wide: vec![],
            registers: 1,
            dispatch: DispatchClass::General,
            handlers: vec![],
            register_root_offset: 0,
        }],
        cache_sites: 0,
        method_sites: vec![],
        method_arguments: vec![],
        field_sites: vec![],
        object_sites: vec![],
        regexp_literal_sites: vec![],
        superinstructions: vec![],
        register_roots: vec![],
    };
    let path = std::env::temp_dir().join(format!("quench-invalid-local-{}", std::process::id()));
    program.write_binary(&path).unwrap();
    let result = ResidualProgram::read_binary(&path);
    std::fs::remove_file(path).unwrap();
    assert_eq!(result.unwrap_err(), "invalid residual local load");
}

#[test]
fn decoder_rejects_runtime_abi_mismatch_before_tables() {
    let program = ResidualProgram {
        specialized: true,
        kind: crate::bytecode::ProgramKind::Script,
        module_requests: Vec::new(),
        module_imports: Vec::new(),
        module_link_plan: None,
        source_name: String::new(),
        atoms: AtomTable::default(),
        constants: vec![],
        functions: vec![Function {
            parent: None,
            name: None,
            is_arrow: false,
            self_binding_slot: None,
            source_text: None,
            params: 0,
            length: 0,
            parameter_end_pc: 0,
            parameter_atoms: vec![],
            rest: false,
            is_async: false,
            is_generator: false,
            is_class_constructor: false,
            derived_constructor: false,
            instance_initializer: None,
            super_home_atom: None,
            constructible: true,
            class_field_initializer: false,
            parameter_eval_arguments_error: false,
            arguments_slot: None,
            simple_parameters: true,
            strict: false,
            locals: 0,
            local_atoms: vec![],
            environment_atoms: vec![],
            selective_capture_slots: None,
            local_registers: Vec::new(),
            inherited_with_scope: false,
            lexical_atoms: vec![],
            global_lexical_atoms: vec![],
            global_var_atoms: vec![],
            global_function_atoms: vec![],
            global_annex_b_var_atoms: vec![],
            global_immutable_atoms: vec![],
            name_bindings: vec![],
            binding_sites: vec![],
            source_positions: vec![],
            environment_clones: vec![],
            code: vec![Instr::new(Op::Return, 0, 0, 0, 0)],
            wide: vec![],
            registers: 1,
            dispatch: DispatchClass::General,
            handlers: vec![],
            register_root_offset: u32::MAX,
        }],
        cache_sites: 0,
        method_sites: vec![],
        method_arguments: vec![],
        field_sites: vec![],
        object_sites: vec![],
        regexp_literal_sites: vec![],
        superinstructions: vec![],
        register_roots: vec![],
    };
    let path = std::env::temp_dir().join(format!("quench-invalid-abi-{}", std::process::id()));
    program.write_binary(&path).unwrap();
    let mut bytes = std::fs::read(&path).unwrap();
    let abi_bytes = ResidualProgram::RUNTIME_ABI_FINGERPRINT.to_le_bytes();
    let abi_offset = bytes
        .windows(abi_bytes.len())
        .position(|window| window == abi_bytes)
        .expect("serialized residual contains its runtime ABI fingerprint");
    bytes[abi_offset] ^= 1;
    std::fs::write(&path, bytes).unwrap();
    let result = ResidualProgram::read_binary(&path);
    std::fs::remove_file(path).unwrap();
    assert_eq!(result.unwrap_err(), "residual runtime ABI mismatch");
}

#[test]
fn decoder_rejects_unknown_property_definition_modes() {
    for wide in [false, true] {
        let mut program = crate::Engine::specialize(
            "class Check {method() {return 42;}}",
            "invalid-definition-mode.js",
        )
        .unwrap();
        let path = std::env::temp_dir().join(format!(
            "quench-invalid-definition-mode-{}-{wide}",
            std::process::id()
        ));
        program.write_binary(&path).unwrap();
        ResidualProgram::read_binary(&path).unwrap();
        let function = program
            .functions
            .iter_mut()
            .find(|f| f.code.iter().any(|i| i.op() == Op::DefinePropertyRecord))
            .unwrap();
        let instruction = function
            .code
            .iter_mut()
            .find(|i| i.op() == Op::DefinePropertyRecord)
            .unwrap();
        let invalid = u32::try_from(PropertyDefinitionMode::ALL.len()).unwrap();
        if wide {
            let index = function.wide.len();
            function.wide.push(WideInstruction::new(
                Op::DefinePropertyRecord,
                instruction.a(),
                instruction.b(),
                instruction.c(),
                invalid,
            ));
            *instruction = Instr::wide(index).unwrap();
        } else {
            *instruction = Instr::new(
                Op::DefinePropertyRecord,
                instruction.a(),
                instruction.b(),
                instruction.c(),
                invalid,
            );
        }
        program.write_binary(&path).unwrap();
        let result = ResidualProgram::read_binary(&path);
        std::fs::remove_file(path).unwrap();
        assert!(
            result
                .unwrap_err()
                .contains("DefinePropertyRecord has an out-of-domain operand")
        );
    }
}

#[test]
fn instance_initializer_plan_survives_round_trip_and_rejects_invalid_owners() {
    let program = crate::Engine::specialize(
        "class Base {} class Derived extends Base {field=1; constructor(){(()=>super())();}}",
        "instance-initializer.js",
    )
    .unwrap();
    let owner = program
        .functions
        .iter()
        .position(|f| f.instance_initializer.is_some())
        .unwrap();
    let initializer = program.functions[owner].instance_initializer.unwrap();
    let path = std::env::temp_dir().join(format!(
        "quench-instance-initializer-{}",
        std::process::id()
    ));
    program.write_binary(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let decoded = ResidualProgram::read_binary(&path).unwrap();
    assert_eq!(
        decoded.functions[owner].instance_initializer,
        Some(initializer)
    );
    decoded.write_binary(&path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    for plan in [program.functions.len() as u32, owner as u32, 0] {
        let mut invalid = program.clone();
        invalid.functions[owner].instance_initializer = Some(plan);
        invalid.write_binary(&path).unwrap();
        assert!(ResidualProgram::read_binary(&path).is_err());
    }
    let mut invalid = program;
    invalid.functions[initializer as usize].parent = Some(owner as u32);
    assert!(invalid.validate().is_err());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn scoped_binding_sites_survive_residual_round_trip() {
    let source = "for (let x of [1]) { with ({}) { x++; } }";
    for generic in [false, true] {
        let program = if generic {
            crate::Engine::specialize_unspecialized(source, "binding-sites.js")
        } else {
            crate::Engine::specialize(source, "binding-sites.js")
        }
        .unwrap();
        let path = std::env::temp_dir().join(format!(
            "quench-binding-sites-{}-{generic}",
            std::process::id()
        ));
        program.write_binary(&path).unwrap();
        let decoded = ResidualProgram::read_binary(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(
            program
                .functions
                .iter()
                .any(|function| !function.binding_sites.is_empty())
        );
        for (before, after) in program.functions.iter().zip(&decoded.functions) {
            assert_eq!(before.binding_sites, after.binding_sites);
        }
        crate::Runtime::new(crate::SystemHost)
            .execute(&decoded)
            .unwrap();
    }
}

#[test]
fn decoder_rejects_invalid_binding_site_metadata() {
    let program = crate::Engine::specialize(
        "for (let x of [1]) { with ({}) { x++; } }",
        "binding-sites.js",
    )
    .unwrap();
    let path = std::env::temp_dir().join(format!(
        "quench-invalid-binding-sites-{}",
        std::process::id()
    ));
    for corrupt in 0..4 {
        let mut invalid = program.clone();
        let function = invalid
            .functions
            .iter_mut()
            .find(|f| !f.binding_sites.is_empty())
            .unwrap();
        match corrupt {
            0 => function.binding_sites[0].resume_pc = 0,
            1 => function.binding_sites[0].resume_pc = function.code.len() as u32 + 1,
            2 => {
                function.binding_sites[0].bindings[0].location =
                    EvalBindingLocation::Local(function.locals)
            }
            3 => function
                .binding_sites
                .insert(0, function.binding_sites[0].clone()),
            _ => unreachable!(),
        }
        invalid.write_binary(&path).unwrap();
        let error = ResidualProgram::read_binary(&path).unwrap_err();
        assert!(
            error.contains("invalid binding-site metadata"),
            "unexpected decoder error: {error}"
        );
    }
    std::fs::remove_file(path).unwrap();
}
