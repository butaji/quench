//! Compile-path negative tests
//!
//! These tests verify that the Rust codegen produces helpful error messages
//! for unsupported JavaScript features. Features that cannot be represented
//! in Rust should fail at codegen time with clear error messages.

use std::process::Command;

/// Test that `with` statement produces an error (not valid Rust)
#[test]
fn test_with_statement_fails() {
    let rust_code = r#"fn test() { with ({}) { } }"#;
    assert!(rust_code_fails_with_error(rust_code), "`with` should fail");
}

/// Test that bare `eval` produces an error
#[test]
fn test_eval_fails() {
    let rust_code = r#"fn test() { eval("x + 1"); }"#;
    assert!(rust_code_fails_with_error(rust_code), "`eval` should fail");
}

/// Test that indirect eval (bracket access) produces an error
#[test]
fn test_indirect_eval_fails() {
    let rust_code = r#"fn test() { (0, eval)("x + 1"); }"#;
    assert!(rust_code_fails_with_error(rust_code), "indirect eval should fail");
}

/// Test that `debugger` statement produces an error
#[test]
fn test_debugger_fails() {
    let rust_code = r#"fn test() { debugger; }"#;
    assert!(rust_code_fails_with_error(rust_code), "`debugger` should fail");
}

/// Test that `delete` on non-identifier produces an error
#[test]
fn test_delete_non_identifier_fails() {
    // delete on computed property is not valid Rust struct field manipulation
    let rust_code = r#"fn test() { let obj = 5; let _ = delete obj.prop; }"#;
    assert!(rust_code_fails_with_error(rust_code), "`delete` should fail for non-identifier");
}

/// Test that `void` expression produces an error (no equivalent in Rust)
#[test]
fn test_void_fails() {
    let rust_code = r#"fn test() { let _ = void some_func(); }"#;
    assert!(rust_code_fails_with_error(rust_code), "`void` should fail");
}

/// Test that labeled `break` in wrong context fails
#[test]
fn test_labeled_break_wrong_context_fails() {
    let rust_code = r#"
fn test() {
    'outer: loop {
        break 'inner;
    }
}
"#;
    assert!(rust_code_fails_with_error(rust_code), "mismatched label should fail");
}

/// Test that labeled `continue` in wrong context fails
#[test]
fn test_labeled_continue_wrong_context_fails() {
    let rust_code = r#"
fn test() {
    'outer: loop {
        continue 'inner;
    }
}
"#;
    assert!(rust_code_fails_with_error(rust_code), "mismatched label should fail");
}

/// Test that `yield` outside generator fails
#[test]
fn test_yield_outside_generator_fails() {
    let rust_code = r#"fn test() { let _ = yield 5; }"#;
    assert!(rust_code_fails_with_error(rust_code), "`yield` outside generator should fail");
}

/// Test that await outside async function fails
#[test]
fn test_await_outside_async_fails() {
    let rust_code = r#"fn test() { let _ = await some_future(); }"#;
    assert!(rust_code_fails_with_error(rust_code), "`await` outside async should fail");
}

/// Test that `super` outside class method fails
#[test]
fn test_super_outside_class_fails() {
    let rust_code = r#"fn test() { let _ = super.field; }"#;
    assert!(rust_code_fails_with_error(rust_code), "`super` outside class should fail");
}

/// Test that `new.target` outside function fails
#[test]
fn test_new_target_outside_function_fails() {
    let rust_code = r#"fn test() { let _ = new.target; }"#;
    assert!(rust_code_fails_with_error(rust_code), "`new.target` outside function should fail");
}

/// Helper: Verify that Rust code fails to compile with an error message
fn rust_code_fails_with_error(code: &str) -> bool {
    let temp_dir = std::env::temp_dir();
    let id = (std::process::id() as u64) << 32 | (rand_simple() as u64);
    let rust_file = temp_dir.join(format!("test_codegen_neg_{}.rs", id));

    let crate_src = format!(
        r#"
#[derive(Clone, Debug)]
pub enum Value {{
    Null,
    Undefined,
    Number(f64),
    String(String),
    Boolean(bool),
}}

{code}

fn main() {{}}
"#,
        code = code
    );

    std::fs::write(&rust_file, crate_src).ok();

    let output = Command::new("rustc")
        .args(["--crate-type=lib", rust_file.to_str().unwrap()])
        .output();

    let _ = std::fs::remove_file(&rust_file);

    match output {
        Ok(o) => {
            // Should fail to compile
            !o.status.success()
        }
        Err(e) => {
            eprintln!("Failed to run rustc: {}", e);
            false
        }
    }
}

/// Simple random number generator for unique file names
fn rand_simple() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u32
}
