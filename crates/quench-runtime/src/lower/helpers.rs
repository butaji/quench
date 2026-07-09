//! Lower helpers - shared utilities for SWC AST lowering

use swc_atoms::Atom;


/// LowerError during lowering
#[derive(Debug, Clone)]
pub struct LowerError {
    pub message: String,
}

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for LowerError {}

impl LowerError {
    pub fn new(message: impl Into<String>) -> Self {
        LowerError { message: message.into() }
    }
}

/// Convert Atom to String using Display trait
pub fn atom_to_string(atom: &Atom) -> String {
    atom.to_string()
}

/// Convert Wtf8Atom to String using Display trait
pub fn wtf8_atom_to_string(atom: &swc_atoms::Wtf8Atom) -> String {
    atom.to_string_lossy().into_owned()
}
