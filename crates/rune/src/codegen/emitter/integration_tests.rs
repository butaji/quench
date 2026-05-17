//! # Integration Tests
//!
//! End-to-end tests for the Rune compiler.
//!
//! These tests validate the core transpilation capabilities.

use crate::analyzer;
use crate::codegen;
use crate::parser;

// ============================================================================
// Core Type System Tests
// ============================================================================

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

// ============================================================================
// Function Tests
// ============================================================================

#[test]
fn test_function_with_params() {
    let source = "
export function add(a: number, b: number): number {
    return a + b;
}
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(result.source.contains("fn"));
}

#[test]
fn test_arrow_function() {
    let source = "
export const multiply = (a: number, b: number): number => a * b;
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
}

#[test]
fn test_async_function() {
    let source = "
export async function fetchData(url: string): Promise<string> {
    return \"data\";
}
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(result.source.contains("async"));
}

// ============================================================================
// Control Flow Tests
// ============================================================================

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

// ============================================================================
// Expression Tests
// ============================================================================

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

// ============================================================================
// Object and Array Tests
// ============================================================================

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

// ============================================================================
// Type System Tests
// ============================================================================

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

// ============================================================================
// Module System Tests
// ============================================================================

#[test]
fn test_import_statement() {
    let source = "
import { Task, createTask } from \"./state.r.ts\";
export function process(task: Task): Task {
    return createTask(task.title);
}
";
    let file = parser::parse_file_from_str(source, "main.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
}

#[test]
fn test_native_import() {
    let source = "
import { fastSqrt } from \"native:math\";
export function sqrtAll(values: number[]): number[] {
    return values.map(v => fastSqrt(v));
}
";
    let file = parser::parse_file_from_str(source, "main.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(result.source.contains("native"));
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_empty_source() {
    let source = "";
    let file = parser::parse_file_from_str(source, "empty.r.ts").unwrap();
    let result = analyzer::analyze(&file).unwrap();
    assert!(result.warnings.is_empty());
}

#[test]
fn test_comment_only_source() {
    let source = "
// This is a comment
// Another comment
";
    let file = parser::parse_file_from_str(source, "comment.r.ts").unwrap();
    let result = analyzer::analyze(&file).unwrap();
    assert!(result.warnings.is_empty());
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

// ============================================================================
// File Type Recognition Tests
// ============================================================================

#[test]
fn test_tsx_file_kind() {
    let source = "export type Widget = { id: string };";
    let file = parser::parse_file_from_str(source, "widget.r.tsx").unwrap();
    assert!(file.is_tsx());
}

#[test]
fn test_typescript_file_kind() {
    let source = "export type Item = { name: string };";
    let file = parser::parse_file_from_str(source, "item.r.ts").unwrap();
    assert!(!file.is_tsx());
}

// ============================================================================
// Location Tracking Tests
// ============================================================================

#[test]
fn test_location_from_offset() {
    let source = "line1\nline2\nline3";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let (line, _col) = file.location_from_offset(0);
    assert_eq!(line, 1);
    let (line2, _) = file.location_from_offset(6);
    assert_eq!(line2, 2);
}

// ============================================================================
// Output Format Tests
// ============================================================================

#[test]
fn test_generate_module_has_source() {
    let source = "export type Point = { x: number; y: number };";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
    assert!(result.source.len() > 10);
}

#[test]
fn test_generate_module_has_name() {
    let source = "export type Point = { x: number; y: number };";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.name.is_empty());
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

#[test]
fn test_generic_type_alias() {
    let source = "export type Pair<A, B> = { first: A, second: B };";
    let file = parser::parse_file_from_str(source, "pair.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
}
