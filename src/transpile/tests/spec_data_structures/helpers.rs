//! Helper functions for data structure tests

pub use crate::transpile::hir::{
    Decl, Expr, Expr::*, ModuleItem, ObjectPatProp, ObjectProp,
    Pat, Pat::*, QuoteCodegen, Stmt,
};

/// Parse source and extract the first variable's init expression
pub fn parse_expr(source: &str) -> Expr {
    let parser = crate::transpile::parser::TsParser::new();
    let result = parser.parse_source(source).expect("parse failed");
    for item in &result.items {
        match item {
            ModuleItem::Decl(Decl::Variable(v)) => {
                if let Some(expr) = &v.init {
                    return (*expr).clone();
                }
            }
            ModuleItem::Stmt(Stmt::Variable(v)) => {
                if let Some(expr) = &v.init {
                    return (*expr).clone();
                }
            }
            _ => {}
        }
    }
    Expr::Invalid
}

/// Parse source and extract the first pattern
pub fn parse_pat(source: &str) -> Option<Pat> {
    let parser = crate::transpile::parser::TsParser::new();
    let result = parser.parse_source(source).expect("parse failed");
    for item in &result.items {
        match item {
            ModuleItem::Decl(Decl::Variable(v)) => return v.pattern.clone(),
            _ => {}
        }
    }
    None
}
