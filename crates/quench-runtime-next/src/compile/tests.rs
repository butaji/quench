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
        String::new(),
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
        String::new(),
    );
    assert_ne!(
        compiler.constant(Constant::Number(0.0)),
        compiler.constant(Constant::Number(-0.0))
    );
}

#[test]
fn constant_runs_remain_fresh_and_contiguous() {
    let mut compiler = Compiler::new_with_mode(
        "test.js",
        "",
        SpecializationMode::Enabled,
        &[],
        FxHashMap::default(),
        String::new(),
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
