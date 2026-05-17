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
         const x: number = 5; let y: number = 10; if (x === y) {}"
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
