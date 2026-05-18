//! # Control Flow Integration Tests

use crate::analyzer;
use crate::codegen;
use crate::parser;

#[test]
fn test_if_else() {
    let source = "
export function max(a: number, b: number): number {
    if (a > b) {
        return a;
    } else {
        return b;
    }
}
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(result.source.contains("if"));
}

#[test]
fn test_while_loop() {
    let source = "
export function countDown(n: number): void {
    while (n > 0) {
        n = n - 1;
    }
}
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(result.source.contains("while"));
}

#[test]
fn test_for_loop() {
    let source = "
export function sumTo(n: number): number {
    let sum = 0;
    for (let i = 0; i < n; i++) {
        sum = sum + i;
    }
    return sum;
}
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(result.source.contains("for"));
    assert!(!result.source.contains("for let"));
}

#[test]
fn test_for_of_loop() {
    let source = "
export function sumArray(arr: number[]): number {
    let sum = 0;
    for (const item of arr) {
        sum = sum + item;
    }
    return sum;
}
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(result.source.contains("for"));
}

#[test]
fn test_large_union() {
    let source = "
export type Color =
    | { tag: \"Red\" }
    | { tag: \"Green\" }
    | { tag: \"Blue\" }
    | { tag: \"Yellow\" }
    | { tag: \"Orange\" }
    | { tag: \"Purple\" };
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
}
