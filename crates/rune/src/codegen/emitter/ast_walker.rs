//! # AST Walker
//!
//! Walks SWC AST and emits Rust code.

use swc_ecma_ast::{Decl, ExportDecl, FnDecl, Module, ModuleDecl, ModuleItem, Stmt};

/// Walks the AST and emits Rust code.
pub struct AstWalker {
    // Output buffer for accumulating code
    output: String,
    // Current indentation level
    indent: usize,
}

impl AstWalker {
    /// Create a new AST walker.
    pub fn new() -> Self {
        Self {
            output: String::new(),
            indent: 0,
        }
    }

    /// Walk a module and emit Rust code.
    pub fn walk_module(&mut self, module: &Module) {
        self.push_line("use serde_json::Value;");
        self.push_line("");

        for item in &module.body {
            match item {
                ModuleItem::Stmt(Stmt::Decl(Decl::Fn(fn_decl))) => {
                    self.emit_function_decl(fn_decl);
                }
                ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(ExportDecl {
                    decl: Decl::Fn(fn_decl), ..
                })) => {
                    self.emit_function_decl(fn_decl);
                }
                _ => {}
            }
        }
    }

    fn emit_function_decl(&mut self, fn_decl: &FnDecl) {
        let fn_name = fn_decl.ident.sym.to_string();
        let rust_name = to_snake_case(&fn_name);

        self.push_indent();
        self.push_str("pub fn ");
        self.push_str(&rust_name);
        self.push_str("(");
        self.push_str(") -> Value {\n");
        self.indent += 1;
        self.push_indent();
        self.push_line("serde_json::json!(())");
        self.indent -= 1;
        self.push_line("}\n");
    }

    fn push_line(&mut self, s: &str) {
        self.output.push_str(s);
        self.output.push('\n');
    }

    fn push_str(&mut self, s: &str) {
        self.output.push_str(s);
    }

    fn push_indent(&mut self) {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
    }

    /// Get the generated output.
    pub fn into_output(self) -> String {
        self.output
    }
}

impl Default for AstWalker {
    fn default() -> Self {
        Self::new()
    }
}

fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('_');
        }
        result.push(c.to_ascii_lowercase());
    }
    result
}
