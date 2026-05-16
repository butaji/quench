//! # Code Emitter
//!
//! Emits Rust code from AST nodes.

use super::types::{StructFields, EnumDefinition, RustType, to_snake_case, to_pascal_case};
use swc_ecma_ast::Stmt;

/// Emits Rust code for types and functions.
pub struct CodeEmitter {
    /// Output buffer
    output: String,
    /// Current indentation level
    indent: usize,
    /// Expected return type for the current function (for type inference)
    expected_return: Option<String>,
    /// Struct name prefix for the current object literal context
    object_struct_name: Option<String>,
}

impl CodeEmitter {
    /// Set the expected return type for type inference.
    pub fn set_expected_return(&mut self, ty: Option<String>) {
        self.expected_return = ty;
    }

    /// Get the expected return type.
    #[must_use]
    pub fn expected_return(&self) -> Option<&String> {
        self.expected_return.as_ref()
    }

    /// Set the struct name context for object literals.
    pub fn set_object_struct(&mut self, name: Option<String>) {
        self.object_struct_name = name;
    }

    /// Get the current struct name context.
    #[must_use]
    pub fn object_struct_name(&self) -> Option<&String> {
        self.object_struct_name.as_ref()
    }
}

impl CodeEmitter {
    /// Create a new code emitter.
    #[must_use]
    pub fn new() -> Self {
        Self {
            output: String::new(),
            indent: 0,
            expected_return: None,
            object_struct_name: None,
        }
    }

    /// Emit a struct definition.
    pub fn emit_struct(&mut self, name: &str, fields: &StructFields) {
        let struct_name = if name.starts_with("__") {
            name.to_string()
        } else {
            to_pascal_case(name)
        };
        self.push_line("#[derive(Debug, Clone)]");
        self.push_line(&format!("pub struct {struct_name} {{"));
        self.indent += 1;
        for (field_name, field_type) in fields {
            let rust_field = to_snake_case(field_name);
            self.push_line(&format!("pub {rust_field}: {field_type},"));
        }
        self.indent -= 1;
        self.push_line("}");
        self.push_line("");
    }

    /// Emit an enum definition.
    pub fn emit_enum(&mut self, ed: &EnumDefinition) {
        let pascal_name = to_pascal_case(&ed.name);
        self.push_line("#[derive(Debug, Clone, Copy, PartialEq)]");
        self.push_line(&format!("pub enum {pascal_name} {{"));
        self.indent += 1;
        for variant in &ed.variants {
            if variant.fields.is_empty() {
                self.push_line(&format!("{},", to_pascal_case(&variant.name)));
            } else {
                let field_strs: Vec<String> = variant
                    .fields
                    .iter()
                    .map(|(n, t)| format!("{}: {t}", to_snake_case(n)))
                    .collect();
                self.push_line(&format!("{} {{ {} }},", to_pascal_case(&variant.name), field_strs.join(", ")));
            }
        }
        self.indent -= 1;
        self.push_line("}");
        self.push_line("");
    }

    /// Emit a function with a body.
    pub fn emit_function_with_body(
        &mut self,
        rust_name: &str,
        params: &[(String, RustType)],
        return_type: &RustType,
        is_async: bool,
        body: Option<Stmt>,
    ) {
        let async_prefix = if is_async { "async " } else { "" };
        let params_str: Vec<String> = params
            .iter()
            .map(|(n, t)| format!("{}: {t}", to_snake_case(n)))
            .collect();
        self.push_indent();
        self.push_str(&format!("pub {async_prefix}fn {rust_name}({}) -> {return_type} {{\n", params_str.join(", ")));
        self.indent += 1;
        // Set expected return type for type inference in the body
        let prev_return = self.expected_return.clone();
        self.expected_return = Some(return_type.to_string());
        if let Some(body_stmt) = body {
            super::emit_body_stmt(self, &body_stmt);
        } else {
            self.push_indent();
            self.push_line("()");
        }
        // Restore previous context
        self.expected_return = prev_return;
        self.indent -= 1;
        self.push_indent();
        self.push_line("}");
        self.push_line("");
    }

    /// Emit a function (no body - returns unit).
    pub fn emit_function(
        &mut self,
        rust_name: &str,
        params: &[(String, RustType)],
        return_type: &RustType,
        is_async: bool,
    ) {
        self.emit_function_with_body(rust_name, params, return_type, is_async, None);
    }

    /// Push a line with newline.
    pub fn push_line(&mut self, s: &str) {
        self.output.push_str(s);
        self.output.push('\n');
    }

    /// Push a string without newline.
    pub fn push_str(&mut self, s: &str) {
        self.output.push_str(s);
    }

    /// Push indentation.
    pub fn push_indent(&mut self) {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
    }

    /// Increment indentation level.
    pub fn inc_indent(&mut self) {
        self.indent += 1;
    }

    /// Decrement indentation level.
    pub fn dec_indent(&mut self) {
        self.indent = self.indent.saturating_sub(1);
    }

    /// Get the output.
    #[must_use]
    pub fn output(&self) -> &str {
        &self.output
    }

    /// Consume emitter and return output.
    #[must_use]
    pub fn into_output(self) -> String {
        self.output
    }
}

impl Default for CodeEmitter {
    fn default() -> Self {
        Self::new()
    }
}
