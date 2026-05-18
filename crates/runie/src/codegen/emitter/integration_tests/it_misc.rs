//! # Miscellaneous Integration Tests

use crate::analyzer;
use crate::codegen;
use crate::parser;

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

#[test]
fn test_location_from_offset() {
    let source = "line1\nline2\nline3";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let (line, _col) = file.location_from_offset(0);
    assert_eq!(line, 1);
    let (line2, _) = file.location_from_offset(6);
    assert_eq!(line2, 2);
}

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
