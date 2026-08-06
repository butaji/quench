//! OXC parser integration
//!
//! Uses OXC to parse JavaScript/JSX/TypeScript source code into the OXC AST,
//! then lower to our runtime AST via lower.rs.

use crate::ast::Program;
use crate::early_errors;
use crate::lower::stmt::lower_program;
use crate::value::JsError;
use oxc::allocator::Allocator;
use oxc::parser::Parser;
use oxc::span::SourceType;
use std::sync::Arc;

/// Parse JavaScript source using OXC (script mode, not module)
pub fn parse_script(source: &str) -> Result<Program, JsError> {
    // Explicitly mark as script so `await` is not reserved (§11.6.2).
    // SourceType::default() is module-first in OXC.
    let source_type = SourceType::default()
        .with_script(true)
        .with_jsx(true)
        .with_commonjs(
            crate::interpreter::is_direct_eval() && crate::interpreter::is_eval_in_class_field(),
        );
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, source_type).parse();
    if !ret.diagnostics.is_empty() {
        return Err(JsError(format!("Parse error: {:?}", ret.diagnostics)));
    }
    if ret.program.body.iter().any(|statement| {
        matches!(
            statement,
            oxc::ast::ast::Statement::ImportDeclaration(_)
                | oxc::ast::ast::Statement::ExportAllDeclaration(_)
                | oxc::ast::ast::Statement::ExportDefaultDeclaration(_)
                | oxc::ast::ast::Statement::ExportNamedDeclaration(_)
        )
    }) {
        return Err(JsError(
            "SyntaxError: module declaration in script".to_string(),
        ));
    }
    check_strict_reserved(&ret.program, false)?;
    check_strict_fn_params(&ret.program)?;
    check_strict_fn_body(&ret.program)?;
    early_errors::check_early_errors(&ret.program)?;
    early_errors::check_break_continue_errors(&ret.program)?;
    if !crate::interpreter::is_direct_eval() {
        early_errors::check_private_names(&ret.program)?;
    }
    if !crate::interpreter::is_direct_eval() {
        early_errors::check_super_outside_class(&ret.program)?;
    }
    // oxc_semantic-based early error detection
    {
        let semantic_ret = oxc_semantic::SemanticBuilder::new()
            .with_build_nodes(false)
            .build(&ret.program);
        if !semantic_ret.diagnostics.is_empty() {
            let msg = format!("{:?}", semantic_ret.diagnostics);
            return Err(JsError(format!("SyntaxError: {}", msg)));
        }
    }
    lower_program(&ret.program).map_err(|e| JsError(e.to_string()))
}

/// Reject strict-mode future reserved words used as binding identifiers.
/// Strict mode applies when the program has a "use strict" directive prologue
/// or when it is inherited from the calling context (e.g. strict eval).
fn check_strict_reserved(program: &oxc::ast::ast::Program, is_module: bool) -> Result<(), JsError> {
    let strict = crate::strict_reserved::has_use_strict_directive(program)
        || is_module
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

/// Check all function declarations/expressions for strict-mode parameter violations.
/// ES2025 §14.1.2: SyntaxError if `"eval"` or `"arguments"` is a parameter name
/// when the function body is strict mode.
fn check_strict_fn_params(program: &oxc::ast::ast::Program) -> Result<(), JsError> {
    // Check if the program itself has "use strict" (inherited by contained functions)
    let prog_strict = program
        .directives
        .iter()
        .any(|d| d.expression.value == "use strict");

    let mut check_fn = |params: &oxc::ast::ast::FormalParameters,
                        body: &oxc::ast::ast::FunctionBody,
                        inherit_strict: bool|
     -> Result<(), JsError> {
        let strict = inherit_strict
            || body
                .directives
                .iter()
                .any(|d| d.expression.value == "use strict");
        if !strict {
            return Ok(());
        }
        let mut names = Vec::new();
        for param in &params.items {
            if let oxc::ast::ast::BindingPattern::BindingIdentifier(ident) = &param.pattern {
                let name = ident.name.as_str();
                if name == "arguments" || name == "eval" {
                    return Err(JsError(format!(
                        "SyntaxError: Unexpected strict mode reserved word: {}",
                        name
                    )));
                }
                if names.contains(&name.to_string()) {
                    return Err(JsError(format!(
                        "SyntaxError: Duplicate parameter name not allowed in strict mode: {}",
                        name
                    )));
                }
                names.push(name.to_string());
            }
        }
        Ok(())
    };

    fn walk_stmts<'a>(
        stmts: &'a [oxc::ast::ast::Statement<'a>],
        strict: bool,
        check_fn: &mut impl FnMut(
            &oxc::ast::ast::FormalParameters,
            &oxc::ast::ast::FunctionBody,
            bool,
        ) -> Result<(), JsError>,
    ) -> Result<(), JsError> {
        for stmt in stmts {
            match stmt {
                oxc::ast::ast::Statement::FunctionDeclaration(func) => {
                    if let Some(body) = &func.body {
                        let body_strict = strict
                            || body
                                .directives
                                .iter()
                                .any(|d| d.expression.value == "use strict");
                        if body_strict
                            && func.id.as_ref().is_some_and(|id| {
                                id.name.as_str() == "eval" || id.name.as_str() == "arguments"
                            })
                        {
                            return Err(JsError(
                                "SyntaxError: Unexpected strict mode reserved word".into(),
                            ));
                        }
                        check_fn(&func.params, body, strict)?;
                    }
                }
                oxc::ast::ast::Statement::ExpressionStatement(expr) => {
                    walk_expr(&expr.expression, strict, check_fn)?;
                }
                oxc::ast::ast::Statement::VariableDeclaration(var_decl) => {
                    for decl in &var_decl.declarations {
                        if let Some(init) = &decl.init {
                            walk_expr(init, strict, check_fn)?;
                        }
                    }
                }
                oxc::ast::ast::Statement::ReturnStatement(ret) => {
                    if let Some(arg) = &ret.argument {
                        walk_expr(arg, strict, check_fn)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn walk_expr<'a>(
        expr: &'a oxc::ast::ast::Expression<'a>,
        strict: bool,
        check_fn: &mut impl FnMut(
            &oxc::ast::ast::FormalParameters,
            &oxc::ast::ast::FunctionBody,
            bool,
        ) -> Result<(), JsError>,
    ) -> Result<(), JsError> {
        match expr {
            oxc::ast::ast::Expression::FunctionExpression(func) => {
                if let Some(body) = &func.body {
                    check_fn(&func.params, body, strict)?;
                }
            }
            oxc::ast::ast::Expression::ArrowFunctionExpression(arrow) => {
                if let oxc::ast::ast::ArrowFunctionBody::FunctionBody(body) = &arrow.body {
                    check_fn(&arrow.params, body, strict)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    walk_stmts(&program.body, prog_strict, &mut check_fn)?;
    Ok(())
}

/// Check all function bodies for strict-mode violations (e.g., assignment to
/// `eval` or `arguments` in strict mode). ES §16.1: in strict mode, these
/// identifiers cannot appear as assignment targets.
fn check_strict_fn_body(program: &oxc::ast::ast::Program) -> Result<(), JsError> {
    let prog_strict = program
        .directives
        .iter()
        .any(|d| d.expression.value == "use strict");
    walk_body_stmts(&program.body, prog_strict)
}

fn walk_body_stmts(stmts: &[oxc::ast::ast::Statement], strict: bool) -> Result<(), JsError> {
    for stmt in stmts {
        match stmt {
            oxc::ast::ast::Statement::FunctionDeclaration(func) => {
                if (strict
                    || func.body.as_ref().is_some_and(|body| {
                        body.directives
                            .iter()
                            .any(|d| d.expression.value == "use strict")
                    }))
                    && func.id.as_ref().is_some_and(|id| {
                        id.name.as_str() == "eval" || id.name.as_str() == "arguments"
                    })
                {
                    return Err(JsError(
                        "SyntaxError: Unexpected strict mode reserved word".into(),
                    ));
                }
                check_fn_strict_body(&func.params, &func.body, strict)?;
                if let Some(body) = &func.body {
                    let body_strict = strict
                        || body
                            .directives
                            .iter()
                            .any(|d| d.expression.value == "use strict");
                    walk_body_stmts(&body.statements, body_strict)?;
                }
            }
            oxc::ast::ast::Statement::ExpressionStatement(expr) => {
                walk_body_expr(&expr.expression, strict)?;
            }
            oxc::ast::ast::Statement::VariableDeclaration(var_decl) => {
                for decl in &var_decl.declarations {
                    if let Some(init) = &decl.init {
                        walk_body_expr(init, strict)?;
                    }
                }
            }
            oxc::ast::ast::Statement::ReturnStatement(ret) => {
                if let Some(arg) = &ret.argument {
                    walk_body_expr(arg, strict)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn walk_body_expr(expr: &oxc::ast::ast::Expression, strict: bool) -> Result<(), JsError> {
    match expr {
        oxc::ast::ast::Expression::FunctionExpression(func) => {
            check_fn_strict_body(&func.params, &func.body, strict)?;
            if let Some(body) = &func.body {
                walk_body_stmts(&body.statements, strict)?;
            }
        }
        oxc::ast::ast::Expression::ArrowFunctionExpression(arrow) => {
            let body = match &arrow.body {
                oxc::ast::ast::ArrowFunctionBody::FunctionBody(body) => body,
                _ => return Ok(()),
            };
            let body_is_strict = strict
                || body
                    .directives
                    .iter()
                    .any(|d| d.expression.value == "use strict");
            if body_is_strict {
                // Check for eval/arguments as parameter names
                for param in &arrow.params.items {
                    if let oxc::ast::ast::BindingPattern::BindingIdentifier(ident) = &param.pattern
                    {
                        let name = ident.name.as_str();
                        if name == "eval" || name == "arguments" {
                            return Err(JsError(format!(
                                "SyntaxError: Unexpected strict mode reserved word: {}",
                                name
                            )));
                        }
                    }
                }
                // Check body statements for assignments to eval/arguments
                check_body_assignments(&body.statements)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn check_fn_strict_body(
    params: &oxc::ast::ast::FormalParameters,
    body: &Option<oxc::allocator::Box<'_, oxc::ast::ast::FunctionBody<'_>>>,
    inherit_strict: bool,
) -> Result<(), JsError> {
    let Some(b) = body else {
        return Ok(());
    };
    let body_is_strict = inherit_strict
        || b.directives
            .iter()
            .any(|d| d.expression.value == "use strict");
    if !body_is_strict {
        return Ok(());
    }
    // Check for eval/arguments as parameter names
    for param in &params.items {
        if let oxc::ast::ast::BindingPattern::BindingIdentifier(ident) = &param.pattern {
            let name = ident.name.as_str();
            if name == "eval" || name == "arguments" {
                return Err(JsError(format!(
                    "SyntaxError: Unexpected strict mode reserved word: {}",
                    name
                )));
            }
        }
    }
    // Check for assignments to eval/arguments in body statements
    check_body_assignments(&b.statements)?;
    Ok(())
}

fn check_body_assignments(stmts: &[oxc::ast::ast::Statement]) -> Result<(), JsError> {
    for stmt in stmts {
        if let oxc::ast::ast::Statement::ExpressionStatement(es) = stmt {
            check_expr_assignments(&es.expression)?;
        }
    }
    Ok(())
}

fn check_expr_assignments(expr: &oxc::ast::ast::Expression) -> Result<(), JsError> {
    if let oxc::ast::ast::Expression::AssignmentExpression(assign) = expr {
        if let oxc::ast::ast::AssignmentTarget::AssignmentTargetIdentifier(ident) = &assign.left {
            let name = ident.name.as_str();
            if name == "eval" || name == "arguments" {
                return Err(JsError(format!(
                    "SyntaxError: Cannot assign to '{}' in strict mode",
                    name
                )));
            }
        }
    }
    Ok(())
}

/// Parse ES module source using OXC
pub fn parse_es_module(source: &str) -> Result<Program, JsError> {
    let source_type = SourceType::default().with_module(true).with_jsx(true);
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, source_type).parse();
    if !ret.diagnostics.is_empty() {
        return Err(JsError(format!("Parse error: {:?}", ret.diagnostics)));
    }
    check_strict_reserved(&ret.program, true)?;
    check_strict_fn_params(&ret.program)?;
    check_strict_fn_body(&ret.program)?;
    early_errors::check_early_errors(&ret.program)?;
    early_errors::check_module_exported_bindings(&ret.program)?;
    early_errors::check_nested_module_exports(&ret.program)?;
    early_errors::check_module_duplicate_labels(&ret.program)?;
    early_errors::check_module_duplicate_function_names(&ret.program)?;
    early_errors::check_break_continue_errors(&ret.program)?;
    early_errors::check_super_outside_class(&ret.program)?;
    early_errors::check_private_names(&ret.program)?;
    // oxc_semantic-based early error detection
    {
        let semantic_ret = oxc_semantic::SemanticBuilder::new()
            .with_build_nodes(false)
            .build(&ret.program);
        if !semantic_ret.diagnostics.is_empty() {
            let msg = format!("{:?}", semantic_ret.diagnostics);
            return Err(JsError(format!("SyntaxError: {}", msg)));
        }
    }
    lower_program(&ret.program).map_err(|e| JsError(e.to_string()))
}

/// Parse JavaScript/JSX source using OXC (script mode)
pub fn parse_jsx(source: &str) -> Result<Program, JsError> {
    let source_type = SourceType::default().with_jsx(true);
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, source_type).parse();
    if !ret.diagnostics.is_empty() {
        return Err(JsError(format!("Parse error: {:?}", ret.diagnostics)));
    }
    lower_program(&ret.program).map_err(|e| JsError(e.to_string()))
}

/// Parse TypeScript source and strip type annotations
pub fn parse_typescript(source: &str) -> Result<Program, JsError> {
    // Strip import/export statements as they are not supported in script mode
    let stripped = strip_imports_exports(source);
    let source_type = SourceType::default().with_typescript(true).with_jsx(true);
    let allocator = Arc::new(Allocator::default());
    let ret = Parser::new(allocator.as_ref(), &stripped, source_type).parse();
    if !ret.diagnostics.is_empty() {
        return Err(JsError(format!("Parse error: {:?}", ret.diagnostics)));
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
    if !ret.diagnostics.is_empty() {
        return Err(JsError(format!("Parse error: {:?}", ret.diagnostics)));
    }
    let result = lower_program(&ret.program).map_err(|e| JsError(e.to_string()));
    drop(allocator);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_parser_rejects_module_declarations() {
        assert!(parse_script("$DONOTEVALUATE(); export default null;").is_err());
        assert!(parse_script("$DONOTEVALUATE(); import x from './x.js';").is_err());
    }

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

    /// { get: fn } must be a data property (key "get", value is function).
    /// { get() {} } must be a getter accessor (key "get", no value).
    /// OXC already distinguishes these via prop.kind; our lower must not re-interpret.
    #[test]
    fn test_parse_object_getter_vs_data_property() {
        use crate::ast::{Expression, PropertyValue};

        // { get: fn } → data property (NOT a getter accessor)
        let r1 = parse_script(r#"var x = { get: function() {} };"#).unwrap();
        let crate::ast::Program::Script(stmts) = r1;
        let crate::ast::Statement::VarDeclaration {
            init: Some(expr), ..
        } = &stmts[0]
        else {
            panic!("expected VarDeclaration with init")
        };
        let Expression::Object(props) = expr else {
            panic!("expected Object expression")
        };
        assert_eq!(props.len(), 1, "expected 1 property");
        let val = &props[0].1;
        assert!(
            matches!(
                val,
                PropertyValue::Value(Expression::FunctionExpression { .. })
            ),
            "{{get: fn}} must be a Value property, got {:?}",
            val,
        );

        // { get() {} } → concise method (data property, NOT getter accessor)
        let r2 = parse_script(r#"var x = { get() {} };"#).unwrap();
        let crate::ast::Program::Script(stmts) = r2;
        let crate::ast::Statement::VarDeclaration {
            init: Some(expr), ..
        } = &stmts[0]
        else {
            panic!("expected VarDeclaration with init")
        };
        let Expression::Object(props) = expr else {
            panic!("expected Object expression")
        };
        assert_eq!(props.len(), 1, "expected 1 property");
        let val = &props[0].1;
        assert!(
            matches!(
                val,
                PropertyValue::Method(Expression::FunctionExpression { .. })
            ),
            "{{get()}} must be a Value property (concise method), got {:?}",
            val,
        );

        // Same for 'set'
        let r3 = parse_script(r#"var x = { set: function(v) {} };"#).unwrap();
        let crate::ast::Program::Script(stmts) = r3;
        let crate::ast::Statement::VarDeclaration {
            init: Some(expr), ..
        } = &stmts[0]
        else {
            panic!("expected VarDeclaration with init")
        };
        let Expression::Object(props) = expr else {
            panic!("expected Object expression")
        };
        assert!(
            matches!(
                props[0].1,
                PropertyValue::Value(Expression::FunctionExpression { .. })
            ),
            "{{set: fn}} must be a Value property",
        );

        // { set(v) {} } → concise method (data property, NOT setter accessor)
        let r4 = parse_script(r#"var x = { set(v) {} };"#).unwrap();
        let crate::ast::Program::Script(stmts) = r4;
        let crate::ast::Statement::VarDeclaration {
            init: Some(expr), ..
        } = &stmts[0]
        else {
            panic!("expected VarDeclaration with init")
        };
        let Expression::Object(props) = expr else {
            panic!("expected Object expression")
        };
        assert!(
            matches!(
                props[0].1,
                PropertyValue::Method(Expression::FunctionExpression { .. })
            ),
            "{{set(v)}} must be a Value property (concise method), got {:?}",
            props[0].1,
        );
    }

    #[test]
    fn test_parse_function() {
        let result = parse_script("function add(a, b) { return a + b; }");
        assert!(result.is_ok(), "Failed: {:?}", result);
    }

    #[test]
    fn strict_nested_function_rejects_eval_assignment() {
        let result =
            parse_script("function outer() { 'use strict'; function inner() { eval = 42; } }");
        assert!(result.is_err(), "strict nested assignment was accepted");
    }

    #[test]
    fn test_strict_fn_body_rejects_eval_assignment() {
        let result = parse_script("function f() { 'use strict'; eval = 42; }");
        assert!(
            result.is_err(),
            "Should reject eval=42 in strict body: {:?}",
            result
        );
    }

    #[test]
    fn test_strict_fn_body_accepts_non_strict_eval_assignment() {
        let result = parse_script("function f() { eval = 42; }");
        assert!(
            result.is_ok(),
            "Should allow eval=42 in sloppy body: {:?}",
            result
        );
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
    fn parse_strict_with_statement_is_syntax_error() {
        let result = parse_script("\"use strict\"; with ({}) {};");
        assert!(
            result.is_err(),
            "with statement in strict mode must be syntax error"
        );
        let err = result.unwrap_err().to_string();
        assert!(err.contains("SyntaxError"), "unexpected error: {err}");
    }

    struct StrictModeGuard(bool);
    impl Drop for StrictModeGuard {
        fn drop(&mut self) {
            crate::interpreter::set_strict_mode(self.0);
        }
    }

    #[test]
    fn parse_strict_script_rejects_with_in_object_getter() {
        let previous = crate::interpreter::is_strict_mode();
        let _guard = StrictModeGuard(previous);
        crate::interpreter::set_strict_mode(true);
        let result = parse_script("var obj = { get(a) { with(a){} } };");
        assert!(
            result.is_err(),
            "with in strict accessor body should fail parse"
        );
        let err = result.unwrap_err().to_string();
        assert!(err.contains("SyntaxError"), "unexpected parse error: {err}");
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

    #[test]
    fn module_rejects_duplicate_labels() {
        assert!(parse_es_module("label: { label: 0; }").is_err());
    }

    #[test]
    fn module_rejects_duplicate_labels_inside_functions() {
        assert!(parse_es_module("function f() { label: {} label: {} }").is_err());
    }

    #[test]
    fn module_rejects_duplicate_top_level_function_names() {
        assert!(parse_es_module("function f() {} function f() {}").is_err());
    }

    #[test]
    fn module_rejects_export_of_undeclared_binding() {
        assert!(parse_es_module("export { Number };").is_err());
    }

    #[test]
    fn module_rejects_restricted_import_binding() {
        assert!(parse_es_module("import { x as arguments } from 'm';").is_err());
    }

    #[test]
    fn module_rejects_strict_reserved_binding_after_call() {
        assert!(parse_es_module("$DONOTEVALUATE();\n\nvar public;").is_err());
    }

    #[test]
    fn module_rejects_duplicate_class_and_default_function_binding() {
        assert!(parse_es_module("class F {}\nexport default function F() {}").is_err());
    }

    #[test]
    fn test_oxc_parses_class_getter_with_computed_key() {
        // What does OXC produce for `class C { get [expr]() {} }`?
        // Does it produce MethodDefinition with kind=Get, or AccessorProperty?
        use oxc::allocator::Allocator;
        use oxc::parser::Parser;
        use oxc::span::SourceType;

        let source = r#"class C { get [thrower()]() {} }"#;
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, source, source_type).parse();
        assert!(
            ret.diagnostics.is_empty(),
            "Parse errors: {:?}",
            ret.diagnostics
        );

        let cls = &ret.program.body[0];
        println!("Statement type: {:?}", cls);
        // ClassDeclaration has a `class_expr` field
        let cls_body = match cls {
            oxc::ast::ast::Statement::ClassDeclaration(cd) => cd.body.body.as_slice(),
            _ => panic!("Expected ClassDeclaration"),
        };
        println!("Number of class elements: {}", cls_body.len());
        for (i, elem) in cls_body.iter().enumerate() {
            println!("Element {}: {:?}", i, elem);
            match elem {
                oxc::ast::ast::ClassElement::MethodDefinition(m) => {
                    println!("  -> MethodDefinition kind={:?}", m.kind);
                    println!("  -> key: {:?}", m.key);
                    println!("  -> value params: {}", m.value.params.items.len());
                }
                oxc::ast::ast::ClassElement::AccessorProperty(a) => {
                    println!("  -> AccessorProperty type={:?}", a.r#type);
                    println!("  -> key: {:?}", a.key);
                    println!("  -> value: {:?}", a.value);
                }
                _ => {}
            }
        }
    }
}
