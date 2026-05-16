//! # Validation Rules
//!
//! Individual validation rules for the TypeScript subset.

/// Rule checker for forbidden features.
#[derive(Debug, Default)]
pub struct RuleChecker {
    /// Enable strict mode
    strict: bool,
}

impl RuleChecker {
    /// Create a new rule checker.
    #[must_use]
    pub fn new() -> Self {
        Self { strict: true }
    }

    /// Create with strict mode enabled.
    #[must_use]
    pub fn strict() -> Self {
        Self { strict: true }
    }

    /// Check if a feature is allowed.
    #[must_use]
    pub fn is_allowed(&self, feature: &str) -> bool {
        !self.strict || !Self::FORBIDDEN.iter().any(|&f| f == feature)
    }

    /// Forbidden features list.
    const FORBIDDEN: &'static [&'static str] = &[
        "any",
        "unknown",
        "var",
        "class",
        "extends",
        "implements",
        "new",
        "this",
        "super",
        "eval",
        "with",
        "typeof",
        "instanceof",
        "delete",
        "try",
        "catch",
        "throw",
    ];
}
