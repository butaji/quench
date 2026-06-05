use proc_macro2::TokenStream;
use quote::ToTokens;

use crate::transpile::hir::{
    Decl, Expr, JSXAttr, JSXAttrValue, JSXChild, JSXExpr, JSXName, Module,
    ModuleItem, QuoteCodegen,
};

// =============================================================================
    // Helpers
    // =============================================================================

    fn parse_jsx(source: &str) -> Module {
        let parser = crate::transpile::parser::TsParser::new();
        parser.parse_tsx(source).expect("parse failed")
    }

    fn find_jsx_expr(module: &Module) -> Option<JSXExpr> {
        for item in &module.items {
            if let ModuleItem::Decl(Decl::Variable(var)) = item {
                if let Some(Expr::JSX(jsx)) = &var.init {
                    return Some(jsx.clone());
                }
            }
        }
        None
    }

    fn find_jsx_expr_in_stmt(module: &Module) -> Option<JSXExpr> {
        for item in &module.items {
            if let Some(jsx) = find_jsx_in_module_item(item) {
                return Some(jsx);
            }
        }
        None
    }

    fn find_jsx_in_module_item(item: &ModuleItem) -> Option<JSXExpr> {
        match item {
            ModuleItem::Decl(Decl::Variable(var)) => find_jsx_in_var_decl(var),
            ModuleItem::Decl(Decl::Function(func)) => find_jsx_in_function(func),
            ModuleItem::Stmt(stmt) => find_jsx_in_stmt(stmt),
            _ => None,
        }
    }

    fn find_jsx_in_var_decl(var: &crate::transpile::hir::VariableDecl) -> Option<JSXExpr> {
        if let Some(Expr::JSX(jsx)) = &var.init {
            return Some(jsx.clone());
        }
        None
    }

    fn find_jsx_in_function(func: &crate::transpile::hir::FunctionDecl) -> Option<JSXExpr> {
        if let Some(ref body) = func.body {
            for stmt in &body.0 {
                if let crate::transpile::hir::Stmt::Return { arg: Some(Expr::JSX(jsx)) } = stmt {
                    return Some(jsx.clone());
                }
            }
        }
        None
    }

    fn find_jsx_in_stmt(stmt: &crate::transpile::hir::Stmt) -> Option<JSXExpr> {
        if let crate::transpile::hir::Stmt::Return { arg: Some(Expr::JSX(jsx)) } = stmt {
            return Some(jsx.clone());
        }
        None
    }

    /// Check if TokenStream contains Value::Null (with various quote spacing)
    fn contains_value_null(tokens: &TokenStream) -> bool {
        let s = tokens.to_string();
        s.contains("Value :: Null") || s.contains("Value::Null") || s.contains("Value . Null")
    }

    /// Assert JSX parses and is not Expr::Invalid
    fn assert_jsx_parses(source: &str) -> JSXExpr {
        let module = parse_jsx(source);
        let jsx = find_jsx_expr(&module).expect(&format!(
            "Expected to find JSX expr in: {}\nModule: {:#?}",
            source, module
        ));
        jsx
    }

    /// Assert JSX codegen produces non-empty tokens
    fn assert_codegen_not_empty(jsx: &JSXExpr) {
        let expr = Expr::JSX(jsx.clone());
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        assert!(!tokens.is_empty(), "Codegen should produce tokens for JSX");
    }

    // =============================================================================
    // 3.1 JSX Elements
    // =============================================================================

