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

    /// Get the rust type annotation for this borrow mode.
    #[must_use]
    pub fn as_mut_prefix(&self) -> &'static str {
        match self {
            BorrowMode::Mut => "&mut ",
            _ => "&",
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
    /// Functions that consume ownership
    consuming_functions: Vec<String>,
}

impl OwnershipAnalyzer {
    /// Create a new ownership analyzer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            consumed: Vec::new(),
            mut_refs: Vec::new(),
            shared_refs: Vec::new(),
            consuming_functions: Self::default_consuming_functions(),
        }
    }

    /// Default list of functions that consume ownership.
    fn default_consuming_functions() -> Vec<String> {
        vec![
            "push".to_string(),
            "pop".to_string(),
            "splice".to_string(),
            "consume".to_string(),
        ]
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

    /// Infer borrow mode from type info.
    #[allow(unused_variables)]
    fn infer_mode(&self, name: &str, info: &TypeInfo) -> BorrowMode {
        use TypeInfo::*;
        match info {
            Function(_) => BorrowMode::Owned,
            String => BorrowMode::Shared,
            StringLiteral(_) => BorrowMode::Shared,
            Array(_) => BorrowMode::Owned,
            Integer(_) | Float | Boolean => BorrowMode::Owned,
            Struct(_) | Enum(_) => BorrowMode::Owned,
            Option(_) | Result(_, _) => BorrowMode::Owned,
            Unknown => BorrowMode::Unknown,
            Generic(_) => BorrowMode::Owned,
        }
    }

    /// Check if a function is known to consume ownership.
    #[must_use]
    pub fn is_consuming_function(&self, name: &str) -> bool {
        self.consuming_functions.iter().any(|f| f == name)
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

    /// Add a custom consuming function.
    pub fn add_consuming_function(&mut self, name: impl Into<String>) {
        self.consuming_functions.push(name.into());
    }

    /// Generate warning for potential move after use.
    #[must_use]
    pub fn check_move_after_use(&self, var_name: &str) -> Option<String> {
        if self.consumed.contains(&var_name.to_string()) {
            Some(format!(
                "Variable '{}' was moved and cannot be used after this point. \
                 Consider using .clone() to explicitly copy.",
                var_name
            ))
        } else {
            None
        }
    }
}

impl Default for OwnershipAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}
