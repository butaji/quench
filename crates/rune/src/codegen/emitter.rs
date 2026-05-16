//! # Rust Emitter
//!
//! Core transpilation from TypeScript to Rust source.

use crate::{parser::SourceFile, analyzer::AnalysisResult};
use crate::codegen::{GeneratedModule, Import, CodegenOptions};

/// Options for code emission.
#[derive(Debug, Clone, Default)]
pub struct EmitOptions {
    /// Generate debug info
    pub source_map: bool,
    /// Pretty print output
    pub pretty: bool,
}

/// Emits Rust code from TypeScript source.
#[derive(Debug)]
pub struct RustEmitter<'a> {
    /// Source file being transpiled
    source: &'a SourceFile,
    /// Analysis results
    analysis: &'a AnalysisResult,
    /// Imports needed by this module
    imports: Vec<Import>,
    /// Output buffer
    output: String,
    /// Current indentation level
    indent: usize,
    /// Generation options
    #[allow(unused)]
    options: CodegenOptions,
}

impl<'a> RustEmitter<'a> {
    /// Create a new emitter.
    #[must_use]
    pub fn new(source: &'a SourceFile, analysis: &'a AnalysisResult) -> Self {
        Self {
            source,
            analysis,
            imports: Vec::new(),
            output: String::new(),
            indent: 0,
            options: CodegenOptions::default(),
        }
    }

    /// Emit the complete module.
    ///
    /// # Errors
    /// Returns an error if code generation fails.
    pub fn emit(mut self) -> crate::Result<GeneratedModule> {
        self.write_header();
        self.write_types();
        self.write_functions();
        self.write_footer();

        let name = self.source.path.file_stem()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("module")
            .to_string();

        Ok(GeneratedModule {
            name,
            source: self.output,
            imports: self.imports,
            types: Vec::new(),
            functions: Vec::new(),
        })
    }

    /// Write module header with imports.
    fn write_header(&mut self) {
        self.push_line("//! Generated from Rune source");
        self.push_line("");
        self.push_line("use std::collections::HashMap;");
        self.push_line("use std::fmt::{self, Write};");
        self.push_line("");

        // Add protocol imports for app crate
        if self.source.path.to_string_lossy().contains("/app/src/") {
            self.push_line("use protocol::{AppState, Filter, Task};");
            self.push_line("");
        }
    }

    /// Write type definitions.
    fn write_types(&mut self) {
        for (_, info) in self.analysis.types.iter() {
            match info {
                crate::analyzer::TypeInfo::Struct(s) => {
                    self.push_line("");
                    self.push_line(&s.to_rust());
                    self.push_line("");
                }
                crate::analyzer::TypeInfo::Enum(e) => {
                    self.push_line("");
                    self.push_line(&e.to_rust());
                    self.push_line("");
                }
                _ => {}
            }
        }
    }

    /// Write function definitions.
    fn write_functions(&mut self) {
        for (name, info) in self.analysis.types.iter() {
            if let crate::analyzer::TypeInfo::Function(func) = info {
                self.emit_function(name, func);
            }
        }
    }

    /// Emit a single function definition.
    fn emit_function(&mut self, name: &str, func: &crate::analyzer::FunctionInfo) {
        let rust_name = to_snake_case(name);
        let async_prefix = if func.is_async { "async " } else { "" };

        // Build parameter list
        let params: Vec<String> = func.params.iter()
            .map(|(n, t)| format!("{}: {}", to_snake_case(n), t.to_rust_type()))
            .collect();
        let params_str = params.join(", ");

        // Build return type
        let return_type = func.return_type.to_rust_type();

        self.push_line("");
        self.push_line(&format!("/// Function: {name}"));

        if func.is_async {
            self.push_line(&format!(
                "{async_prefix}pub fn {rust_name}({params_str}) -> impl Future<Output = {return_type}> + '_ {{"
            ));
        } else {
            self.push_line(&format!("pub fn {rust_name}({params_str}) -> {return_type} {{"));
        }

        self.indent += 1;

        // Generate function body based on the source
        self.generate_function_body(name);

        self.indent -= 1;
        self.push_line("}");
        self.push_line("");
    }

    /// Generate function body from source.
    #[allow(clippy::too_many_lines)]
    fn generate_function_body(&mut self, _name: &str) {
        // Find the function in the source text
        let source = &self.source.source;

        for line in source.lines() {
            let trimmed = line.trim();

            // Skip empty lines and comments
            if trimmed.is_empty() || trimmed.starts_with("//") {
                self.push_line("");
                continue;
            }

            // Find function body
            if trimmed.contains("function ") && trimmed.contains(_name) {
                let body_start = trimmed.find('{').or_else(|| trimmed.find("=>"));
                if let Some(start) = body_start {
                    let body = &trimmed[start + 1..];
                    if body.contains('}') {
                        let cleaned = body.trim_end_matches('}').trim_start_matches('{').trim();
                        self.translate_line(&format!("    {cleaned}"));
                    }
                }
            }
        }
    }

    /// Translate a TypeScript line to Rust.
    fn translate_line(&mut self, line: &str) {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            self.push_line("");
            return;
        }

        // Variable declarations
        if trimmed.starts_with("const ") || trimmed.starts_with("let ") {
            self.translate_binding(trimmed);
            return;
        }

        // Return statements
        if let Some(expr) = trimmed.strip_prefix("return ") {
            let expr = expr.trim_end_matches(';');
            self.push_indent();
            self.push_line(&format!("return {};", self.translate_expr(expr)));
            return;
        }

        // If statements
        if trimmed.starts_with("if ") {
            self.translate_if(trimmed);
            return;
        }

        // For loops
        if trimmed.starts_with("for ") {
            self.translate_for(trimmed);
            return;
        }

        // Switch statements
        if trimmed.starts_with("switch ") {
            self.translate_switch(trimmed);
            return;
        }

        // While loops
        if trimmed.starts_with("while ") {
            self.translate_while(trimmed);
            return;
        }

        // Default: function calls in expression context
        self.push_indent();
        self.push_line(&format!(
            "{};",
            self.translate_expr(trimmed.trim_end_matches(';'))
        ));
    }

    /// Translate a variable binding.
    fn translate_binding(&mut self, line: &str) {
        let (keyword, rest) = if let Some(r) = line.strip_prefix("const ") {
            ("let", r)
        } else {
            ("let mut", line.strip_prefix("let ").unwrap_or(line))
        };

        let rest = rest.trim_end_matches(';');

        if let Some(eq_pos) = rest.find(" = ") {
            let name_type = &rest[..eq_pos];
            let value = &rest[eq_pos + 3..];

            if let Some(colon_pos) = name_type.find(": ") {
                let name = name_type[..colon_pos].trim();
                let type_hint = name_type[colon_pos + 2..].trim();
                self.push_indent();
                self.push_line(&format!(
                    "{keyword} {name}: {} = {};",
                    self.parse_type_hint(type_hint),
                    self.translate_expr(value)
                ));
            } else {
                let name = name_type.trim();
                self.push_indent();
                self.push_line(&format!(
                    "{keyword} {name} = {};",
                    self.translate_expr(value)
                ));
            }
        }
    }

    /// Translate an if statement.
    fn translate_if(&mut self, line: &str) {
        let rest = line.strip_prefix("if ").unwrap();

        if let Some(brace_pos) = rest.find('{') {
            let condition = rest[..brace_pos].trim();
            self.push_indent();
            self.push_line(&format!("if {} {{", self.translate_condition(condition)));
            self.indent += 1;
        }
    }

    /// Translate a for loop.
    fn translate_for(&mut self, line: &str) {
        if line.contains(" of ") {
            // Iterator-based for loop
            if let Some(of_pos) = line.find(" of ") {
                let binding = &line[5..of_pos].trim();
                let array = rest_until(line, of_pos + 4, ')');
                let var_name = binding
                    .replace("const ", "")
                    .replace("let ", "")
                    .trim()
                    .to_string();

                self.push_indent();
                self.push_line(&format!("for {var_name} in {} {{", self.translate_expr(array)));
                self.indent += 1;
            }
        } else if line.contains(" = ") {
            // C-style for loop
            let rest = line.strip_prefix("for ").unwrap().trim_start_matches('(').trim_end_matches(')');
            self.push_indent();
            self.push_line(&format!(
                "for {} {{",
                self.translate_expr(rest.trim_end_matches(';'))
            ));
            self.indent += 1;
        }
    }

    /// Translate a switch statement.
    fn translate_switch(&mut self, line: &str) {
        let rest = line.strip_prefix("switch ").unwrap();

        if let Some(brace_pos) = rest.find('{') {
            let expr = rest[..brace_pos].trim();
            self.push_indent();
            self.push_line(&format!("match {} {{", self.translate_expr(expr)));
            self.indent += 1;
        }
    }

    /// Translate a while loop.
    fn translate_while(&mut self, line: &str) {
        let rest = line.strip_prefix("while ").unwrap();

        if let Some(brace_pos) = rest.find('{') {
            let condition = rest[..brace_pos].trim();
            self.push_indent();
            self.push_line(&format!("while {} {{", self.translate_condition(condition)));
            self.indent += 1;
        }
    }

    /// Translate an expression.
    #[allow(clippy::too_many_lines)]
    fn translate_expr(&self, expr: &str) -> String {
        let expr = expr.trim();

        // String literals - keep as-is
        if expr.starts_with('"') || expr.starts_with('\'') {
            return expr.to_string();
        }

        // Number literals - keep as-is
        if expr.parse::<f64>().is_ok() {
            return expr.to_string();
        }

        // Boolean literals
        if expr == "true" || expr == "false" {
            return expr.to_string();
        }

        // Ternary operator
        if expr.contains(" ? ") {
            return self.translate_ternary(expr);
        }

        // Binary operations
        if let Some(op) = Self::find_binary_op(expr) {
            return self.translate_binary_op(expr, op);
        }

        // Function calls
        if expr.contains('(') {
            return self.translate_call(expr);
        }

        // Property access
        if expr.contains('.') && !expr.contains('(') {
            return Self::translate_property_access(expr);
        }

        // Array literals
        if expr.starts_with('[') {
            return self.translate_array_literal(expr);
        }

        // Object literals
        if expr.starts_with('{') {
            return self.translate_object_literal(expr);
        }

        // Simple identifier
        to_snake_case(expr)
    }

    /// Translate a ternary expression.
    fn translate_ternary(&self, expr: &str) -> String {
        // Find the ? and : positions
        let question_pos = expr.find(" ? ")
            .unwrap_or_else(|| expr.find("?(").unwrap_or(0));
        let colon_pos = expr.rfind(" : ")
            .unwrap_or_else(|| expr.rfind(":(").unwrap_or(0));

        if colon_pos > question_pos {
            let condition = self.translate_condition(&expr[..question_pos]);
            let then_expr = expr[question_pos + 3..colon_pos].trim();
            let else_expr = expr[colon_pos + 3..]
                .trim_end_matches(';')
                .trim_end_matches(')');

            return format!(
                "if {} {{ {} }} else {{ {} }}",
                condition,
                self.translate_expr(then_expr),
                self.translate_expr(else_expr)
            );
        }

        expr.to_string()
    }

    /// Find binary operator in expression.
    fn find_binary_op(expr: &str) -> Option<&'static str> {
        const OPS: &[&str] = &[
            "+=", "-=", "*=", "/=", "===", "!==", "==", "!=",
            "<=", ">=", "&&", "||", "+", "-", "*", "/", "%",
            ">", "<", "|", "&", "^",
        ];

        OPS.iter().find(|&&op| expr.contains(op)).copied()
    }

    /// Translate a binary operation.
    fn translate_binary_op(&self, expr: &str, op: &str) -> String {
        let parts: Vec<&str> = expr.split(op).collect();

        if parts.len() == 2 {
            let left = parts[0].trim();
            let right = parts[1].trim();

            let rust_op = match op {
                "===" | "==" => "==",
                "!==" | "!=" => "!=",
                "&&" => "&&",
                "||" => "||",
                "+" | "-" | "*" | "/" | "%" | "|" | "&" | "^" => op,
                "<" | ">" | "<=" | ">=" => op,
                _ => op,
            };

            return format!(
                "({} {rust_op} {})",
                self.translate_expr(left),
                self.translate_expr(right)
            );
        }

        expr.to_string()
    }

    /// Translate a function call.
    fn translate_call(&self, expr: &str) -> String {
        if let Some(paren_pos) = expr.find('(') {
            let func = &expr[..paren_pos];
            let args = expr[paren_pos + 1..]
                .trim_end_matches(')')
                .trim_end_matches(';');

            let rust_func = to_snake_case(func);
            let translated_args: Vec<String> = args.split(',')
                .map(|a| self.translate_expr(a.trim()))
                .collect();

            return format!("{rust_func}({})", translated_args.join(", "));
        }

        to_snake_case(expr)
    }

    /// Translate property access.
    fn translate_property_access(expr: &str) -> String {
        expr.split('.')
            .map(to_snake_case)
            .collect::<Vec<_>>()
            .join(".")
    }

    /// Translate an array literal.
    fn translate_array_literal(&self, expr: &str) -> String {
        let inner = expr.trim_start_matches('[').trim_end_matches(']');
        let elements: Vec<String> = inner
            .split(',')
            .map(|e| self.translate_expr(e.trim()))
            .collect();

        format!("vec![{}]", elements.join(", "))
    }

    /// Translate an object literal.
    fn translate_object_literal(&self, expr: &str) -> String {
        let inner = expr.trim_start_matches('{').trim_end_matches('}');
        let fields: Vec<String> = inner
            .split(',')
            .filter_map(|f| {
                let f = f.trim();
                if f.is_empty() {
                    return None;
                }
                let parts: Vec<&str> = f.split(':').collect();
                if parts.len() == 2 {
                    let key = parts[0].trim();
                    let value = self.translate_expr(parts[1].trim());
                    return Some(format!("{key}: {value}"));
                }
                Some(f.to_string())
            })
            .collect();

        format!("{{ {} }}", fields.join(", "))
    }

    /// Translate a condition.
    fn translate_condition(&self, cond: &str) -> String {
        let cond = cond.trim();

        // Negation
        if let Some(inner) = cond.strip_prefix('!') {
            return format!("!{}", self.translate_condition(inner.trim()));
        }

        // Parentheses
        if cond.starts_with('(') && cond.ends_with(')') {
            let inner = &cond[1..cond.len() - 1];
            return format!("({})", self.translate_condition(inner));
        }

        self.translate_expr(cond)
    }

    /// Write module footer.
    fn write_footer(&mut self) {
        self.push_line("// End of generated code");
    }

    /// Parse a type hint string.
    #[allow(clippy::unused_self)]
    fn parse_type_hint(&self, hint: &str) -> String {
        match hint {
            "number" => "f64".to_string(),
            "string" => "String".to_string(),
            "boolean" => "bool".to_string(),
            "i32" | "integer" => "i32".to_string(),
            "usize" => "usize".to_string(),
            "void" | "undefined" => "()".to_string(),
            _ => hint.to_string(),
        }
    }

    fn push_line(&mut self, s: &str) {
        self.output.push_str(s);
        self.output.push('\n');
    }

    fn push_indent(&mut self) {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
    }
}

/// Extract substring until a closing character.
fn rest_until(s: &str, start: usize, end: char) -> &str {
    if let Some(end_pos) = s[start..].find(end) {
        &s[start..start + end_pos]
    } else {
        &s[start..]
    }
}

/// Convert camelCase/snake_case to snake_case.
#[must_use]
pub fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();

    for (i, c) in chars.iter().enumerate() {
        if c.is_uppercase() {
            if i > 0 && !chars[i - 1].is_uppercase() {
                result.push('_');
            }
            result.push(c.to_ascii_lowercase());
        } else if *c == '-' {
            result.push('_');
        } else {
            result.push(*c);
        }
    }

    // Handle consecutive uppercase (e.g., "URLParser" -> "url_parser")
    let mut final_result = String::new();
    for (i, c) in result.chars().enumerate() {
        if i > 0 && i < result.len() - 1 {
            let prev = result.chars().nth(i - 1);
            let next = result.chars().nth(i + 1);
            if c.is_uppercase() && prev.is_some_and(|p| !p.is_uppercase()) {
                final_result.push('_');
            }
            if c.is_uppercase() && next.is_some_and(|n| n.is_uppercase()) {
                // keep as is
            } else if c.is_uppercase() {
                final_result.push('_');
            }
        }
        final_result.push(c.to_ascii_lowercase());
    }

    final_result.trim_matches('_').to_string()
}
