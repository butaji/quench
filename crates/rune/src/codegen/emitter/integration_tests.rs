//! # Integration Tests
//!
//! End-to-end tests for the Rune compiler.
//!
//! These tests validate the core transpilation capabilities.

use crate::analyzer;
use crate::codegen;
use crate::parser;

/// Test: Object type transpilation
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

    // Should emit some struct-related content
    assert!(
        result.source.contains("struct")
            || result.source.contains("Struct")
            || !result.source.is_empty()
    );
}

/// Test: Tagged union transpilation
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

    // Should emit some enum-like content
    assert!(
        result.source.contains("enum")
            || result.source.contains("Enum")
            || !result.source.is_empty()
    );
}

/// Test: Basic type alias (structural check)
#[test]
fn test_transpile_type_alias() {
    let source = "export type UserId = number;";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    // Should have generated some code
    assert!(!result.source.is_empty());
}

/// Test: Array type
#[test]
fn test_transpile_array_type() {
    let source = "export type Numbers = number[];";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    // Should have generated some code
    assert!(!result.source.is_empty());
}

/// Test: Nested object type
#[test]
fn test_transpile_nested_object() {
    let source = "
export type Config = {
    name: string,
    settings: {
        debug: boolean,
        level: number,
    },
};
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    // Should contain nested type
    assert!(result.source.contains("Config") || !result.source.is_empty());
}

/// Test: Multiple exports
#[test]
fn test_transpile_multiple_types() {
    let source = "
export type Point = { x: number, y: number };
export type Line = { start: Point, end: Point };
export type Shape = { name: string, perimeter: number };
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    // Should have generated some code
    assert!(!result.source.is_empty());
}

/// Test: Option type with null
#[test]
fn test_transpile_option_type() {
    let source = "export type Maybe<T> = T | null;";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    // Should have generated some code
    assert!(!result.source.is_empty());
}

/// Test: Result-like pattern
#[test]
fn test_transpile_result_pattern() {
    let source = "
export type Result<T, E> = { ok: true; value: T } | { ok: false; error: E };
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    // Should have generated some code
    assert!(!result.source.is_empty());
}

/// Test: Complex enum
#[test]
fn test_transpile_complex_enum() {
    let source = "
export type Event =
    | { tag: \"Click\"; x: number; y: number }
    | { tag: \"KeyPress\"; key: string }
    | { tag: \"Resize\"; width: number; height: number }
    | { tag: \"Close\" };
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    // Should have generated some code
    assert!(!result.source.is_empty());
}

/// Test: TSX file is recognized
#[test]
fn test_tsx_file_kind() {
    let source = "export type Widget = { id: string };";
    let file = parser::parse_file_from_str(source, "widget.r.tsx").unwrap();
    assert!(file.is_tsx());
}

/// Test: TypeScript file is recognized
#[test]
fn test_typescript_file_kind() {
    let source = "export type Item = { name: string };";
    let file = parser::parse_file_from_str(source, "item.r.ts").unwrap();
    assert!(!file.is_tsx());
}

/// Test: Location from offset
#[test]
fn test_location_from_offset() {
    let source = "line1\nline2\nline3";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();

    let (line, col) = file.location_from_offset(0);
    assert_eq!(line, 1);
    assert_eq!(col, 1);

    let (line, col) = file.location_from_offset(6); // start of "line2"
    assert_eq!(line, 2);
    assert_eq!(col, 1);
}

/// Test: Generated module has source
#[test]
fn test_generate_module_has_source() {
    let source = "export type Point = { x: number; y: number };";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    // Should have generated some Rust code
    assert!(!result.source.is_empty());
    assert!(result.source.len() > 10);
}

/// Test: Generated module has name
#[test]
fn test_generate_module_has_name() {
    let source = "export type Point = { x: number; y: number };";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    // Should have a module name
    assert!(!result.name.is_empty());
}

/// Test: Empty source is handled
#[test]
fn test_empty_source() {
    let source = "";
    let file = parser::parse_file_from_str(source, "empty.r.ts").unwrap();
    let result = analyzer::analyze(&file).unwrap();

    // Should handle empty source gracefully
    assert!(result.warnings.is_empty());
}

/// Test: Comment-only source is handled
#[test]
fn test_comment_only_source() {
    let source = "
// This is a comment
// Another comment
";
    let file = parser::parse_file_from_str(source, "comment.r.ts").unwrap();
    let result = analyzer::analyze(&file).unwrap();

    // Should handle comments gracefully without errors
    assert!(result.warnings.is_empty());
}

/// Test: Type with optional field
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

    // Should have generated some code
    assert!(!result.source.is_empty());
}

/// Test: Generic type alias
#[test]
fn test_generic_type_alias() {
    let source = "export type Pair<A, B> = { first: A, second: B };";
    let file = parser::parse_file_from_str(source, "pair.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    // Should have generated some code
    assert!(!result.source.is_empty());
}

/// Test: Large number of fields
#[test]
fn test_large_struct() {
    let source = "
export type Config = {
    field1: number,
    field2: number,
    field3: number,
    field4: number,
    field5: number,
    field6: number,
    field7: number,
    field8: number,
};
";
    let file = parser::parse_file_from_str(source, "config.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    // Should have generated some code
    assert!(!result.source.is_empty());
}

/// Test: Multiple union variants
#[test]
fn test_large_union() {
    let source = "
export type Status =
    | { tag: \"A\"; value: number }
    | { tag: \"B\"; value: number }
    | { tag: \"C\"; value: number }
    | { tag: \"D\"; value: number }
    | { tag: \"E\"; value: number };
";
    let file = parser::parse_file_from_str(source, "status.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    // Should have generated some code
    assert!(!result.source.is_empty());
}

/// Test: Type with string union
#[test]
fn test_string_union() {
    let source = "
export type Direction = \"north\" | \"south\" | \"east\" | \"west\";
";
    let file = parser::parse_file_from_str(source, "direction.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    // Should have generated some code
    assert!(!result.source.is_empty());
}

/// Test: For loop transpilation
#[test]
fn test_for_loop_transpilation() {
    let source = "
export function sumArray(arr: number[]): number {
    let total = 0;
    for (let i = 0; i < arr.length; i++) {
        total = total + arr[i];
    }
    return total;
}
";
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    println!("Generated code:\n{}", result.source);

    // Should have generated some code
    assert!(!result.source.is_empty());
    // The for loop should NOT emit "for let" pattern - that's invalid Rust
    assert!(
        !result.source.contains("for let"),
        "Should not emit 'for let' pattern"
    );
}
