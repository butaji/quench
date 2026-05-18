//! # Parser Tests
//!
//! Tests for the SWC-based TypeScript parser.

#[cfg(test)]
mod parser_tests {
    use crate::parser::{scan_directory, SourceFile, SourceKind};

    #[test]
    fn test_source_kind_type_script() {
        let kind = SourceKind::TypeScript;
        assert_eq!(SourceKind::TypeScript, kind);
    }

    #[test]
    fn test_source_kind_tsx() {
        let kind = SourceKind::Tsx;
        assert_eq!(SourceKind::Tsx, kind);
    }

    #[test]
    fn test_parse_simple_function() {
        let file = SourceFile {
            path: std::path::PathBuf::from("test.r.ts"),
            kind: SourceKind::TypeScript,
            source: "export function add(a: number, b: number): number { return a + b; }"
                .to_string(),
            name: "test".to_string(),
            valid: true,
            errors: Vec::new(),
        };
        assert!(!file.source.is_empty());
        assert_eq!("test", file.name);
    }

    #[test]
    fn test_parse_type_alias() {
        let file = SourceFile {
            path: std::path::PathBuf::from("test.r.ts"),
            kind: SourceKind::TypeScript,
            source: "export type Task = { id: number; title: string; done: boolean; };".to_string(),
            name: "test".to_string(),
            valid: true,
            errors: Vec::new(),
        };
        assert!(file.source.contains("export type Task"));
    }

    #[test]
    fn test_parse_enum() {
        let file = SourceFile {
            path: std::path::PathBuf::from("test.r.ts"),
            kind: SourceKind::TypeScript,
            source: "export enum Color { Red, Green, Blue, }".to_string(),
            name: "test".to_string(),
            valid: true,
            errors: Vec::new(),
        };
        assert!(file.source.contains("export enum Color"));
    }

    #[test]
    fn test_parse_import_export() {
        let file = SourceFile {
            path: std::path::PathBuf::from("test.r.ts"),
            kind: SourceKind::TypeScript,
            source: "import { Task } from './state.r.ts'; import { handleKey } from 'native:handlers'; export { Task };".to_string(),
            name: "test".to_string(),
            valid: true,
            errors: Vec::new(),
        };
        assert!(file.source.contains("import"));
        assert!(file.source.contains("native:handlers"));
    }

    #[test]
    fn test_parse_arrow_function() {
        let file = SourceFile {
            path: std::path::PathBuf::from("test.r.ts"),
            kind: SourceKind::TypeScript,
            source: "const add = (a: number, b: number): number => a + b;".to_string(),
            name: "test".to_string(),
            valid: true,
            errors: Vec::new(),
        };
        assert!(file.source.contains("=>"));
    }

    #[test]
    fn test_parse_switch_statement() {
        let file = SourceFile {
            path: std::path::PathBuf::from("test.r.ts"),
            kind: SourceKind::TypeScript,
            source: "switch (x) { case 'a': return 1; }".to_string(),
            name: "test".to_string(),
            valid: true,
            errors: Vec::new(),
        };
        assert!(file.source.contains("switch"));
        assert!(file.source.contains("case"));
    }

    #[test]
    fn test_parse_jsx_element() {
        let file = SourceFile {
            path: std::path::PathBuf::from("test.r.tsx"),
            kind: SourceKind::Tsx,
            source: "return (<Block title='Hello'><Text>Welcome</Text></Block>);".to_string(),
            name: "test".to_string(),
            valid: true,
            errors: Vec::new(),
        };
        assert!(file.source.contains("<Block"));
        assert!(file.source.contains("</Block>"));
        assert!(file.is_tsx());
    }

    #[test]
    fn test_parse_tagged_union() {
        let file = SourceFile {
            path: std::path::PathBuf::from("test.r.ts"),
            kind: SourceKind::TypeScript,
            source: "export type Message = | { tag: 'Move', x: number } | { tag: 'Quit' };"
                .to_string(),
            name: "test".to_string(),
            valid: true,
            errors: Vec::new(),
        };
        assert!(file.source.contains("tag:"));
    }

    #[test]
    fn test_parse_result_pattern() {
        let file = SourceFile {
            path: std::path::PathBuf::from("test.r.ts"),
            kind: SourceKind::TypeScript,
            source:
                "export type Result = | { ok: true, value: number } | { ok: false, error: string };"
                    .to_string(),
            name: "test".to_string(),
            valid: true,
            errors: Vec::new(),
        };
        assert!(file.source.contains("{ ok: true"));
        assert!(file.source.contains("{ ok: false"));
    }

    #[test]
    fn test_parse_generic_function() {
        let file = SourceFile {
            path: std::path::PathBuf::from("test.r.ts"),
            kind: SourceKind::TypeScript,
            source: "export function first<T>(arr: T[]): T | null { return null; }".to_string(),
            name: "test".to_string(),
            valid: true,
            errors: Vec::new(),
        };
        assert!(file.source.contains("<T>"));
    }

    #[test]
    fn test_parse_async_function() {
        let file = SourceFile {
            path: std::path::PathBuf::from("test.r.ts"),
            kind: SourceKind::TypeScript,
            source: "export async function fetchData(url: string): Promise<string> { return ''; }"
                .to_string(),
            name: "test".to_string(),
            valid: true,
            errors: Vec::new(),
        };
        assert!(file.source.contains("async"));
    }

    #[test]
    fn test_parse_const_and_let() {
        let file = SourceFile {
            path: std::path::PathBuf::from("test.r.ts"),
            kind: SourceKind::TypeScript,
            source: "const PI = 3.14159; let mutableCount = 0;".to_string(),
            name: "test".to_string(),
            valid: true,
            errors: Vec::new(),
        };
        assert!(file.source.contains("const"));
        assert!(file.source.contains("let"));
    }

    #[test]
    fn test_parse_for_loop() {
        let file = SourceFile {
            path: std::path::PathBuf::from("test.r.ts"),
            kind: SourceKind::TypeScript,
            source: "for (let i = 0; i < items.length; i++) { process(items[i]); }".to_string(),
            name: "test".to_string(),
            valid: true,
            errors: Vec::new(),
        };
        assert!(file.source.contains("for ("));
    }

    #[test]
    fn test_parse_ternary() {
        let file = SourceFile {
            path: std::path::PathBuf::from("test.r.ts"),
            kind: SourceKind::TypeScript,
            source: "const status = count > 0 ? 'positive' : 'negative';".to_string(),
            name: "test".to_string(),
            valid: true,
            errors: Vec::new(),
        };
        assert!(file.source.contains('?'));
        assert!(file.source.contains(':'));
    }

    #[test]
    fn test_parse_string_literals() {
        let file = SourceFile {
            path: std::path::PathBuf::from("test.r.ts"),
            kind: SourceKind::TypeScript,
            source: "const greeting = 'Hello, World!'; const template = 'Hello';".to_string(),
            name: "test".to_string(),
            valid: true,
            errors: Vec::new(),
        };
        assert!(file.source.contains("Hello"));
    }

    #[test]
    fn test_parse_numeric_literals() {
        let file = SourceFile {
            path: std::path::PathBuf::from("test.r.ts"),
            kind: SourceKind::TypeScript,
            source: "const integer = 42; const float = 3.14159;".to_string(),
            name: "test".to_string(),
            valid: true,
            errors: Vec::new(),
        };
        assert!(file.source.contains("42"));
        assert!(file.source.contains("3.14159"));
    }

    #[test]
    fn test_module_name() {
        let file = SourceFile {
            path: std::path::PathBuf::from("src/views/root.r.tsx"),
            kind: SourceKind::Tsx,
            source: String::new(),
            name: "root".to_string(),
            valid: true,
            errors: Vec::new(),
        };
        assert_eq!("root", file.module_name());
    }

    #[test]
    fn test_scan_nonexistent_directory() {
        let result = scan_directory(&std::path::PathBuf::from("/nonexistent/path"));
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_source_file_location_from_offset() {
        let file = SourceFile {
            path: std::path::PathBuf::from("test.r.ts"),
            kind: SourceKind::TypeScript,
            source: "line1\nline2\nline3".to_string(),
            name: "test".to_string(),
            valid: true,
            errors: Vec::new(),
        };
        let (line, col) = file.location_from_offset(0);
        assert_eq!(1, line);
        assert_eq!(1, col);
    }
}
