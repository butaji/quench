//! Code generator: High-level IR → Rust source code

use anyhow::Result;

pub mod stmt;
pub mod expr;
pub mod jsx;
pub mod type_gen;
pub mod pat;
pub mod response;
pub mod variable;
pub mod function;
pub mod module;
pub mod infer;

pub use stmt::CodeGenStmt;
pub use expr::CodeGenExpr;
pub use jsx::CodeGenJsx;
pub use type_gen::TypeGen;
pub use pat::PatGen;
pub use response::ResponseGen;
pub use variable::VarGen;
pub use function::FnGen;
pub use module::ModuleGen;
pub use infer::TypeInfer;

use super::hir::*;
#[allow(unused_imports)]
use crate::transpile::codegen::{stmt::*, expr::*, jsx::*, type_gen::*, pat::*, response::*, variable::*, function::*, module::*, infer::*};

pub struct CodeGenerator {
    pub imports: Vec<String>,
    pub indent: usize,
    pub generate_handlers: bool,
    pub type_defs: std::collections::HashMap<String, TypeDecl>,
}

impl Default for CodeGenerator {
    fn default() -> Self { Self::new() }
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

    pub fn set_generate_handlers(&mut self, value: bool) {
        self.generate_handlers = value;
    }

    pub fn to_snake_case(&self, s: &str) -> String {
        let mut result = String::new();
        for (i, c) in s.chars().enumerate() {
            if (c.is_uppercase() || c == '-') && i > 0 {
                result.push('_');
            }
            if c != '-' {
                result.push(c.to_ascii_lowercase());
            }
        }
        result
    }

    pub fn to_pascal_case(&self, s: &str) -> String {
        let mut result = String::new();
        let mut capitalize = true;
        for c in s.chars() {
            if c == '_' || c == '-' {
                capitalize = true;
            } else if capitalize {
                result.push(c.to_ascii_uppercase());
                capitalize = false;
            } else {
                result.push(c);
            }
        }
        result
    }

    pub fn to_kebab_case(&self, s: &str) -> String {
        let mut result = String::new();
        for (i, c) in s.chars().enumerate() {
            if c.is_uppercase() && i > 0 {
                result.push('-');
            }
            result.push(c.to_ascii_lowercase());
        }
        result
    }

    pub fn strip_outer_parens(s: &str) -> String {
        let s = s.trim();
        if s.len() >= 2 && s.starts_with('(') && s.ends_with(')') {
            let mut depth = 0;
            let bytes = s.as_bytes();
            for (i, &b) in bytes.iter().enumerate() {
                if b == b'(' { depth += 1; }
                else if b == b')' {
                    depth -= 1;
                    if depth == 0 && i == s.len() - 1 {
                        return Self::strip_outer_parens(&s[1..s.len() - 1]);
                    }
                }
            }
        }
        s.to_string()
    }

    pub fn indent_str(&self) -> String {
        "    ".repeat(self.indent)
    }

    pub fn with_indent<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.indent += 1;
        let result = f(self);
        self.indent -= 1;
        result
    }

    pub fn type_to_rust(&self, type_: &Type) -> String {
        TypeGen::type_to_rust(self, type_)
    }

    pub fn stmt_to_rust(&mut self, stmt: &Stmt) -> Result<String> {
        CodeGenStmt::stmt_to_rust(self, stmt)
    }

    pub fn expr_to_rust(&self, expr: &Expr) -> String {
        CodeGenExpr::expr_to_rust(self, expr)
    }

    pub fn jsx_to_rust(&self, x: &JSXExpr) -> String {
        CodeGenJsx::jsx_to_rust(self, x)
    }

    pub fn jsx_element_to_string(&self, x: &JSXExpr, depth: usize) -> String {
        CodeGenJsx::jsx_element_to_string(self, x, depth)
    }

    pub fn jsx_element_inner(&self, x: &JSXExpr) -> String {
        CodeGenJsx::jsx_element_inner(self, x)
    }

    pub fn jsx_attr_value_to_rust(&self, v: &JSXAttrValue) -> String {
        CodeGenJsx::jsx_attr_value_to_rust(self, v)
    }

    pub fn jsx_child_to_rust(&self, child: &JSXChild) -> String {
        CodeGenJsx::jsx_child_to_rust(self, child)
    }

    pub fn expr_needs_clone(&self, e: &Expr) -> bool {
        CodeGenJsx::expr_needs_clone(e)
    }

    pub fn jsx_style_value_to_rust(&self, v: &JSXAttrValue) -> String {
        CodeGenJsx::jsx_style_value_to_rust(self, v)
    }

    pub fn generate_module(&mut self, module: &Module) -> Result<String> {
        ModuleGen::generate_module(self, module)
    }

    pub fn generate_function(&mut self, decl: &FunctionDecl, is_component: bool) -> Result<String> {
        FnGen::generate_function(self, decl, is_component)
    }

    pub fn generate_handler_method(&self, method: &str, value: &Expr) -> Result<String> {
        FnGen::generate_handler_method(self, method, value)
    }

    pub fn generate_variable(&self, var: &VariableDecl) -> Result<String> {
        VarGen::generate_variable(self, var)
    }

    pub fn generate_decl(&mut self, decl: &Decl) -> Result<String> {
        match decl {
            Decl::Function(f) => self.generate_function(f, false),
            Decl::Variable(v) => self.generate_variable(v),
            Decl::Class(_) => Ok(String::new()),
            Decl::Type(_) => Ok(String::new()),
        }
    }

    pub fn generate_type_decl(&self, t: &TypeDecl) -> Result<String> {
        use crate::transpile::hir::Type as H;
        match &t.type_ {
            H::Union { types } if types.iter().all(|t| matches!(t, H::Literal { kind: crate::transpile::hir::LiteralKind::String, .. })) => {
                let variants: Vec<String> = types.iter().map(|t| {
                    if let H::Literal { kind: crate::transpile::hir::LiteralKind::String, value } = t {
                        let var_name = self.to_pascal_case(value);
                        format!("    {}(String),", var_name)
                    } else {
                        String::new()
                    }
                }).collect();
                let variants_str = variants.join("\n");
                let generics_str = if t.generics.is_empty() { String::new() } else { format!("<{}>", t.generics.len()) };
                Ok(format!("pub enum {}{} {{\n{}}}\n", t.name, generics_str, variants_str))
            }
            _ => Ok(format!("// Type: {}", t.name)),
        }
    }

    pub fn lookup_type_def(&self, name: &str) -> Option<Type> {
        self.type_defs.get(name).map(|t| t.type_.clone())
    }

    pub fn infer_expr_type(&self, expr: &Expr) -> Option<Type> {
        TypeInfer::infer_expr_type(self, expr)
    }

    pub fn for_init_to_rust(&self, init: &Option<ForInit>) -> String {
        CodeGenStmt::for_init_to_rust(self, init)
    }

    pub fn pat_to_rust(&self, pat: &Pat, source_name: &str) -> Vec<String> {
        PatGen::pat_to_rust(self, pat, source_name)
    }

    pub fn generate_new_response(&self, args: &[Expr]) -> String {
        ResponseGen::generate_new_response(self, args)
    }

    pub fn is_string_literal_union(&self, types: &[Type]) -> bool {
        types.iter().all(|t| matches!(t, Type::String))
    }

    pub fn generate_enum(&self, name: &str, generics_str: String, types: &[Type]) -> String {
        let variants: Vec<String> = types.iter().enumerate().map(|(i, t)| {
            let v_name = format!("{}{}", name.chars().next().unwrap().to_uppercase(), &name[1..]);
            match t {
                Type::String => format!("    {}{}(String),", v_name, i),
                _ => format!("    {}{}({}),", v_name, i, self.type_to_rust(t)),
            }
        }).collect();
        let variants_str = variants.join("\n");
        format!("pub enum {}<{}> {{\n{}}}\n", name, generics_str, variants_str)
    }

    pub fn jsx_attr_to_rust(&self, name: &str) -> String {
        match name {
            "class" | "className" => "class_name".to_string(),
            "for" | "htmlFor" => "for_id".to_string(),
            "tabindex" => "tab_index".to_string(),
            _ if name.starts_with("on") && name.len() > 2 && name.chars().nth(2).map(|c| c.is_uppercase()).unwrap_or(false) => {
                let event_part = &name[2..];
                format!("on_{}", self.to_snake_case(event_part))
            }
            _ => {
                if name.contains('-') {
                    self.to_snake_case(name)
                } else {
                    name.to_string()
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snake_case() {
        let cg = CodeGenerator::new();
        assert_eq!(cg.to_snake_case("HelloWorld"), "hello_world");
        assert_eq!(cg.to_snake_case("hello-world"), "hello_world");
    }
}
