//! Response generation

use crate::transpile::codegen::CodeGenerator;
use crate::transpile::hir::*;

pub struct ResponseGen;

impl ResponseGen {
    pub fn generate_new_response(cg: &CodeGenerator, args: &[Expr]) -> String {
        let body = args.first().map(|a| cg.expr_to_rust(a)).unwrap_or_default();
        format!("Response::new({})", body)
    }
}
