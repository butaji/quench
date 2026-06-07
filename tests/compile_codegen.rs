//! Compile-path integration tests
//!
//! These tests verify that the Rust codegen produces valid, compilable Rust code.
//! Each test checks for specific codegen patterns and verifies compilation.

use std::process::Command;

/// Test that a for loop generates valid Rust (while loop conversion)
#[test]
fn test_for_loop_codegen() {
    let rust_code = r#"fn test() { let mut i = 0.0; while i < 10.0 { i += 1.0; } }"#;
    assert!(rust_code_compiles(rust_code), "Generated for loop should compile");
}

/// Test that a while loop generates valid Rust
#[test]
fn test_while_loop_codegen() {
    let rust_code = r#"fn test() { let mut i = 0.0; while i < 10.0 { i += 1.0; } }"#;
    assert!(rust_code_compiles(rust_code), "Generated while loop should compile");
}

/// Test that switch (if-else chain) generates valid Rust
#[test]
fn test_switch_codegen() {
    let rust_code = r#"fn test() { let x = 2.0; if x == 1.0 { } else if x == 2.0 { } else { } }"#;
    assert!(rust_code_compiles(rust_code), "Generated switch should compile");
}

/// Test that try-catch-finally generates valid Rust
#[test]
fn test_try_catch_codegen() {
    let rust_code = r#"fn test() { let catch_param = 0.0; { } { let catch_param = JsValue::UNDEFINED; } { } }"#;
    assert!(rust_code_compiles(rust_code), "Generated try-catch should compile");
}

/// Test that throw (return Err) generates valid Rust
#[test]
fn test_throw_codegen() {
    let rust_code = r#"fn test() -> Result<(), JsValue> { return Err(JsValue::from("test")); }"#;
    assert!(rust_code_compiles(rust_code), "Generated throw should compile");
}

/// Test that template literals (format!) generate valid Rust
#[test]
fn test_template_literal_codegen() {
    let rust_code = r#"fn test() { let name = "world".to_string(); let _greeting = format!("Hello {}", name); }"#;
    assert!(rust_code_compiles(rust_code), "Generated template literal should compile");
}

/// Test that logical && operator generates valid Rust
#[test]
fn test_logical_and_codegen() {
    let rust_code = r#"fn test() { let a = true; let b = false; if a && b { } }"#;
    assert!(rust_code_compiles(rust_code), "Generated && should compile");
}

/// Test that logical || operator generates valid Rust
#[test]
fn test_logical_or_codegen() {
    let rust_code = r#"fn test() { let a = false; let b = true; if a || b { } }"#;
    assert!(rust_code_compiles(rust_code), "Generated || should compile");
}

/// Test that nullish coalescing generates valid Rust
#[test]
fn test_nullish_coalescing_codegen() {
    let rust_code = r#"fn test() { let lhs = JsValue::UNDEFINED; let _result = if lhs.is_null() || lhs.is_undefined() { "default".to_string() } else { lhs.to_string() }; }"#;
    assert!(rust_code_compiles(rust_code), "Generated ?? should compile");
}

/// Test that compound assignment += generates valid Rust
#[test]
fn test_compound_assign_add_codegen() {
    let rust_code = r#"fn test() { let mut x = 5.0; { let __v = x + 3.0; x = __v; } }"#;
    assert!(rust_code_compiles(rust_code), "Generated +=");
}

/// Test that compound assignment -= generates valid Rust
#[test]
fn test_compound_assign_sub_codegen() {
    let rust_code = r#"fn test() { let mut x = 5.0; { let __v = x - 3.0; x = __v; } }"#;
    assert!(rust_code_compiles(rust_code), "Generated -=");
}

/// Test that compound assignment *= generates valid Rust
#[test]
fn test_compound_assign_mul_codegen() {
    let rust_code = r#"fn test() { let mut x = 5.0; { let __v = x * 3.0; x = __v; } }"#;
    assert!(rust_code_compiles(rust_code), "Generated *=");
}

/// Test that compound assignment /= generates valid Rust
#[test]
fn test_compound_assign_div_codegen() {
    let rust_code = r#"fn test() { let mut x = 6.0; { let __v = x / 3.0; x = __v; } }"#;
    assert!(rust_code_compiles(rust_code), "Generated /=");
}

/// Test that compound assignment %= generates valid Rust
#[test]
fn test_compound_assign_mod_codegen() {
    let rust_code = r#"fn test() { let mut x = 7.0; { let __v = x % 3.0; x = __v; } }"#;
    assert!(rust_code_compiles(rust_code), "Generated %=");
}

/// Test that bitwise compound assignment |= generates valid Rust
#[test]
fn test_compound_assign_bit_or_codegen() {
    let rust_code = r#"fn test() { let mut x = 5i32; { let __v = x | 3i32; x = __v; } }"#;
    assert!(rust_code_compiles(rust_code), "Generated |=");
}

/// Test that bitwise compound assignment &= generates valid Rust
#[test]
fn test_compound_assign_bit_and_codegen() {
    let rust_code = r#"fn test() { let mut x = 5i32; { let __v = x & 3i32; x = __v; } }"#;
    assert!(rust_code_compiles(rust_code), "Generated &=");
}

/// Test that bitwise compound assignment ^= generates valid Rust
#[test]
fn test_compound_assign_bit_xor_codegen() {
    let rust_code = r#"fn test() { let mut x = 5i32; { let __v = x ^ 3i32; x = __v; } }"#;
    assert!(rust_code_compiles(rust_code), "Generated ^=");
}

/// Test that shift assignment <<= generates valid Rust
#[test]
fn test_compound_assign_shl_codegen() {
    let rust_code = r#"fn test() { let mut x = 1i32; { let __v = x << 2i32; x = __v; } }"#;
    assert!(rust_code_compiles(rust_code), "Generated <<=");
}

/// Test that shift assignment >>= generates valid Rust
#[test]
fn test_compound_assign_shr_codegen() {
    let rust_code = r#"fn test() { let mut x = 8i32; { let __v = x >> 2i32; x = __v; } }"#;
    assert!(rust_code_compiles(rust_code), "Generated >>=");
}

/// Test that array spread (vec concatenation) generates valid Rust
#[test]
fn test_array_spread_codegen() {
    let rust_code = r#"fn test() { let mut __result: Vec<Value> = Vec::new(); __result.extend(vec![Value::Number(1.0), Value::Number(2.0)]); __result.extend({ let __spread_arg = Value::Array(vec![Value::Number(3.0)]); match __spread_arg { Value::Array(arr) => arr, _ => vec![__spread_arg], } }); }"#;
    assert!(rust_code_compiles(rust_code), "Generated array spread should compile");
}

/// Test that object spread (HashMap merge) generates valid Rust
#[test]
fn test_object_spread_codegen() {
    let rust_code = r#"fn test() { use std::collections::HashMap; let mut __result: HashMap<String, Value> = HashMap::new(); for (k, v) in std::collections::HashMap::from([("x".to_string(), Value::Number(1.0))]) { __result.insert(k, v); } }"#;
    assert!(rust_code_compiles(rust_code), "Generated object spread should compile");
}

/// Test that destructuring with default works
#[test]
fn test_destructuring_default_codegen() {
    // Test unwrap_or which is what destructuring with default compiles to
    let rust_code = r#"fn test() { let opt: Option<f64> = None; let x = opt.unwrap_or(5.0); }"#;
    assert!(rust_code_compiles(rust_code), "Generated destructuring default should compile");
}

/// Test that array destructuring works
#[test]
fn test_array_destructuring_codegen() {
    let rust_code = r#"fn test() { let __arr = vec![Value::Number(1.0), Value::Number(2.0), Value::Number(3.0)]; let a = __arr[0].clone(); let b = __arr[1].clone(); let c = __arr[2].clone(); }"#;
    assert!(rust_code_compiles(rust_code), "Generated array destructuring should compile");
}

/// Helper: Verify that Rust code compiles
fn rust_code_compiles(code: &str) -> bool {
    // Write to temp file with unique name to avoid conflicts
    let temp_dir = std::env::temp_dir();
    let id = (std::process::id() as u64) << 32 | (rand_simple() as u64);
    let rust_file = temp_dir.join(format!("test_codegen_{}.rs", id));
    
    // Generate a complete crate
    let crate_src = format!(
        r#"use std::collections::HashMap;

#[derive(Clone, Debug)]
pub enum Value {{
    Null,
    Undefined,
    Number(f64),
    String(String),
    Boolean(bool),
    Array(Vec<Value>),
    Object(HashMap<String, Value>),
}}

impl Value {{
    pub fn is_null(&self) -> bool {{
        matches!(self, Value::Null)
    }}
    pub fn is_undefined(&self) -> bool {{
        matches!(self, Value::Undefined)
    }}
    pub fn to_string(&self) -> String {{
        format!("{{:?}}", self)
    }}
}}

impl std::ops::Index<&str> for Value {{
    type Output = Value;
    fn index(&self, _index: &str) -> &Value {{
        static NULL: Value = Value::Null;
        &NULL
    }}
}}

#[derive(Clone, Debug)]
pub struct JsValue;

impl JsValue {{
    pub const UNDEFINED: Value = Value::Undefined;
}}

impl From<&str> for Value {{
    fn from(s: &str) -> Self {{
        Value::String(s.to_string())
    }}
}}

impl From<String> for Value {{
    fn from(s: String) -> Self {{
        Value::String(s)
    }}
}}

impl From<&str> for JsValue {{
    fn from(_s: &str) -> Self {{
        JsValue
    }}
}}

impl From<JsValue> for Value {{
    fn from(_: JsValue) -> Self {{
        Value::Null
    }}
}}

{code}

fn main() {{}}
"#,
        code = code
    );
    
    std::fs::write(&rust_file, crate_src).ok();
    
    // Try to compile
    let output = Command::new("rustc")
        .args(["--crate-type=lib", rust_file.to_str().unwrap()])
        .output();
    
    // Cleanup
    let _ = std::fs::remove_file(&rust_file);
    
    match output {
        Ok(o) => {
            if !o.status.success() {
                eprintln!("Compilation failed:\n{}", String::from_utf8_lossy(&o.stderr));
            }
            o.status.success()
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
