use super::*;

#[test]
fn decoder_rejects_out_of_range_local_load() {
    let program = ResidualProgram {
        atoms: AtomTable::default(),
        constants: vec![],
        functions: vec![Function {
            parent: None,
            name: None,
            params: 0,
            locals: 1,
            code: vec![Instr::new(Op::LoadLocal, 0, 0, 0, 1)],
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
