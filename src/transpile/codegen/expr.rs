//! Expression generation

use crate::transpile::codegen::CodeGenerator;
use crate::transpile::hir::*;

pub struct CodeGenExpr;

impl CodeGenExpr {
    pub fn expr_to_rust(_cg: &CodeGenerator, _expr: &Expr) -> String {
        "serde_json::Value::Null".to_string()
    }
}
