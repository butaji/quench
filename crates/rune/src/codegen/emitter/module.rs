//! # Module Emitter
//!
//! Emits module-level constructs (types, functions, imports).

use super::RustEmitter;
use crate::analyzer::TypeInfo;

/// Write type definitions.
pub fn write_types(emitter: &mut RustEmitter) {
    // Collect types first to avoid borrow conflict
    let types: Vec<(String, TypeInfo)> = emitter.analysis.types.iter()
        .map(|(name, info)| (name.to_string(), info.clone()))
        .collect();

    for (_, info) in types {
        match info {
            TypeInfo::Struct(s) => {
                emitter.push_line(&s.to_rust());
                emitter.push_line("");
            }
            TypeInfo::Enum(e) => {
                emitter.push_line(&e.to_rust());
                emitter.push_line("");
            }
            _ => {}
        }
    }
}
