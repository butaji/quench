use super::*;

#[test]
fn scalar_constants_reuse_exact_slots() {
    let mut compiler = Compiler::new("test.js");
    assert_eq!(compiler.constant(Constant::Number(1.0)), 0);
    assert_eq!(compiler.constant(Constant::String("x".into())), 1);
    assert_eq!(compiler.constant(Constant::Number(1.0)), 0);
    assert_eq!(compiler.constant(Constant::String("x".into())), 1);
    assert_eq!(compiler.constants.len(), 2);
}

#[test]
fn scalar_constants_preserve_signed_zero_bits() {
    let mut compiler = Compiler::new("test.js");
    assert_ne!(
        compiler.constant(Constant::Number(0.0)),
        compiler.constant(Constant::Number(-0.0))
    );
}

#[test]
fn constant_runs_remain_fresh_and_contiguous() {
    let mut compiler = Compiler::new("test.js");
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
