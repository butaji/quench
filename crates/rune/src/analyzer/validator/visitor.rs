//! # Validation Visitors
//!
//! Visitor traits for AST traversal.

use swc_ecma_ast::*;
use crate::analyzer::context::AnalysisContext;
use super::ValidationError;

/// Validation visitor trait.
pub trait ValidateVisitor {
    fn visit_module(&mut self, module: &Module, ctx: &mut AnalysisContext) -> crate::Result<()>;
    fn visit_stmt(&mut self, stmt: &Stmt, ctx: &mut AnalysisContext) -> crate::Result<()>;
    fn visit_expr(&mut self, expr: &Expr, ctx: &mut AnalysisContext) -> crate::Result<()>;
    fn visit_decl(&mut self, decl: &Decl, ctx: &mut AnalysisContext) -> crate::Result<()>;
}

/// Default implementation for ValidateVisitor.
impl<T: ?Sized> ValidateVisitor for T
where
    T: FnMut(&dyn ValidateVisitor, &Module, &mut AnalysisContext) -> crate::Result<()>,
{
    fn visit_module(&mut self, _module: &Module, _ctx: &mut AnalysisContext) -> crate::Result<()> {
        Ok(())
    }

    fn visit_stmt(&mut self, _stmt: &Stmt, _ctx: &mut AnalysisContext) -> crate::Result<()> {
        Ok(())
    }

    fn visit_expr(&mut self, _expr: &Expr, _ctx: &mut AnalysisContext) -> crate::Result<()> {
        Ok(())
    }

    fn visit_decl(&mut self, _decl: &Decl, _ctx: &mut AnalysisContext) -> crate::Result<()> {
        Ok(())
    }
}
