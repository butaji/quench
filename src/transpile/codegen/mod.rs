//! Code generator: High-level IR → Rust source code

pub mod expr;
pub mod function;
pub mod infer;
pub mod jsx;
pub mod module;
pub mod pat;
pub mod response;
pub mod stmt;
pub mod type_gen;
pub mod variable;

pub use expr::CodeGenExpr;
pub use function::FnGen;
pub use infer::TypeInfer;
pub use jsx::CodeGenJsx;
pub use module::ModuleGen;
pub use pat::PatGen;
pub use response::ResponseGen;
pub use stmt::CodeGenStmt;
pub use type_gen::TypeGen;
pub use variable::VarGen;

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
    pub fn type_to_rust(&self, t: &crate::transpile::hir::Type) -> String {
        TypeGen::type_to_rust(self, t)
    }
    pub fn expr_to_rust(&self, e: &crate::transpile::hir::Expr) -> String {
        CodeGenExpr::expr_to_rust(self, e)
    }
    pub fn jsx_to_rust(&mut self, x: &crate::transpile::hir::JSXExpr) -> String {
        let mut cg = CodeGenJsx;
        cg.jsx_to_rust(x)
    }
    pub fn set_generate_handlers(&mut self, v: bool) {
        self.generate_handlers = v;
    }
    pub fn stmt_to_rust(&mut self, s: &crate::transpile::hir::Stmt) -> anyhow::Result<String> {
        CodeGenStmt::stmt_to_rust(self, s)
    }
    pub fn generate_module(&mut self, m: &crate::transpile::hir::Module) -> anyhow::Result<String> {
        ModuleGen::generate_module(self, m)
    }
    pub fn generate_function(
        &mut self,
        d: &crate::transpile::hir::FunctionDecl,
        c: bool,
    ) -> anyhow::Result<String> {
        FnGen::generate_function(self, d, c)
    }
    pub fn generate_decl(&mut self, d: &crate::transpile::hir::Decl) -> anyhow::Result<String> {
        match d {
            crate::transpile::hir::Decl::Function(f) => self.generate_function(f, false),
            _ => Ok(String::new()),
        }
    }
    pub fn generate_type_decl(
        &self,
        t: &crate::transpile::hir::TypeDecl,
    ) -> anyhow::Result<String> {
        Ok(String::new())
    }
    pub fn lookup_type_def(&self, n: &str) -> Option<crate::transpile::hir::Type> {
        self.type_defs.get(n).map(|t| t.type_.clone())
    }
    pub fn infer_expr_type(
        &self,
        e: &crate::transpile::hir::Expr,
    ) -> Option<crate::transpile::hir::Type> {
        TypeInfer::infer_expr_type(self, e)
    }
    pub fn jsx_attr_to_rust(&self, n: &str) -> String {
        n.to_string()
    }
}
