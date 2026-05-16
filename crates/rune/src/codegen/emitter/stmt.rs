//! # Statement Transpiler
//!
//! Transpiles TypeScript statements to Rust statements.

use super::{RustEmitter, ExprTranspiler};

/// Extract substring until a closing character.
#[must_use]
pub fn rest_until(s: &str, start: usize, end: char) -> &str {
    if let Some(end_pos) = s[start..].find(end) {
        &s[start..start + end_pos]
    } else {
        &s[start..]
    }
}

/// Transpiles statements.
pub struct StmtTranspiler;

impl StmtTranspiler {
    /// Translate a TypeScript line to Rust.
    #[allow(clippy::too_many_lines)]
    pub fn translate_line(emitter: &mut RustEmitter, line: &str) {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            emitter.push_line("");
            return;
        }

        // Variable declarations
        if trimmed.starts_with("const ") || trimmed.starts_with("let ") {
            Self::translate_binding(emitter, trimmed);
            return;
        }

        // Return statements
        if let Some(expr) = trimmed.strip_prefix("return ") {
            let expr = expr.trim_end_matches(';');
            emitter.push_indent();
            let translated = ExprTranspiler::new(emitter).transpile(expr);
            emitter.push_line(&format!("return {translated};"));
            return;
        }

        // If statements
        if trimmed.starts_with("if ") {
            Self::translate_if(emitter, trimmed);
            return;
        }

        // For loops
        if trimmed.starts_with("for ") {
            Self::translate_for(emitter, trimmed);
            return;
        }

        // Switch statements
        if trimmed.starts_with("switch ") {
            Self::translate_switch(emitter, trimmed);
            return;
        }

        // While loops
        if trimmed.starts_with("while ") {
            Self::translate_while(emitter, trimmed);
            return;
        }

        // Default: expression statements
        emitter.push_indent();
        emitter.push_line(&format!(
            "{};",
            ExprTranspiler::new(emitter).transpile(trimmed.trim_end_matches(';'))
        ));
    }

    /// Translate a variable binding.
    pub fn translate_binding(emitter: &mut RustEmitter, line: &str) {
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
                emitter.push_indent();
                let translated = ExprTranspiler::new(emitter).transpile(value);
                emitter.push_line(&format!(
                    "{keyword} {name}: {} = {translated};",
                    Self::parse_type_hint(type_hint)
                ));
            } else {
                let name = name_type.trim();
                emitter.push_indent();
                let translated = ExprTranspiler::new(emitter).transpile(value);
                emitter.push_line(&format!("{keyword} {name} = {translated};"));
            }
        }
    }

    /// Translate an if statement.
    pub fn translate_if(emitter: &mut RustEmitter, line: &str) {
        let rest = line.strip_prefix("if ").unwrap();

        if let Some(brace_pos) = rest.find('{') {
            let condition = rest[..brace_pos].trim();
            let translated = ExprTranspiler::new(emitter).transpile_condition(condition);
            emitter.push_indent();
            emitter.push_line(&format!("if {translated} {{"));
            emitter.indent += 1;
        }
    }

    /// Translate a for loop.
    pub fn translate_for(emitter: &mut RustEmitter, line: &str) {
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

                emitter.push_indent();
                let translated = ExprTranspiler::new(emitter).transpile(array);
                emitter.push_line(&format!("for {var_name} in {translated} {{"));
                emitter.indent += 1;
            }
        } else if line.contains(" = ") {
            // C-style for loop
            let rest = line.strip_prefix("for ").unwrap()
                .trim_start_matches('(')
                .trim_end_matches(')');
            emitter.push_indent();
            let translated = ExprTranspiler::new(emitter).transpile(rest.trim_end_matches(';'));
            emitter.push_line(&format!("for {translated} {{"));
            emitter.indent += 1;
        }
    }

    /// Translate a switch statement.
    pub fn translate_switch(emitter: &mut RustEmitter, line: &str) {
        let rest = line.strip_prefix("switch ").unwrap();

        if let Some(brace_pos) = rest.find('{') {
            let expr = rest[..brace_pos].trim();
            let translated = ExprTranspiler::new(emitter).transpile(expr);
            emitter.push_indent();
            emitter.push_line(&format!("match {translated} {{"));
            emitter.indent += 1;
        }
    }

    /// Translate a while loop.
    pub fn translate_while(emitter: &mut RustEmitter, line: &str) {
        let rest = line.strip_prefix("while ").unwrap();

        if let Some(brace_pos) = rest.find('{') {
            let condition = rest[..brace_pos].trim();
            let translated = ExprTranspiler::new(emitter).transpile_condition(condition);
            emitter.push_indent();
            emitter.push_line(&format!("while {translated} {{"));
            emitter.indent += 1;
        }
    }

    /// Parse a type hint string.
    #[must_use]
    pub fn parse_type_hint(hint: &str) -> String {
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

    /// Generate function body from source.
    #[allow(clippy::too_many_lines)]
    pub fn generate_function_body(emitter: &mut RustEmitter, name: &str) {
        let source = &emitter.source.source;

        for line in source.lines() {
            let trimmed = line.trim();

            // Skip empty lines and comments
            if trimmed.is_empty() || trimmed.starts_with("//") {
                emitter.push_line("");
                continue;
            }

            // Find function body
            if trimmed.contains("function ") && trimmed.contains(name) {
                let body_start = trimmed.find('{').or_else(|| trimmed.find("=>"));
                if let Some(start) = body_start {
                    let body = &trimmed[start + 1..];
                    if body.contains('}') {
                        let cleaned = body.trim_end_matches('}').trim_start_matches('{').trim();
                        Self::translate_line(emitter, &format!("    {cleaned}"));
                    }
                }
            }
        }
    }
}
