//! Statement generation

use crate::transpile::codegen::CodeGenerator;
use crate::transpile::hir::*;

pub struct CodeGenStmt;

impl CodeGenStmt {
    pub fn stmt_to_rust(_cg: &mut CodeGenerator, _stmt: &Stmt) -> anyhow::Result<String> {
        Ok(String::new())
    }
}
