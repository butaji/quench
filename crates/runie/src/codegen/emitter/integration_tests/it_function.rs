//! # Function Integration Tests

use crate::analyzer;
use crate::codegen;
use crate::parser;

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
