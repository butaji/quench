//! Expression conversion

use crate::transpile::hir;
use oxc_ast::ast::*;

pub fn convert_expr(expr: &Expression) -> Option<hir::Expr> {
    use Expression::*;
    match expr {
        BooleanLiteral(b) => Some(hir::Expr::Boolean(b.value)),
        NumericLiteral(n) => Some(hir::Expr::Number(n.value)),
        StringLiteral(s) => Some(hir::Expr::String(s.value.to_string())),
        NullLiteral(_) => Some(hir::Expr::Null),
        Identifier(id) => Some(hir::Expr::Ident {
            name: id.name.to_string(),
        }),
        ArrayExpression(_arr) => Some(hir::Expr::Array { elems: vec![] }),
        _ => None,
    }
}
