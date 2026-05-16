//! # Validation Rules
//!
//! Individual validation rules for the TypeScript subset.

use swc_ecma_ast::*;
use crate::analyzer::ValidationError;
use crate::analyzer::context::AnalysisContext;

/// Check if an expression is allowed.
pub fn validate_expr_is_allowed(expr: &Expr, ctx: &mut AnalysisContext) -> Option<ValidationError> {
    match expr {
        Expr::Class(_) => Some(ValidationError {
            location: ctx.current_location(),
            message: "Classes are forbidden. Use plain objects.".into(),
            code: "no-class",
        }),
        Expr::This(_) => Some(ValidationError {
            location: ctx.current_location(),
            message: "this is forbidden. Use explicit parameters.".into(),
            code: "no-this",
        }),
        Expr::Super(_) => Some(ValidationError {
            location: ctx.current_location(),
            message: "super is forbidden.".into(),
            code: "no-super",
        }),
        Expr::Yield(_) => Some(ValidationError {
            location: ctx.current_location(),
            message: "yield is forbidden.".into(),
            code: "no-yield",
        }),
        Expr::MetaProp(_) => Some(ValidationError {
            location: ctx.current_location(),
            message: "Meta properties are forbidden.".into(),
            code: "no-metaprop",
        }),
        _ => None,
    }
}

/// Check if a literal is allowed.
pub fn validate_lit_is_allowed(lit: &Lit, ctx: &mut AnalysisContext) -> Option<ValidationError> {
    match lit {
        Lit::Regex(_) => Some(ValidationError {
            location: ctx.current_location(),
            message: "Regex literals are forbidden.".into(),
            code: "no-regex",
        }),
        _ => None,
    }
}

/// Check if a binary operator is allowed.
pub fn validate_bin_op_is_allowed(op: BinaryOp, ctx: &mut AnalysisContext) -> Option<ValidationError> {
    match op {
        BinaryOp::EqEq | BinaryOp::NotEq => Some(ValidationError {
            location: ctx.current_location(),
            message: "Use === instead of ==".into(),
            code: "no-loose-eq",
        }),
        _ => None,
    }
}

/// Check if a statement is allowed.
pub fn validate_stmt_is_allowed(stmt: &Stmt, ctx: &mut AnalysisContext) -> Option<ValidationError> {
    match stmt {
        Stmt::Try(_) => Some(ValidationError {
            location: ctx.current_location(),
            message: "try/catch is forbidden. Use Result<T,E> pattern.".into(),
            code: "no-try-catch",
        }),
        Stmt::Throw(_) => Some(ValidationError {
            location: ctx.current_location(),
            message: "throw is forbidden. Use Result<T,E> pattern.".into(),
            code: "no-throw",
        }),
        Stmt::With(_) => Some(ValidationError {
            location: ctx.current_location(),
            message: "with is forbidden.".into(),
            code: "no-with",
        }),
        _ => None,
    }
}

/// Check if a declaration is allowed.
pub fn validate_decl_is_allowed(decl: &Decl, ctx: &mut AnalysisContext) -> Option<ValidationError> {
    match decl {
        Decl::Class(_) => Some(ValidationError {
            location: ctx.current_location(),
            message: "Classes are forbidden. Use plain objects.".into(),
            code: "no-class",
        }),
        _ => None,
    }
}

/// Check if a TypeScript keyword type is allowed.
pub fn validate_ts_keyword_is_allowed(kind: TsKeywordTypeKind, ctx: &mut AnalysisContext) -> Option<ValidationError> {
    match kind {
        TsKeywordTypeKind::TsAnyType | TsKeywordTypeKind::TsUnknownType => {
            Some(ValidationError {
                location: ctx.current_location(),
                message: "any and unknown are forbidden. Use concrete types.".into(),
                code: "no-any-unknown",
            })
        }
        _ => None,
    }
}

/// Check if a module declaration is allowed.
pub fn validate_module_decl_is_allowed(decl: &ModuleDecl, ctx: &mut AnalysisContext) -> Option<ValidationError> {
    match decl {
        ModuleDecl::ImportStar(_) => Some(ValidationError {
            location: ctx.current_location(),
            message: "Wildcard imports are forbidden.".into(),
            code: "no-wildcard-import",
        }),
        _ => None,
    }
}
