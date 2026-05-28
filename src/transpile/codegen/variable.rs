//! Variable declaration generation

use anyhow::Result;
use crate::transpile::codegen::CodeGenerator;
use crate::transpile::hir::*;

pub struct VarGen;

impl VarGen {
    pub fn generate_variable(cg: &CodeGenerator, var: &VariableDecl) -> Result<String> {
        let mut output = String::new();
        let keyword = match var.kind {
            VariableKind::Const => "const",
            VariableKind::Let => "let mut",
            VariableKind::Var => "let",
        };
        let _type_hint = var.type_.as_ref().map(|t| format!(": {}", cg.type_to_rust(t)));
        let init = var.init.as_ref().map(|e| {
            let val = cg.expr_to_rust(e);
            if var.kind == VariableKind::Const {
                format!(": {} = {}", cg.type_to_rust(var.type_.as_ref().unwrap_or(&Type::Unknown)), val)
            } else {
                format!(" = {}", val)
            }
        });

        if let Some(init) = init {
            output.push_str(&format!("{}{}{};\n", keyword, var.name, init));
        }
        Ok(output)
    }
}
