//! Type inference utilities

use crate::transpile::codegen::CodeGenerator;
use crate::transpile::hir::*;

pub struct TypeInfer;

impl TypeInfer {
    pub fn infer_expr_type(_cg: &CodeGenerator, _expr: &Expr) -> Option<Type> {
        None
    }
}
