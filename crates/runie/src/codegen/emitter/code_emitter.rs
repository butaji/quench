//! # Code Emitter
//!
//! Emits Rust code from AST nodes.

use super::types::{EnumDefinition, RustType, StructFields};
use crate::codegen::emitter::utils::{to_pascal_case, to_snake_case};
use swc_ecma_ast::Stmt;

/// A warning generated during code emission.
#[derive(Debug, Clone)]
pub struct EmitterWarning {
    /// Warning code identifier
    pub code: &'static str,
    /// Warning message
    pub message: String,
}

/// Type registry for distinguishing built-in types from user-defined structs.
#[derive(Debug, Clone, Default)]
pub struct TypeRegistry {
    /// User-defined type names (interfaces, type aliases, enums)
    user_types: std::collections::HashSet<String>,
}

impl TypeRegistry {
    /// Check if a type name is a known built-in Rust type.
    #[must_use]
    pub fn is_builtin_type(&self, name: &str) -> bool {
        matches!(
            name,
            "String" | "str" | "bool" | "f64" | "f32" | "i32" | "i64" | "u32" | "u64"
                | "usize" | "isize" | "char" | "()" | "Vec" | "Option" | "Result"
                | "Box" | "Rc" | "Arc" | "HashMap" | "HashSet" | "Cow"
        ) || name.starts_with("Vec<")
            || name.starts_with("HashMap<")
            || name.starts_with("Option<")
    }

    /// Check if a type name is a known user-defined type.
    #[must_use]
    pub fn is_user_defined_type(&self, name: &str) -> bool {
        !self.is_builtin_type(name) && self.user_types.contains(name)
    }

    /// Register a user-defined type.
    pub fn register_user_type(&mut self, name: &str) {
        self.user_types.insert(name.to_string());
    }
}

/// Emits Rust code for types and functions.
pub struct CodeEmitter {
    /// Output buffer with pre-allocated capacity
    output: String,
    /// Current indentation level
    indent: usize,
    /// Expected return type for the current function (for type inference)
    expected_return: Option<String>,
    /// Struct name prefix for the current object literal context
    object_struct_name: Option<String>,
    /// Warnings collected during emission
    warnings: Vec<EmitterWarning>,
    /// Type registry for distinguishing built-in vs user-defined types
    type_registry: TypeRegistry,
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
            // Pre-allocate for typical file sizes (reduces heap allocations)
            output: String::with_capacity(4096),
            indent: 0,
            expected_return: None,
            object_struct_name: None,
            warnings: Vec::new(),
            type_registry: TypeRegistry::default(),
        }
    }

    /// Get a reference to the type registry.
    #[must_use]
    pub fn type_registry(&self) -> &TypeRegistry {
        &self.type_registry
    }

    /// Get a mutable reference to the type registry.
    pub fn type_registry_mut(&mut self) -> &mut TypeRegistry {
        &mut self.type_registry
    }

    /// Emit a warning.
    pub fn emit_warning(&mut self, code: &'static str, message: &str) {
        self.warnings.push(EmitterWarning {
            code,
            message: message.to_string(),
        });
    }

    /// Get all warnings.
    #[must_use]
    pub fn warnings(&self) -> &[EmitterWarning] {
        &self.warnings
    }

    /// Take all warnings.
    #[must_use]
    pub fn take_warnings(&mut self) -> Vec<EmitterWarning> {
        std::mem::take(&mut self.warnings)
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
            self.push_indent();
            let rust_field = to_snake_case(field_name);
            self.push_line(&format!("pub {rust_field}: {field_type},"));
        }
        self.indent -= 1;
        self.push_indent();
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
            self.push_indent();
            if variant.fields.is_empty() {
                self.push_line(&format!("{},", to_pascal_case(&variant.name)));
            } else {
                let field_strs: Vec<String> = variant
                    .fields
                    .iter()
                    .map(|(n, t)| format!("{}: {t}", to_snake_case(n)))
                    .collect();
                self.push_line(&format!(
                    "{} {{ {} }},",
                    to_pascal_case(&variant.name),
                    field_strs.join(", ")
                ));
            }
        }
        self.indent -= 1;
        self.push_indent();
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
        self.push_str(&format!(
            "pub {async_prefix}fn {rust_name}({}) -> {return_type} {{\n",
            params_str.join(", ")
        ));
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

    /// Get mutable reference to output buffer.
    pub fn output_mut(&mut self) -> &mut String {
        &mut self.output
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
