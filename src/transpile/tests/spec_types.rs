//! Comprehensive parser + codegen tests for TypeScript Types (SUPPORTED_SUBSET.md 4.1-4.3)
//!
//! Tests are grouped by:
//! 1. Primitive type annotations
//! 2. Complex type annotations
//! 3. Type declarations (interface, type alias, enum)
//! 4. Type-directed lowering (THE KEY FEATURE - string unions -> enums, interfaces -> structs, etc.)

#[cfg(test)]
mod spec_types;
