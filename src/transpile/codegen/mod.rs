//! Code generator: High-level IR → Rust source code

// pub mod function;
// pub mod infer;
// pub mod pat;
// pub mod response;
// pub mod variable;

// pub use function::FnGen;
// pub use infer::TypeInfer;

pub struct CodeGenerator {
    pub imports: Vec<String>,
    pub indent: usize,
    pub generate_handlers: bool,
    pub type_defs: std::collections::HashMap<String, crate::transpile::hir::TypeDecl>,
}

impl Default for CodeGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeGenerator {
    pub fn new() -> Self {
        Self {
            imports: vec![
                "use runts_lib::runtime::prelude::*;".to_string(),
                "use serde::{Serialize, Deserialize};".to_string(),
            ],
            indent: 0,
            generate_handlers: true,
            type_defs: std::collections::HashMap::new(),
        }
    }
    pub fn to_snake_case(&self, s: &str) -> String {
        let mut r = String::new();
        for (i, c) in s.chars().enumerate() {
            if (c.is_uppercase() || c == '-') && i > 0 {
                r.push('_');
            }
            if c != '-' {
                r.push(c.to_ascii_lowercase());
            }
        }
        r
    }
    pub fn indent_str(&self) -> String {
        "    ".repeat(self.indent)
    }
    pub fn set_generate_handlers(&mut self, v: bool) {
        self.generate_handlers = v;
    }
    pub fn generate_function(
        &mut self,
        _d: &crate::transpile::hir::FunctionDecl,
        _c: bool,
    ) -> anyhow::Result<String> {
        todo!("generate_function moved to plugin-based TokenStream codegen")
    }
    pub fn generate_decl(&mut self, d: &crate::transpile::hir::Decl) -> anyhow::Result<String> {
        match d {
            crate::transpile::hir::Decl::Function(f) => self.generate_function(f, false),
            _ => Ok(String::new()),
        }
    }
    pub fn generate_type_decl(
        &self,
        _t: &crate::transpile::hir::TypeDecl,
    ) -> anyhow::Result<String> {
        Ok(String::new())
    }
    pub fn lookup_type_def(&self, n: &str) -> Option<crate::transpile::hir::Type> {
        self.type_defs.get(n).map(|t| t.type_.clone())
    }
    pub fn infer_expr_type(
        &self,
        _e: &crate::transpile::hir::Expr,
    ) -> Option<crate::transpile::hir::Type> {
        todo!("infer_expr_type moved to plugin-based TokenStream codegen")
    }
    pub fn jsx_attr_to_rust(&self, n: &str) -> String {
        n.to_string()
    }
    pub fn generate_module(&mut self, _module: &crate::transpile::hir::Module) -> anyhow::Result<String> {
        todo!("generate_module moved to plugin-based TokenStream codegen")
    }
    pub fn type_to_rust(&self, _t: &crate::transpile::hir::Type) -> String {
        todo!("type_to_rust moved to plugin-based TokenStream codegen")
    }
    pub fn expr_to_rust(&self, _e: &crate::transpile::hir::Expr) -> String {
        todo!("expr_to_rust moved to plugin-based TokenStream codegen")
    }
}
