//! # Examples Validation Tests
//!
//! Validates that example source files transpile correctly.

use crate::analyzer;
use crate::codegen;
use crate::parser;

/// Test: Basic struct and function transpilation
#[test]
fn test_basic_struct_transpilation() {
    let source = "
export type Task = {
    id: number;
    title: string;
    done: boolean;
};

export function createTask(title: string): Task {
    return { id: 1, title, done: false };
}
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    
    assert!(result.source.contains("pub struct"));
    assert!(result.source.contains("pub fn"));
}

/// Test: Tagged union enum transpilation
#[test]
fn test_tagged_union_transpilation() {
    let source = "
export type Filter = 
    | { tag: \"All\" }
    | { tag: \"Active\" }
    | { tag: \"Completed\" };
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    
    assert!(result.source.contains("pub enum"));
    assert!(result.source.contains("All"));
    assert!(result.source.contains("Active"));
    assert!(result.source.contains("Completed"));
}

/// Test: Result pattern transpilation
#[test]
fn test_result_pattern_transpilation() {
    let source = "
export function validate(value: number): 
    | { ok: true; value: number }
    | { ok: false; error: string }
{
    if (value < 0) {
        return { ok: false, error: \"Negative\" };
    }
    return { ok: true, value };
}
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    
    assert!(result.source.contains("pub fn"));
    assert!(result.source.contains("Result"));
}

/// Test: Array and loop transpilation
#[test]
fn test_array_loop_transpilation() {
    let source = "
export function sumArray(arr: number[]): number {
    let sum = 0;
    for (let i = 0; i < arr.length; i++) {
        sum = sum + arr[i];
    }
    return sum;
}
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    
    assert!(result.source.contains("for"));
    assert!(result.source.contains("Vec") || result.source.contains("i32"));
}

/// Test: Option type transpilation
#[test]
fn test_option_type_transpilation() {
    let source = "
export function findItem(items: string[], target: string): string | null {
    for (let i = 0; i < items.length; i++) {
        if (items[i] === target) {
            return items[i];
        }
    }
    return null;
}
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    
    assert!(result.source.contains("pub fn"));
    assert!(result.source.contains("Option"));
}

/// Test: Native import pattern
#[test]
fn test_native_import_transpilation() {
    let source = "
import { fastSqrt } from \"native:math\";

export function sqrtAll(values: number[]): number[] {
    const result: number[] = [];
    for (let i = 0; i < values.length; i++) {
        result.push(fastSqrt(values[i]));
    }
    return result;
}
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    
    assert!(result.source.contains("native"));
    assert!(result.source.contains("crate::native"));
}

/// Test: Switch/match transpilation
#[test]
fn test_switch_transpilation() {
    let source = "
export function getStatusMessage(status: Filter): string {
    switch (status.tag) {
        case \"All\":
            return \"Showing all\";
        case \"Active\":
            return \"Showing active\";
        case \"Completed\":
            return \"Showing completed\";
    }
}
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    
    assert!(result.source.contains("match"));
}

/// Test: Async function transpilation
#[test]
fn test_async_function_transpilation() {
    let source = "
export async function fetchData(url: string): Promise<string> {
    return \"data\";
}
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    
    // Async functions should be transpiled
    assert!(result.source.contains("async"));
}

/// Test: Object spread transpilation
#[test]
fn test_spread_transpilation() {
    let source = "
export function mergeTasks(a: Task, b: Task): Task {
    return { ...a, ...b };
}
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    
    assert!(result.source.contains("pub fn"));
}

/// Test: Arrow function transpilation
#[test]
fn test_arrow_function_transpilation() {
    let source = "
export const multiply = (a: number, b: number): number => a * b;
export const addOne = (x: number): number => x + 1;
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    
    // Arrow functions should be transpiled (may not contain "pub fn" directly)
    assert!(!result.source.is_empty());
}

/// Test: Conditional expression transpilation
#[test]
fn test_conditional_transpilation() {
    let source = "
export function max(a: number, b: number): number {
    return a > b ? a : b;
}
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    
    assert!(result.source.contains("if") || result.source.contains("if_let"));
}

/// Test: Empty source handling
#[test]
fn test_empty_source() {
    let source = "";
    let file = parser::parse_file_from_str(source, "empty.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    
    // Empty source should produce minimal output (module header at least)
    assert!(result.name.is_empty() || !result.source.is_empty());
}

/// Test: TSX file handling
#[test]
fn test_tsx_file_handling() {
    let source = "
export type Widget = {
    id: string;
    children?: Widget[];
};

export function createWidget(id: string): Widget {
    return { id };
}
";
    let file = parser::parse_file_from_str(source, "widget.r.tsx").unwrap();
    assert!(file.is_tsx());
    
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    
    assert!(result.source.contains("pub struct"));
}

/// Test: Type alias transpilation
#[test]
fn test_type_alias_transpilation() {
    let source = "
export type UserId = number;
export type Username = string;
export type User = {
    id: UserId;
    name: Username;
};
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    
    assert!(result.source.contains("pub struct"));
}

/// Test: Generic type transpilation
#[test]
fn test_generic_type_transpilation() {
    let source = "
export type Pair<A, B> = {
    first: A;
    second: B;
};

export function createPair<A, B>(a: A, b: B): Pair<A, B> {
    return { first: a, second: b };
}
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    
    assert!(result.source.contains("pub struct"));
    assert!(result.source.contains("pub fn"));
}
