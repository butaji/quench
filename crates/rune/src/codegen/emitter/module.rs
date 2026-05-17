//! # Module Emitter
//!
//! Emits module-level constructs (types, functions, imports).

use super::RustEmitter;
use crate::analyzer::TypeInfo;

/// Emit module-level code (types, functions, imports).
#[allow(clippy::needless_pass_by_ref_mut)]
pub fn emit_module(_emitter: &mut RustEmitter, _source: &crate::parser::SourceFile) {
    // Module header is already written by RustEmitter::emit
    // This function handles any additional module-level processing
    // Types are emitted during AST walking
}

/// Write type definitions.
#[allow(dead_code)]
pub fn write_types(emitter: &mut RustEmitter, types: &[(String, TypeInfo)]) {
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
            TypeInfo::Function(_) | TypeInfo::Option(_) | TypeInfo::Result(_, _) => {}
            TypeInfo::Unknown
            | TypeInfo::Integer(_)
            | TypeInfo::Float
            | TypeInfo::String
            | TypeInfo::StringLiteral(_)
            | TypeInfo::Boolean
            | TypeInfo::Array(_)
            | TypeInfo::Generic(_) => {}
        }
    }
}
