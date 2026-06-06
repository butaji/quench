//! Helper functions for control flow tests

use proc_macro2::TokenStream;
use quote::ToTokens;

use crate::transpile::hir::*;
use crate::transpile::parser::TsParser;

/// Parse a statement and return the first statement
pub fn parse_first_stmt(source: &str) -> Stmt {
    let parser = TsParser::new();
    let result = parser.parse_source(source).expect("parse failed");
    let item = result.items.first().expect("no items");
    match item {
        ModuleItem::Stmt(s) => s.clone(),
        ModuleItem::Decl(Decl::Function(_)) => Stmt::FunctionDecl(FunctionDecl {
            name: String::new(),
            generics: vec![],
            params: vec![],
            return_type: None,
            body: None,
            is_async: false,
            is_generator: false,
            decorators: vec![],
            throws: false,
            error_type: None,
        }),
        ModuleItem::Decl(Decl::Variable(v)) => Stmt::Variable(v.clone()),
        ModuleItem::Decl(Decl::Class(_)) => Stmt::Class(ClassDecl {
            name: String::new(),
            extends: None,
            members: vec![],
            generics: vec![],
            methods: vec![],
        }),
        ModuleItem::Decl(Decl::Type(_)) => Stmt::Empty,
        ModuleItem::Import(_) => Stmt::Empty,
        ModuleItem::Export(_) => Stmt::Empty,
    }
}

/// Assert statement parsed from source is NOT Stmt::Empty
pub fn assert_not_empty(source: &str, label: &str) {
    let stmt = parse_first_stmt(source);
    assert!(
        !matches!(stmt, Stmt::Empty),
        "{}: parsed to Stmt::Empty, expected non-empty: {:?}",
        label,
        source
    );
}

/// Get codegen output for a statement, assert it's Some
pub fn assert_codegen_some(stmt: &Stmt, label: &str) -> TokenStream {
    let cg = QuoteCodegen::default();
    let result = cg.gen_stmt(stmt);
    assert!(result.is_some(), "{}: gen_stmt returned None", label);
    result.unwrap()
}

/// Check if TokenStream contains Value::Null (with any spacing)
pub fn contains_value_null(tokens: &TokenStream) -> bool {
    let s = tokens.to_string();
    s.contains("Value :: Null") || s.contains("Value::Null") || s.contains("Value . Null")
}

/// Helper: wrap statement(s) in a function for valid JS
pub fn wrap_in_function(body: &str) -> String {
    format!("function f() {{ {} }}", body)
}

/// Parse a function body and return its statements
pub fn parse_function_body(source: &str) -> Vec<Stmt> {
    let parser = TsParser::new();
    let result = parser.parse_source(source).expect("parse failed");
    for item in &result.items {
        if let ModuleItem::Decl(Decl::Function(ref f)) = item {
            if let Some(ref body) = f.body {
                return body.0.clone();
            }
        }
    }
    vec![]
}

/// Find a statement of a specific type in a list
pub fn find_stmt<T: Fn(&Stmt) -> bool>(stmts: &[Stmt], pred: T) -> Option<&Stmt> {
    stmts.iter().find(|s| pred(s))
}

/// Helper to check if a loop body contains a specific statement
pub fn loop_body_contains(stmt: &Stmt, target: &str) -> bool {
    let stmt = match stmt {
        Stmt::Labeled { body, .. } => body.as_ref(),
        other => other,
    };
    find_loop_body(stmt).map_or(false, |body_stmt| has_target_stmt(body_stmt, target))
}

fn find_loop_body(stmt: &Stmt) -> Option<&Stmt> {
    match stmt {
        Stmt::For { body, .. } => Some(body.as_ref()),
        Stmt::While { body, .. } => Some(body.as_ref()),
        Stmt::DoWhile { body, .. } => Some(body.as_ref()),
        _ => None,
    }
}

fn has_target_stmt(stmt: &Stmt, target: &str) -> bool {
    if let Stmt::Block { stmts } = stmt {
        stmts.iter().any(|s| match target {
            "break" => matches!(s, Stmt::Break { .. }),
            "continue" => matches!(s, Stmt::Continue { .. }),
            _ => false,
        })
    } else {
        false
    }
}
