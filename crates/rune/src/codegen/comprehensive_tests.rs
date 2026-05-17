//! # Comprehensive Tests
//!
//! Comprehensive tests for the Rune compiler.

#[cfg(test)]
mod parser_tests {
    use crate::parser::{parse_file_from_str, SourceKind};

    #[test]
    fn test_parse_number_literal() {
        let source = "const x = 42;";
        let file = parse_file_from_str(source, "test.r.ts").unwrap();
        assert!(file.valid);
    }

    #[test]
    fn test_parse_string_literal() {
        let source = "const name = \"hello\";";
        let file = parse_file_from_str(source, "test.r.ts").unwrap();
        assert!(file.valid);
    }

    #[test]
    fn test_parse_boolean() {
        let source = "const flag = true;";
        let file = parse_file_from_str(source, "test.r.ts").unwrap();
        assert!(file.valid);
    }

    #[test]
    fn test_parse_function() {
        let source = "export function add(a: number, b: number): number { return a + b; }";
        let file = parse_file_from_str(source, "test.r.ts").unwrap();
        assert!(file.valid);
    }

    #[test]
    fn test_parse_arrow_function() {
        let source = "const add = (a: number, b: number): number => a + b;";
        let file = parse_file_from_str(source, "test.r.ts").unwrap();
        assert!(file.valid);
    }

    #[test]
    fn test_parse_struct() {
        let source = "export type Point = { x: number, y: number };";
        let file = parse_file_from_str(source, "test.r.ts").unwrap();
        assert!(file.valid);
    }

    #[test]
    fn test_parse_tagged_union() {
        let source = "export type Message = { tag: \"Move\", x: number } | { tag: \"Stop\" };";
        let file = parse_file_from_str(source, "test.r.ts").unwrap();
        assert!(file.valid);
    }

    #[test]
    fn test_parse_option_type() {
        let source = "export type Maybe<T> = T | null;";
        let file = parse_file_from_str(source, "test.r.ts").unwrap();
        assert!(file.valid);
    }

    #[test]
    fn test_parse_array() {
        let source = "export const nums: number[] = [1, 2, 3];";
        let file = parse_file_from_str(source, "test.r.ts").unwrap();
        assert!(file.valid);
    }

    #[test]
    fn test_parse_tsx() {
        let source = "export function render(): Widget { return null; }";
        let file = parse_file_from_str(source, "widget.r.tsx").unwrap();
        assert_eq!(file.kind, SourceKind::Tsx);
    }
}

#[cfg(test)]
mod analyzer_tests {
    use crate::analyzer::analyze;
    use crate::parser::parse_file_from_str;

    #[test]
    fn test_analyze_simple_function() {
        let source = "export function add(a: number, b: number): number { return a + b; }";
        let file = parse_file_from_str(source, "test.r.ts").unwrap();
        let result = analyze(&file).unwrap();
        // Analysis completes successfully
        let _ = result.exports;
    }

    #[test]
    fn test_analyze_empty_source() {
        let source = "";
        let file = parse_file_from_str(source, "test.r.ts").unwrap();
        let result = analyze(&file).unwrap();
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn test_analyze_comments() {
        let source = "// This is a comment";
        let file = parse_file_from_str(source, "test.r.ts").unwrap();
        let result = analyze(&file).unwrap();
        assert!(result.warnings.is_empty());
    }
}

#[cfg(test)]
mod codegen_tests {
    use crate::analyzer::analyze;
    use crate::codegen::generate;
    use crate::parser::parse_file_from_str;

    #[test]
    fn test_generate_struct() {
        let source = "export type Point = { x: number, y: number };";
        let file = parse_file_from_str(source, "test.r.ts").unwrap();
        let analysis = analyze(&file).unwrap();
        let result = generate(&file, &analysis).unwrap();
        assert!(!result.source.is_empty());
    }

    #[test]
    fn test_generate_function() {
        let source = "export function add(a: number, b: number): number { return a + b; }";
        let file = parse_file_from_str(source, "test.r.ts").unwrap();
        let analysis = analyze(&file).unwrap();
        let result = generate(&file, &analysis).unwrap();
        assert!(!result.source.is_empty());
    }

    #[test]
    fn test_generate_tagged_union() {
        let source = "export type Message = { tag: \"Move\", x: number } | { tag: \"Stop\" };";
        let file = parse_file_from_str(source, "test.r.ts").unwrap();
        let analysis = analyze(&file).unwrap();
        let result = generate(&file, &analysis).unwrap();
        assert!(!result.source.is_empty());
    }
}
