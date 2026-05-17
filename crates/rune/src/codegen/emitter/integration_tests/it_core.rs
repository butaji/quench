//! # Core Type System Integration Tests

use crate::analyzer;
use crate::codegen;
use crate::parser;

#[test]
fn test_transpile_struct_type() {
    let source = "
export type Point = {
    x: number,
    y: number,
};
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
}

#[test]
fn test_transpile_tagged_union() {
    let source = "
export type Message =
    | { tag: \"Move\"; x: number; y: number }
    | { tag: \"Stop\" };
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
}

#[test]
fn test_transpile_result_pattern() {
    let source = "
export type Result<T, E> = { ok: true; value: T } | { ok: false; error: E };
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
}

#[test]
fn test_option_type() {
    let source = "
export function findItem(arr: string[], target: string): string | null {
    for (let i = 0; i < arr.length; i++) {
        if (arr[i] === target) return arr[i];
    }
    return null;
}
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
}

#[test]
fn test_enum_like_union() {
    let source = "
export type Status = \"pending\" | \"active\" | \"completed\";
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
}

#[test]
fn test_generic_function() {
    let source = "
export function identity<T>(value: T): T {
    return value;
}
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
}

#[test]
fn test_generic_type_alias() {
    let source = "export type Pair<A, B> = { first: A, second: B };";
    let file = parser::parse_file_from_str(source, "pair.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
}

#[test]
fn test_nested_types() {
    let source = "
export type Nested = {
    outer: {
        middle: {
            inner: number,
        },
    },
};
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
}
