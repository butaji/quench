//! # Expression Integration Tests

use crate::analyzer;
use crate::codegen;
use crate::parser;

#[test]
fn test_binary_operators() {
    let source = "
export function ops(a: number, b: number): number {
    return a + b - 1 * 2 / 3;
}
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(result.source.contains('+') || result.source.contains('-'));
}

#[test]
fn test_comparison_operators() {
    let source = "
export function compare(a: number, b: number): boolean {
    return a === b && a !== b;
}
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
}

#[test]
fn test_ternary_expression() {
    let source = "
export function abs(n: number): number {
    return n >= 0 ? n : -n;
}
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
}

#[test]
fn test_array_literal() {
    let source = "
export const numbers: number[] = [1, 2, 3, 4, 5];
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
}

#[test]
fn test_object_literal() {
    let source = "
export function createPoint(x: number, y: number): { x: number; y: number } {
    return { x, y };
}
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
}

#[test]
fn test_spread_operator() {
    let source = "
export function merge(a: { x: number }, b: { y: number }): { x: number; y: number } {
    return { ...a, ...b };
}
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
}

#[test]
fn test_optional_field() {
    let source = "
export type User = {
    id: number,
    name: string,
    email?: string,
};
";
    let file = parser::parse_file_from_str(source, "user.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
}
