//! # SubsetValidator Tests
//!
//! Tests for the zero-overhead subset validation.

use crate::analyzer::SubsetValidator;
use crate::parser::{SourceFile, SourceKind};

fn create_test_file(source: &str) -> SourceFile {
    SourceFile {
        path: std::path::PathBuf::from("test.r.ts"),
        kind: SourceKind::TypeScript,
        source: source.to_string(),
        name: "test".to_string(),
        valid: true,
        errors: Vec::new(),
    }
}

#[test]
fn test_subset_validator_any_type() {
    let mut validator = SubsetValidator::new();
    let file = create_test_file("let x: any = 5;");
    let result = validator.validate(&file);
    assert!(result.is_err());
}

#[test]
fn test_subset_validator_class() {
    let mut validator = SubsetValidator::new();
    let file = create_test_file("class Foo {}");
    let result = validator.validate(&file);
    assert!(result.is_err());
}

#[test]
fn test_subset_validator_var() {
    let mut validator = SubsetValidator::new();
    let file = create_test_file("var x = 5;");
    let result = validator.validate(&file);
    assert!(result.is_err());
}

#[test]
fn test_subset_validator_loose_equality() {
    let mut validator = SubsetValidator::new();
    let file = create_test_file("if (x == y) {}");
    let result = validator.validate(&file);
    assert!(result.is_err());
}

#[test]
fn test_subset_validator_try_catch() {
    let mut validator = SubsetValidator::new();
    let file = create_test_file("try { } catch (e) { }");
    let result = validator.validate(&file);
    assert!(result.is_err());
}

#[test]
fn test_subset_validator_valid_code() {
    let mut validator = SubsetValidator::new();
    let file = create_test_file(
        "export function add(a: number, b: number): number { return a + b; } \
         const x: number = 5; let y: number = 10; if (x === y) {}",
    );
    let result = validator.validate(&file);
    assert!(result.is_ok());
}

#[test]
fn test_subset_validator_comments_ignored() {
    let mut validator = SubsetValidator::new();
    let file = create_test_file("// var x = 5; // class Foo {} // try {} catch {}");
    let result = validator.validate(&file);
    assert!(result.is_ok());
}

// Additional tests for new validation features

#[test]
fn test_subset_validator_delete() {
    let mut validator = SubsetValidator::new();
    let file = create_test_file("delete obj.key;");
    let result = validator.validate(&file);
    assert!(result.is_err());
}

#[test]
fn test_subset_validator_for_in() {
    let mut validator = SubsetValidator::new();
    let file = create_test_file("for (const key in obj) {}");
    let result = validator.validate(&file);
    assert!(result.is_err());
}

#[test]
fn test_subset_validator_typeof() {
    let mut validator = SubsetValidator::new();
    let file = create_test_file("const t = typeof x;");
    let result = validator.validate(&file);
    assert!(result.is_err());
}

#[test]
fn test_subset_validator_instanceof() {
    let mut validator = SubsetValidator::new();
    let file = create_test_file("if (x instanceof String) {}");
    let result = validator.validate(&file);
    assert!(result.is_err());
}

#[test]
fn test_subset_validator_arguments() {
    let mut validator = SubsetValidator::new();
    let file = create_test_file("console.log(arguments);");
    let result = validator.validate(&file);
    assert!(result.is_err());
}

#[test]
fn test_subset_validator_implicit_coercion() {
    let mut validator = SubsetValidator::new();
    // Bare identifier is not flagged (could be boolean variable)
    let file = create_test_file("if (isActive) {}");
    let result = validator.validate(&file);
    assert!(result.is_ok());

    // Null/undefined in condition is flagged
    let file = create_test_file("if (null) {}");
    let result = validator.validate(&file);
    assert!(result.is_err());
}

#[test]
fn test_subset_validator_implicit_coercion_empty_string() {
    let mut validator = SubsetValidator::new();
    let file = create_test_file("if ('') {}");
    let result = validator.validate(&file);
    assert!(result.is_err());
}

#[test]
fn test_subset_validator_implicit_coercion_numeric() {
    let mut validator = SubsetValidator::new();
    let file = create_test_file("if (0) {}");
    let result = validator.validate(&file);
    assert!(result.is_err());
}

#[test]
fn test_subset_validator_valid_explicit_check() {
    let mut validator = SubsetValidator::new();
    let file = create_test_file("if (str !== '') {}");
    let result = validator.validate(&file);
    assert!(result.is_ok());
}

#[test]
fn test_subset_validator_valid_map_access() {
    let mut validator = SubsetValidator::new();
    let file = create_test_file("const val = map.get(key);");
    let result = validator.validate(&file);
    assert!(result.is_ok());
}
