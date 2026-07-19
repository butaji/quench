//! OXC parser integration
//!
//! Uses OXC to parse JavaScript/JSX/TypeScript source code into the OXC AST,
//! then lower to our runtime AST via lower.rs.

use crate::ast::Program;
use crate::lower::stmt::lower_program;
use crate::value::JsError;
use oxc::allocator::Allocator;
use oxc::parser::Parser;
use oxc::span::SourceType;
use std::sync::Arc;

/// Parse JavaScript source using OXC (script mode, not module)
pub fn parse_script(source: &str) -> Result<Program, JsError> {
    let source_type = SourceType::default().with_jsx(true);
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, source_type).parse();
    if !ret.errors.is_empty() {
        return Err(JsError(format!("Parse error: {:?}", ret.errors)));
    }
    check_strict_reserved(&ret.program)?;
    let result = lower_program(&ret.program).map_err(|e| JsError(e.to_string()));
    result
}

/// Reject strict-mode future reserved words used as binding identifiers.
/// Strict mode applies when the program has a "use strict" directive prologue
/// or when it is inherited from the calling context (e.g. strict eval).
fn check_strict_reserved(program: &oxc::ast::ast::Program) -> Result<(), JsError> {
    let strict = crate::strict_reserved::has_use_strict_directive(program)
        || crate::interpreter::is_strict_mode();
    if !strict {
        return Ok(());
    }
    if let Some(name) = crate::strict_reserved::find_strict_reserved_binding(program) {
        return Err(JsError(format!(
            "SyntaxError: Unexpected strict mode reserved word: {}",
            name
        )));
    }
    Ok(())
}

/// Parse ES module source using OXC
pub fn parse_es_module(source: &str) -> Result<Program, JsError> {
    let source_type = SourceType::default().with_module(true).with_jsx(true);
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, source_type).parse();
    if !ret.errors.is_empty() {
        return Err(JsError(format!("Parse error: {:?}", ret.errors)));
    }
    if let Some(name) = crate::strict_reserved::find_strict_reserved_binding(&ret.program) {
        return Err(JsError(format!(
            "SyntaxError: Unexpected strict mode reserved word: {}",
            name
        )));
    }
    let result = lower_program(&ret.program).map_err(|e| JsError(e.to_string()));
    result
}

/// Parse JavaScript/JSX source using OXC (script mode)
pub fn parse_jsx(source: &str) -> Result<Program, JsError> {
    let source_type = SourceType::default().with_jsx(true);
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, source_type).parse();
    if !ret.errors.is_empty() {
        return Err(JsError(format!("Parse error: {:?}", ret.errors)));
    }
    let result = lower_program(&ret.program).map_err(|e| JsError(e.to_string()));
    result
}

/// Parse TypeScript source and strip type annotations
pub fn parse_typescript(source: &str) -> Result<Program, JsError> {
    // Strip import/export statements as they are not supported in script mode
    let stripped = strip_imports_exports(source);
    let source_type = SourceType::default().with_typescript(true).with_jsx(true);
    let allocator = Arc::new(Allocator::default());
    let ret = Parser::new(allocator.as_ref(), &stripped, source_type).parse();
    if !ret.errors.is_empty() {
        return Err(JsError(format!("Parse error: {:?}", ret.errors)));
    }
    let result = lower_program(&ret.program).map_err(|e| JsError(e.to_string()));
    drop(allocator);
    result
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
    let source_type = SourceType::default().with_typescript(true);
    let allocator = Arc::new(Allocator::default());
    let ret = Parser::new(allocator.as_ref(), source, source_type).parse();
    if !ret.errors.is_empty() {
        return Err(JsError(format!("Parse error: {:?}", ret.errors)));
    }
    let result = lower_program(&ret.program).map_err(|e| JsError(e.to_string()));
    drop(allocator);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple() {
        let result = parse_script("42");
        assert!(result.is_ok(), "Failed: {:?}", result);
    }

    #[test]
    fn test_parse_binary() {
        let result = parse_script("1 + 2;");
        assert!(result.is_ok(), "Failed: {:?}", result);
    }

    #[test]
    fn test_parse_var() {
        let result = parse_script("var x = 1 + 2;");
        assert!(result.is_ok(), "Failed: {:?}", result);
    }

    #[test]
    fn test_parse_object() {
        let result = parse_script(r#"const x = { a: 1, b: 2 };"#);
        assert!(result.is_ok(), "Failed: {:?}", result);
    }

    #[test]
    fn test_parse_function() {
        let result = parse_script("function add(a, b) { return a + b; }");
        assert!(result.is_ok(), "Failed: {:?}", result);
    }

    #[test]
    fn test_parse_arrow() {
        let result = parse_script("const add = (a, b) => a + b;");
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
        let result =
            parse_typescript("interface Foo { bar: number; } const x: Foo = { bar: 1 }; x;");
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
        let result = parse_typescript(
            r#"
            function Test(): JSX.Element {
                const setCount = (c: number) => c + 1;
                return <Box>test</Box>;
            }
        "#,
        );
        assert!(result.is_ok(), "Failed: {:?}", result);
    }

    #[test]
    fn test_parse_legacy_octal_sloppy() {
        // Legacy octal literals (e.g. 01, 07) are allowed in sloppy mode
        let result = parse_script("a = 01;");
        assert!(
            result.is_ok(),
            "OXC should parse legacy octal in sloppy mode: {:?}",
            result
        );
    }

    #[test]
    fn test_directives_in_program() {
        // Check that OXC captures directives separately from body
        use oxc::allocator::Allocator;
        use oxc::parser::Parser;
        use oxc::span::SourceType;

        let source = r#""use strict"; eval("01;")"#;
        let source_type = SourceType::default().with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, source, source_type).parse();
        println!("directives.len() = {}", ret.program.directives.len());
        for d in &ret.program.directives {
            println!("  directive: {:?}", d.directive);
            println!("  expression.value: {:?}", d.expression.value);
        }
        println!("body.len() = {}", ret.program.body.len());
        assert!(
            !ret.program.directives.is_empty(),
            "Expected directives but got none"
        );
    }

    #[test]
    fn test_lowered_program_has_directive() {
        // Verify that lower_program correctly preprends directives
        let result = parse_script(r#""use strict"; eval("01;")"#);
        match &result {
            Ok(crate::ast::Program::Script(stmts)) => {
                println!("lowered statements count: {}", stmts.len());
                if let Some(crate::ast::Statement::Expression(expr)) = stmts.first() {
                    println!("first statement expr: {:?}", expr);
                }
                // First statement should be "use strict" directive
                assert!(!stmts.is_empty(), "Expected at least 1 statement");
                if let Some(crate::ast::Statement::Expression(expr)) = stmts.first() {
                    if let crate::ast::Expression::String(s) = expr.as_ref() {
                        assert_eq!(s.trim(), "use strict", "Expected 'use strict' directive");
                    } else {
                        panic!("Expected String expression, got {:?}", expr);
                    }
                }
            }
            #[allow(unreachable_patterns)]
            Ok(_) => panic!("Expected Script, got something else"),
            Err(e) => panic!("Parse failed: {:?}", e),
        }
    }
}
