use super::*;

#[test]
fn scalar_constants_reuse_exact_slots() {
    let mut compiler = Compiler::new_with_mode("test.js", SpecializationMode::Enabled);
    assert_eq!(compiler.constant(Constant::Number(1.0)), 0);
    assert_eq!(compiler.constant(Constant::String("x".into())), 1);
    assert_eq!(compiler.constant(Constant::Number(1.0)), 0);
    assert_eq!(compiler.constant(Constant::String("x".into())), 1);
    assert_eq!(compiler.constants.len(), 2);
}

#[test]
fn scalar_constants_preserve_signed_zero_bits() {
    let mut compiler = Compiler::new_with_mode("test.js", SpecializationMode::Enabled);
    assert_ne!(
        compiler.constant(Constant::Number(0.0)),
        compiler.constant(Constant::Number(-0.0))
    );
}

#[test]
fn constant_runs_remain_fresh_and_contiguous() {
    let mut compiler = Compiler::new_with_mode("test.js", SpecializationMode::Enabled);
    assert_eq!(compiler.constant(Constant::Number(1.0)), 0);
    let start = compiler.constant_run(vec![Constant::Number(1.0), Constant::Number(1.0)]);
    assert_eq!(start, 1);
    assert_eq!(compiler.constants.len(), 3);
    assert_eq!(compiler.constant(Constant::Number(1.0)), 0);
}

#[test]
fn packed_domain_overflow_is_a_diagnostic() {
    let source = format!("[{}];", "0,".repeat(4097));
    let errors = Engine::specialize(&source, "packed-overflow.js").unwrap_err();
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("packed instruction domain"))
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
