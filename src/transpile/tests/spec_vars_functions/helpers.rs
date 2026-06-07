//! Helper functions for variable and function tests

pub use crate::transpile::hir::{
    Decl, Expr, FunctionDecl, ModuleItem, ObjectPatProp, Param, Pat, Pat::*,
    QuoteCodegen, Stmt, Type, VariableDecl, VariableKind,
};
pub use proc_macro2::TokenStream;
pub use quote::ToTokens;

// =============================================================================
// Parser helpers
// =============================================================================

pub fn parse_source(source: &str) -> Vec<ModuleItem> {
    let parser = crate::transpile::parser::TsParser::new();
    parser.parse_source(source).expect("parse failed").items
}

pub fn parse_first_decl(source: &str) -> Decl {
    let items = parse_source(source);
    let item = items.first().expect("no items");
    match item {
        ModuleItem::Decl(d) => d.clone(),
        ModuleItem::Stmt(Stmt::Variable(v)) => Decl::Variable(VariableDecl {
            name: v.name.clone(),
            kind: v.kind.clone(),
            type_: v.type_.clone(),
            init: v.init.clone(),
            pattern: v.pattern.clone(),
        }),
        ModuleItem::Stmt(Stmt::FunctionDecl(f)) => Decl::Function(FunctionDecl {
            name: f.name.clone(),
            generics: f.generics.clone(),
            params: f.params.clone(),
            return_type: f.return_type.clone(),
            body: f.body.clone(),
            is_async: f.is_async,
            is_generator: f.is_generator,
            decorators: f.decorators.clone(),
            throws: f.throws,
            error_type: f.error_type.clone(),
        }),
        _ => panic!("expected decl, got {:?}", item),
    }
}

pub fn find_function(source: &str) -> FunctionDecl {
    let items = parse_source(source);
    for item in &items {
        if let ModuleItem::Decl(Decl::Function(f)) = item {
            return f.clone();
        }
    }
    panic!("no function found in: {}", source);
}

pub fn find_variable(source: &str) -> VariableDecl {
    let items = parse_source(source);
    for item in &items {
        if let ModuleItem::Decl(Decl::Variable(v)) = item {
            return v.clone();
        }
        if let ModuleItem::Stmt(Stmt::Variable(v)) = item {
            return v.clone();
        }
    }
    panic!("no variable found in: {}", source);
}

pub fn find_expr_in_var(source: &str) -> Expr {
    let v = find_variable(source);
    v.init.expect("no init")
}

// =============================================================================
// Codegen helpers
// =============================================================================

pub fn codegen_expr(expr: &Expr) -> TokenStream {
    QuoteCodegen::default().gen_expr(expr)
}

pub fn codegen_stmt(stmt: &Stmt) -> Option<TokenStream> {
    QuoteCodegen::default().gen_stmt(stmt)
}

pub fn codegen_fn(func: &FunctionDecl) -> TokenStream {
    QuoteCodegen::default().gen_fn(func)
}

pub fn codegen_decl(decl: &Decl) -> TokenStream {
    match decl {
        Decl::Function(f) => codegen_fn(f),
        Decl::Variable(v) => {
            let stmt = Stmt::Variable(v.clone());
            codegen_stmt(&stmt).unwrap_or_default()
        }
        _ => TokenStream::new(),
    }
}

pub fn assert_codegen_not_null(expr: &Expr, label: &str) {
    let tokens = codegen_expr(expr);
    let s = tokens.to_string();
    assert!(
        !s.contains("Value :: Null") && !s.contains("Value::Null"),
        "{}: codegen produced Value::Null: {}",
        label,
        s
    );
}
