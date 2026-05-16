//! # Module Writer
//!
//! Writes Rust module structure.

use super::RustEmitter;

/// Write module header with imports.
pub fn write_header(emitter: &mut RustEmitter) {
    emitter.push_line("//! Generated from Rune source");
    emitter.push_line("");
    emitter.push_line("use std::collections::HashMap;");
    emitter.push_line("use std::fmt::{self, Write};");
    emitter.push_line("");

    // Add protocol imports for app crate
    if emitter.source.path.to_string_lossy().contains("/app/src/") {
        emitter.push_line("use protocol::{AppState, Filter, Task};");
        emitter.push_line("");
    }
}

/// Write type definitions.
pub fn write_types(emitter: &mut RustEmitter) {
    for (_, info) in emitter.analysis.types.iter() {
        match info {
            crate::analyzer::TypeInfo::Struct(s) => {
                emitter.push_line("");
                emitter.push_line(&s.to_rust());
                emitter.push_line("");
            }
            crate::analyzer::TypeInfo::Enum(e) => {
                emitter.push_line("");
                emitter.push_line(&e.to_rust());
                emitter.push_line("");
            }
            _ => {}
        }
    }
}

/// Write function definitions.
pub fn write_functions(emitter: &mut RustEmitter) {
    for (name, info) in emitter.analysis.types.iter() {
        if let crate::analyzer::TypeInfo::Function(func) = info {
            emit_function(emitter, name, func);
        }
    }
}

/// Emit a single function definition.
pub fn emit_function(
    emitter: &mut RustEmitter,
    name: &str,
    func: &crate::analyzer::FunctionInfo,
) {
    use super::to_snake_case;
    use super::stmt::StmtTranspiler;

    let rust_name = to_snake_case(name);
    let async_prefix = if func.is_async { "async " } else { "" };

    // Build parameter list
    let params: Vec<String> = func.params.iter()
        .map(|(n, t)| format!("{}: {}", to_snake_case(n), t.to_rust_type()))
        .collect();
    let params_str = params.join(", ");

    // Build return type
    let return_type = func.return_type.to_rust_type();

    emitter.push_line("");
    emitter.push_line(&format!("/// Function: {name}"));

    if func.is_async {
        emitter.push_line(&format!(
            "{async_prefix}pub fn {rust_name}({params_str}) -> impl Future<Output = {return_type}> + '_ {{"
        ));
    } else {
        emitter.push_line(&format!("pub fn {rust_name}({params_str}) -> {return_type} {{"));
    }

    emitter.indent += 1;

    // Generate function body based on the source
    StmtTranspiler::generate_function_body(emitter, name);

    emitter.indent -= 1;
    emitter.push_line("}");
    emitter.push_line("");
}
