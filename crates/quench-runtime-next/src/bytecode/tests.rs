use super::*;

#[test]
fn decoder_rejects_out_of_range_local_load() {
    let program = ResidualProgram {
        specialized: true,
        module: false,
        module_requests: Vec::new(),
        module_imports: Vec::new(),
        module_link_plan: None,
        source_name: String::new(),
        atoms: AtomTable::default(),
        constants: vec![],
        functions: vec![Function {
            parent: None,
            name: None,
            params: 0,
            length: 0,
            parameter_end_pc: 0,
            parameter_atoms: vec![],
            rest: false,
            is_async: false,
            is_generator: false,
            is_class_constructor: false,
            derived_constructor: false,
            super_home_atom: None,
            constructible: true,
            class_field_initializer: false,
            arguments_slot: None,
            strict: false,
            locals: 1,
            local_atoms: vec![],
            lexical_atoms: vec![],
            global_lexical_atoms: vec![],
            global_var_atoms: vec![],
            global_function_atoms: vec![],
            global_immutable_atoms: vec![],
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
        superinstructions: vec![],
        register_roots: vec![],
    };
    let path = std::env::temp_dir().join(format!("rqj-invalid-local-{}", std::process::id()));
    program.write_binary(&path).unwrap();
    let result = ResidualProgram::read_binary(&path);
    std::fs::remove_file(path).unwrap();
    assert_eq!(result.unwrap_err(), "invalid residual local load");
}

#[test]
fn decoder_rejects_runtime_abi_mismatch_before_tables() {
    let program = ResidualProgram {
        specialized: true,
        module: false,
        module_requests: Vec::new(),
        module_imports: Vec::new(),
        module_link_plan: None,
        source_name: String::new(),
        atoms: AtomTable::default(),
        constants: vec![],
        functions: vec![Function {
            parent: None,
            name: None,
            params: 0,
            length: 0,
            parameter_end_pc: 0,
            parameter_atoms: vec![],
            rest: false,
            is_async: false,
            is_generator: false,
            is_class_constructor: false,
            derived_constructor: false,
            super_home_atom: None,
            constructible: true,
            class_field_initializer: false,
            arguments_slot: None,
            strict: false,
            locals: 0,
            local_atoms: vec![],
            lexical_atoms: vec![],
            global_lexical_atoms: vec![],
            global_var_atoms: vec![],
            global_function_atoms: vec![],
            global_immutable_atoms: vec![],
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
        superinstructions: vec![],
        register_roots: vec![],
    };
    let path = std::env::temp_dir().join(format!("rqj-invalid-abi-{}", std::process::id()));
    program.write_binary(&path).unwrap();
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[5] ^= 1;
    std::fs::write(&path, bytes).unwrap();
    let result = ResidualProgram::read_binary(&path);
    std::fs::remove_file(path).unwrap();
    assert_eq!(result.unwrap_err(), "residual runtime ABI mismatch");
}
