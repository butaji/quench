//! # Ownership Analysis
//!
//! Infers Rust ownership patterns from TypeScript usage.

use super::{OwnershipAnalysis, TypeInfo, TypeMap};

/// Borrow mode for a binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorrowMode {
    /// Immutable borrow `&T`
    Shared,
    /// Mutable borrow `&mut T`
    Mut,
    /// Owned value `T`
    Owned,
    /// Unknown mode
    Unknown,
}

impl BorrowMode {
    /// Check if this mode allows mutation.
    #[must_use]
    pub fn is_mutable(&self) -> bool {
        matches!(self, BorrowMode::Mut | BorrowMode::Owned)
    }

    /// Combine two borrow modes.
    #[must_use]
    #[allow(clippy::similar_names)]
    pub fn combine(self, other: BorrowMode) -> BorrowMode {
        use BorrowMode::*;
        match (self, other) {
            (Unknown, m) | (m, Unknown) => m,
            (Shared, Shared) => Shared,
            (Mut, _) | (_, Mut) => Mut,
            (Owned, Owned) => Owned,
            (Shared, Owned) | (Owned, Shared) => Owned,
        }
    }

    /// Convert to Rust reference prefix.
    #[must_use]
    pub fn to_rust_prefix(&self) -> &'static str {
        match self {
            BorrowMode::Shared => "&",
            BorrowMode::Mut => "&mut ",
            BorrowMode::Owned => "",
            BorrowMode::Unknown => "&",
        }
    }
}

/// Analyzes ownership and borrowing patterns.
#[derive(Debug)]
pub struct OwnershipAnalyzer {
    /// Consumed values (moved)
    consumed: Vec<String>,
    /// Mutable references taken
    mut_refs: Vec<String>,
    /// Shared references taken
    shared_refs: Vec<String>,
}

impl OwnershipAnalyzer {
    /// Create a new ownership analyzer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            consumed: Vec::new(),
            mut_refs: Vec::new(),
            shared_refs: Vec::new(),
        }
    }

    /// Analyze types and produce ownership information.
    #[must_use]
    pub fn analyze(&mut self, types: &TypeMap) -> OwnershipAnalysis {
        let mut ownership = OwnershipAnalysis::default();

        for (name, info) in types.iter() {
            let mode = self.infer_mode(name, info);
            ownership.set(name.to_string(), mode);
        }

        ownership
    }

    /// Infer borrow mode from type info and usage patterns.
    fn infer_mode(&self, _name: &str, info: &TypeInfo) -> BorrowMode {
        match info {
            // Functions that take &mut self are mutable
            TypeInfo::Function(_) => BorrowMode::Owned,
            // Strings are usually borrowed unless mutated
            TypeInfo::String | TypeInfo::StringLiteral(_) => BorrowMode::Shared,
            // Arrays can be borrowed or owned
            TypeInfo::Array(_) => BorrowMode::Owned,
            // Primitives are usually owned or small-copied
            TypeInfo::Integer(_) | TypeInfo::Float | TypeInfo::Boolean => BorrowMode::Owned,
            // Complex types are usually owned
            TypeInfo::Struct(_) | TypeInfo::Enum(_) => BorrowMode::Owned,
            // Options and Results are usually owned
            TypeInfo::Option(_) | TypeInfo::Result(_, _) => BorrowMode::Owned,
            // Unknown defaults to shared
            TypeInfo::Unknown => BorrowMode::Unknown,
            // Generics default to owned
            TypeInfo::Generic(_) => BorrowMode::Owned,
        }
    }

    /// Record that a value was consumed (moved).
    pub fn record_consume(&mut self, name: &str) {
        self.consumed.push(name.to_string());
    }

    /// Record that a mutable reference was taken.
    pub fn record_mut_ref(&mut self, name: &str) {
        self.mut_refs.push(name.to_string());
    }

    /// Record that a shared reference was taken.
    pub fn record_shared_ref(&mut self, name: &str) {
        self.shared_refs.push(name.to_string());
    }

    /// Check if a value was consumed (moved).
    #[must_use]
    pub fn was_consumed(&self, name: &str) -> bool {
        self.consumed.contains(&name.to_string())
    }
}

impl Default for OwnershipAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}
