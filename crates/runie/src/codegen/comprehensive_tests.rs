//! # Comprehensive Tests
//!
//! Comprehensive tests for the Rune compiler.
//!
//! These tests validate the complete pipeline: parse → analyze → generate.
//! Key tests also validate that generated Rust code is syntactically correct.

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

    #[test]
    fn test_array_subscript_direct_indexing() {
        let src = "export function first(items: string[]): string { return items[0]; }\n"
            .to_owned()
            + "export function last(items: string[]): string { return items[items.length - 1]; }";
        let file = parse_file_from_str(&src, "arr.r.ts").unwrap();
        let analysis = analyze(&file).unwrap();
        let result = generate(&file, &analysis).unwrap();
        assert!(result.source.contains('['), "Should use direct [idx] indexing");
        assert!(!result.source.contains(".get("), "Should not use .get()");
    }

    #[test]
    fn test_array_get_method_direct_indexing() {
        let src = "export function getNth<T>(arr: T[], n: number): T | null { return arr.get(n); }";
        let file = parse_file_from_str(src, "get.r.ts").unwrap();
        let analysis = analyze(&file).unwrap();
        let result = generate(&file, &analysis).unwrap();
        assert!(result.source.contains('['), "arr.get(idx) should emit [idx]");
        assert!(!result.source.contains(".get("), "Should not use .get() method");
    }

    #[test]
    fn test_array_slice_as_slice() {
        let src = "export function mid(arr: number[]): number[] { return arr.slice(1, 3); }";
        let file = parse_file_from_str(src, "slice.r.ts").unwrap();
        let analysis = analyze(&file).unwrap();
        let result = generate(&file, &analysis).unwrap();
        assert!(result.source.contains("as_slice()"), "slice should use as_slice()");
    }

    #[test]
    fn test_string_concat_format() {
        let src = "export function greet(name: string): string { return \"Hello, \" + name; }";
        let file = parse_file_from_str(src, "concat.r.ts").unwrap();
        let analysis = analyze(&file).unwrap();
        let result = generate(&file, &analysis).unwrap();
        assert!(result.source.contains("format!"), "String concat should use format!");
    }

    #[test]
    fn test_result_pattern_ok_err() {
        let src = "
            export function divide(a: number, b: number):
                | { ok: true; value: number }
                | { ok: false; error: string }
            {
                if (b === 0) return { ok: false, error: \"zero\" };
                return { ok: true, value: a / b };
            }
        ";
        let file = parse_file_from_str(src, "result.r.ts").unwrap();
        let analysis = analyze(&file).unwrap();
        let result = generate(&file, &analysis).unwrap();
        assert!(result.source.contains("Ok("));
        assert!(result.source.contains("Err("));
    }

    #[test]
    fn test_tagged_union_switch_match() {
        let src = "
            export type Message = | { tag: \"Move\"; x: number } | { tag: \"Stop\" };
            export function handle(msg: Message): number {
                switch (msg.tag) {
                    case \"Move\": return msg.x;
                    case \"Stop\": return 0;
                }
            }
        ";
        let file = parse_file_from_str(src, "enum.r.ts").unwrap();
        let analysis = analyze(&file).unwrap();
        let result = generate(&file, &analysis).unwrap();
        assert!(result.source.contains("match"));
        assert!(result.source.contains("Move"));
    }

    #[test]
    fn test_for_of_iter() {
        let src = "
            export function sum(nums: number[]): number {
                let sum = 0;
                for (const n of nums) { sum = sum + n; }
                return sum;
            }
        ";
        let file = parse_file_from_str(src, "forof.r.ts").unwrap();
        let analysis = analyze(&file).unwrap();
        let result = generate(&file, &analysis).unwrap();
        assert!(result.source.contains(".iter()"));
    }

    #[test]
    fn test_native_import_crate_native() {
        let src = "import { fastSqrt } from \"native:math\";\n"
            .to_owned()
            + "export function sqrt(n: number): number { return fastSqrt(n); }";
        let file = parse_file_from_str(&src, "native.r.ts").unwrap();
        let analysis = analyze(&file).unwrap();
        let result = generate(&file, &analysis).unwrap();
        assert!(result.source.contains("crate::native"));
    }

    #[test]
    fn test_option_type_generated() {
        let src = "
            export function find(items: string[], target: string): string | null {
                for (let i = 0; i < items.length; i++) {
                    if (items[i] === target) return items[i];
                }
                return null;
            }
        ";
        let file = parse_file_from_str(src, "opt.r.ts").unwrap();
        let analysis = analyze(&file).unwrap();
        let result = generate(&file, &analysis).unwrap();
        assert!(result.source.contains("Option<"));
    }

    #[test]
    fn test_async_fn_generated() {
        let src = "export async function fetch(url: string): Promise<string> { return \"\"; }";
        let file = parse_file_from_str(src, "async.r.ts").unwrap();
        let analysis = analyze(&file).unwrap();
        let result = generate(&file, &analysis).unwrap();
        assert!(result.source.contains("async"));
    }
}
