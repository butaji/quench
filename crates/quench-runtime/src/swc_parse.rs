//! SWC parser integration
//!
//! Uses swc to parse JavaScript/JSX/TypeScript source code into the swc AST,
//! then lower to our runtime AST via lower.rs.

use swc_common::{
    sync::Lrc,
    FileName, SourceMap,
};
use swc_ecma_parser::{
    lexer::Lexer,
    Parser,
    StringInput,
    Syntax,
    EsSyntax,
    TsSyntax,
};
use crate::ast::Program;
use crate::value::JsError;
use crate::lower::lower_script;

/// Parse JavaScript source into an swc `Script` AST.
pub fn parse_swc_script(source: &str) -> Result<swc_ecma_ast::Script, JsError> {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(
        Lrc::new(FileName::Custom("input".into())),
        source.to_string(),
    );

    let lexer = Lexer::new(
        Syntax::Es(EsSyntax {
            jsx: false,
            ..Default::default()
        }),
        Default::default(),
        StringInput::from(&*fm),
        None,
    );

    let mut parser = Parser::new_from(lexer);

    parser.parse_script().map_err(|e| {
        JsError(format!("Parse error: {:?}", e))
    })
}

/// Parse JavaScript source using swc (script mode, not module)
pub fn parse_swc(source: &str) -> Result<Program, JsError> {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(
        Lrc::new(FileName::Custom("input".into())),
        source.to_string(),
    );
    
    let lexer = Lexer::new(
        Syntax::Es(EsSyntax {
            jsx: true,
            ..Default::default()
        }),
        Default::default(),
        StringInput::from(&*fm),
        None,
    );
    
    let mut parser = Parser::new_from(lexer);
    
    // Use parse_script for regular JS/JSX code (not ES modules)
    let script = parser.parse_script().map_err(|e| {
        JsError(format!("Parse error: {:?}", e))
    })?;
    
    // Lower swc AST to our runtime AST
    lower_script(&script).map_err(|e| JsError(e.to_string()))
}

/// Parse JavaScript/JSX source using swc (script mode)
pub fn parse_jsx(source: &str) -> Result<Program, JsError> {
    parse_with_syntax(source, Syntax::Es(EsSyntax { jsx: true, ..Default::default() }))
}

/// Parse TypeScript source and strip type annotations
pub fn parse_typescript(source: &str) -> Result<Program, JsError> {
    // Strip import/export statements as they are not supported in script mode
    let stripped = strip_imports_exports(source);
    parse_with_syntax(
        &stripped,
        Syntax::Typescript(TsSyntax {
            tsx: true,
            decorators: true,
            ..Default::default()
        }),
    )
}

/// Strip import/export statements for script-mode parsing
fn strip_imports_exports(source: &str) -> String {
    source
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.starts_with("import ")
                && !trimmed.starts_with("export ")
                && !trimmed.starts_with("import type ")
                && !trimmed.starts_with("export type ")
                && !trimmed.starts_with("import =")
                && !trimmed.starts_with("export =")
                && !trimmed.starts_with("export {")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parse TypeScript without JSX support
#[allow(dead_code)]
pub fn parse_ts(source: &str) -> Result<Program, JsError> {
    parse_with_syntax(
        source,
        Syntax::Typescript(TsSyntax {
            tsx: false,
            decorators: true,
            ..Default::default()
        }),
    )
}

fn parse_with_syntax(source: &str, syntax: Syntax) -> Result<Program, JsError> {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(
        Lrc::new(FileName::Custom("input".into())),
        source.to_string(),
    );

    let lexer = Lexer::new(syntax, Default::default(), StringInput::from(&*fm), None);
    let mut parser = Parser::new_from(lexer);

    let script = parser.parse_script().map_err(|e| JsError(format!("Parse error: {:?}", e)))?;
    lower_script(&script).map_err(|e| JsError(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple() {
        let result = parse_swc("42");
        assert!(result.is_ok(), "Failed: {:?}", result);
    }

    #[test]
    fn test_parse_binary() {
        let result = parse_swc("1 + 2;");
        assert!(result.is_ok(), "Failed: {:?}", result);
    }

    #[test]
    fn test_parse_var() {
        let result = parse_swc("var x = 1 + 2;");
        assert!(result.is_ok(), "Failed: {:?}", result);
    }

    #[test]
    fn test_parse_object() {
        let result = parse_swc(r#"const x = { a: 1, b: 2 };"#);
        assert!(result.is_ok(), "Failed: {:?}", result);
    }

    #[test]
    fn test_parse_function() {
        let result = parse_swc("function add(a, b) { return a + b; }");
        assert!(result.is_ok(), "Failed: {:?}", result);
    }

    #[test]
    fn test_parse_arrow() {
        let result = parse_swc("const add = (a, b) => a + b;");
        assert!(result.is_ok(), "Failed: {:?}", result);
    }

    #[test]
    fn test_parse_typescript_basic() {
        // Test TypeScript type annotations are stripped
        let result = parse_typescript("const x: number = 42; x;");
        assert!(result.is_ok(), "Failed: {:?}", result);
    }

    #[test]
    fn test_parse_typescript_interface() {
        // Test that TypeScript interface declarations are handled
        let result = parse_typescript("interface Foo { bar: number; } const x: Foo = { bar: 1 }; x;");
        assert!(result.is_ok(), "Failed: {:?}", result);
    }

    #[test]
    fn test_parse_typescript_jsx() {
        // Test TypeScript with JSX
        let result = parse_typescript("const el = <div>hello</div>; el;");
        assert!(result.is_ok(), "Failed: {:?}", result);
    }

    #[test]
    fn test_parse_typescript_with_arrow_params() {
        // Test TypeScript with type annotations in arrow function parameters
        let result = parse_typescript("const setCount = (c: number) => c + 1; setCount;");
        assert!(result.is_ok(), "Failed: {:?}", result);
    }

    #[test]
    fn test_parse_typescript_complex() {
        // Test more complex TypeScript with JSX
        let result = parse_typescript(r#"
            function Test(): JSX.Element {
                const setCount = (c: number) => c + 1;
                return <Box>test</Box>;
            }
        "#);
        assert!(result.is_ok(), "Failed: {:?}", result);
    }

    #[test]
    fn test_parse_counter_tsx() {
        // Test parsing the actual counter.tsx file
        // Path relative to crate root
        let source = std::fs::read_to_string("../../examples/counter.tsx").unwrap();
        let result = parse_typescript(&source);
        assert!(result.is_ok(), "Failed to parse counter.tsx: {:?}", result);
    }

    #[test]
    fn test_parse_runtime_js() {
        let source = std::fs::read_to_string("../../src/runtime.js").unwrap();
        let result = parse_swc(&source);
        assert!(result.is_ok(), "Failed to parse runtime.js: {:?}", result);
    }
}
