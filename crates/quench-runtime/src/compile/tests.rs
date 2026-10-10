use super::*;

#[test]
fn regression_global_var_lowering_checks_instruction_roles_before_slots() {
    let wide_count = (1..=u16::MAX)
        .find(|&count| Instr::try_new(Op::MakeConstArray, 0, count, 0, 0).is_none())
        .unwrap();
    let wide_array = format!("[{}];", "0,".repeat(usize::from(wide_count)));
    for suffix in ["", wide_array.as_str()] {
        let source = format!(
            "var global = 1; let lexical = 2; global = global + lexical; \
             function read() {{ return global; }} read(); {suffix}"
        );
        for specialize in [false, true] {
            let program = if specialize {
                Engine::specialize(&source, "global-lowering.js")
            } else {
                Engine::specialize_unspecialized(&source, "global-lowering.js")
            }
            .unwrap();
            let root = &program.functions[0];
            assert!(
                root.code
                    .iter()
                    .any(|instruction| instruction.op() == Op::StoreEnvLocal)
            );
            assert!(!root.global_lexical_atoms.is_empty());
            assert!(
                root.global_lexical_atoms
                    .iter()
                    .all(|atom| !root.global_var_atoms.contains(atom))
            );
            assert_eq!(!root.wide.is_empty(), !suffix.is_empty());
            program.validate().unwrap();
        }
    }
}

#[test]
fn scalar_constants_reuse_exact_slots() {
    let mut compiler = Compiler::new_with_mode(
        "test.js",
        "",
        SpecializationMode::Enabled,
        &[],
        FxHashMap::default(),
    );
    assert_eq!(compiler.constant(Constant::Number(1.0)), 0);
    assert_eq!(compiler.constant(Constant::String("x".into())), 1);
    assert_eq!(compiler.constant(Constant::Number(1.0)), 0);
    assert_eq!(compiler.constant(Constant::String("x".into())), 1);
    assert_eq!(compiler.constants.len(), 2);
}

#[test]
fn scalar_constants_preserve_signed_zero_bits() {
    let mut compiler = Compiler::new_with_mode(
        "test.js",
        "",
        SpecializationMode::Enabled,
        &[],
        FxHashMap::default(),
    );
    assert_ne!(
        compiler.constant(Constant::Number(0.0)),
        compiler.constant(Constant::Number(-0.0))
    );
}

#[test]
fn fixed_register_parameter_promotion_excludes_mutation_and_dynamic_name_ops() {
    for body in [
        "value = 2;",
        "value += 2;",
        "value++;",
        "for (value of [2]) {}",
        "({ field: value } = { field: 2 });",
        "delete value;",
    ] {
        let source = format!("function update(value) {{ {body} return value; }}");
        let program = Engine::specialize(&source, "mutable-parameter.js").unwrap();
        assert!(
            program.functions[1].local_registers.is_empty(),
            "write should keep parameter in its local slot: {body}"
        );
        program.validate().unwrap();
    }

    let program = Engine::specialize(
        "function read(value) { var other = 2; return value + other; }",
        "immutable-parameter.js",
    )
    .unwrap();
    assert_eq!(program.functions[1].local_registers.len(), 1);
    program.validate().unwrap();
}

#[test]
fn validator_rejects_bytecode_that_overwrites_a_promoted_parameter() {
    let mut program = Engine::specialize(
        "function read(value) { return value + 1; }",
        "promoted-register-validation.js",
    )
    .unwrap();
    let function = &mut program.functions[1];
    let binary = function
        .code
        .iter_mut()
        .find(|instruction| instruction.op() == Op::Binary)
        .unwrap();
    binary.set_result_register(0);
    assert!(program.validate().is_err());
}


#[test]
fn constant_runs_remain_fresh_and_contiguous() {
    let mut compiler = Compiler::new_with_mode(
        "test.js",
        "",
        SpecializationMode::Enabled,
        &[],
        FxHashMap::default(),
    );
    assert_eq!(compiler.constant(Constant::Number(1.0)), 0);
    let start = compiler.constant_run(vec![Constant::Number(1.0), Constant::Number(1.0)]);
    assert_eq!(start, 1);
    assert_eq!(compiler.constants.len(), 3);
    assert_eq!(compiler.constant(Constant::Number(1.0)), 0);
}

#[test]
fn packed_domain_overflow_uses_the_wide_side_table() {
    let source = format!("[{}];", "0,".repeat(4097));
    let program = Engine::specialize(&source, "packed-overflow.js").unwrap();
    assert!(
        program
            .functions
            .iter()
            .any(|function| !function.wide.is_empty())
    );
}

#[test]
fn regexp_literals_lower_to_validated_site_metadata() {
    let program = Engine::specialize(
        "function make() { return function inner() { return /a/g; }; } make()();",
        "regexp-site.js",
    )
    .unwrap();
    assert_eq!(program.regexp_literal_sites.len(), 1);
    let site = &program.regexp_literal_sites[0];
    assert!(matches!(
        program.constants.get(site.pattern_constant as usize),
        Some(Constant::String(pattern)) if pattern == "a"
    ));
    assert!(matches!(
        program.constants.get(site.flags_constant as usize),
        Some(Constant::String(flags)) if flags == "g"
    ));
    assert!(program.functions.iter().any(|function| {
        function
            .code
            .iter()
            .any(|instruction| instruction.op() == Op::CreateRegExpLiteral)
    }));
    assert!(
        program
            .functions
            .iter()
            .flat_map(|function| &function.code)
            .all(|instruction| instruction.op() != Op::Construct)
    );
    program.validate().unwrap();

    let path = std::env::temp_dir().join(format!("quench-regexp-site-{}", std::process::id()));
    program.write_binary(&path).unwrap();
    let decoded = ResidualProgram::read_binary(&path).unwrap();
    assert_eq!(decoded.regexp_literal_sites, program.regexp_literal_sites);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn constant_computed_property_uses_field_cache_site() {
    let program = Engine::specialize(
        "var object = { answer: 42 }; print(object['answer']);",
        "computed.js",
    )
    .unwrap();
    assert!(
        program.functions[0]
            .code
            .iter()
            .any(|instruction| instruction.op() == Op::GetField)
    );
    assert!(program.field_sites.is_empty());
    assert!(program.cache_sites > 0);
}

#[test]
fn unspecialized_entry_keeps_generic_dispatch_class() {
    let program = Engine::specialize_unspecialized("print(40 + 2);", "generic.js").unwrap();
    assert!(!program.specialized);
    assert!(
        program
            .functions
            .iter()
            .all(|function| function.dispatch == DispatchClass::General)
    );
}
