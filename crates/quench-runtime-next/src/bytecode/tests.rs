use super::*;

#[test]
fn decoder_rejects_out_of_range_local_load() {
    let program = ResidualProgram {
        specialized: true,
        atoms: AtomTable::default(),
        constants: vec![],
        functions: vec![Function {
            parent: None,
            name: None,
            params: 0,
            rest: false,
            is_async: false,
            is_generator: false,
            arguments_slot: None,
            locals: 1,
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
        atoms: AtomTable::default(),
        constants: vec![],
        functions: vec![Function {
            parent: None,
            name: None,
            params: 0,
            rest: false,
            is_async: false,
            is_generator: false,
            arguments_slot: None,
            locals: 0,
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
