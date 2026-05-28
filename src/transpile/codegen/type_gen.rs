//! Type generation

use crate::transpile::codegen::CodeGenerator;
use crate::transpile::hir::*;

pub struct TypeGen;

impl TypeGen {
    pub fn type_to_rust(_cg: &CodeGenerator, _type_: &Type) -> String {
        "serde_json::Value".to_string()
    }
}
