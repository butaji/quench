//! # Subset Validator Main
//!
//! Validates the zero-overhead TypeScript subset using text patterns.

use crate::analyzer::context::AnalysisContext;

/// Validation error with source location.
#[derive(Debug, Clone)]
pub struct ValidationError {
    pub location: String,
    pub message: String,
    pub code: &'static str,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.location, self.message)
    }
}

/// Validates the zero-overhead TypeScript subset.
#[derive(Debug, Default)]
pub struct SubsetValidator {
    complexity: usize,
}

impl SubsetValidator {
    /// Create a new validator.
    pub fn new() -> Self {
        Self { complexity: 0 }
    }

    /// Validate source file.
    pub fn validate(&mut self, source: &crate::parser::SourceFile) -> crate::Result<()> {
        let mut ctx = AnalysisContext::new(source);
        self.validate_text(&source.source, source.path.display().to_string(), &mut ctx)
    }

    /// Validate source text.
    pub fn validate_text(
        &mut self,
        source: &str,
        path: String,
        ctx: &mut AnalysisContext,
    ) -> crate::Result<()> {
        for line in source.lines() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with("//") {
                continue;
            }

            // Check for forbidden keywords
            if let Some(err) = check_forbidden_keywords(line, &path) {
                return Err(err);
            }

            // Check for high complexity
            if line.matches("case ").count() > 10 {
                self.complexity += 1;
            }
        }

        if self.complexity > 10 {
            ctx.add_warning(
                path,
                "High cyclomatic complexity detected (>10 switch cases)".into(),
                "complexity",
            );
        }

        Ok(())
    }
}

/// Check for forbidden keywords and patterns.
fn check_forbidden_keywords(line: &str, path: &str) -> Option<crate::RuneError> {
    let forbidden = [
        ("any", "Type 'any' requires dynamic dispatch. Use concrete types."),
        ("unknown", "Type 'unknown' requires dynamic dispatch. Use concrete types."),
        ("class ", "Classes and prototype inheritance are forbidden. Use plain objects and functions."),
        ("new ", "Constructor syntax is forbidden. Use factory functions."),
        ("var ", "Use const or let instead of var."),
        (" == ", "Use === for strict equality."),
        (" != ", "Use !== for strict inequality."),
        (" try {", "try/catch is forbidden. Use Result<T,E> return pattern."),
        (" catch ", "catch is forbidden. Use Result<T,E> return pattern."),
        ("throw ", "throw is forbidden. Use Result<T,E> return pattern."),
        ("eval(", "eval is forbidden. Dynamic code execution is not allowed."),
        ("with (", "with statement is forbidden. Use explicit scoping."),
        ("typeof ", "typeof is forbidden. Runtime type inspection not allowed."),
        (" instanceof ", "instanceof is forbidden. Use explicit type checks."),
        ("delete ", "delete is forbidden. Use ownership and explicit drops."),
        ("for (const k in", "for...in on objects is forbidden. Use for...of with Object.keys()."),
        (" arguments", "arguments object is forbidden. Use rest parameters."),
    ];

    for (pattern, message) in &forbidden {
        if line.contains(pattern) {
            return Some(crate::RuneError::Analysis {
                location: format!("{path}:1"),
                message: message.to_string(),
            });
        }
    }

    // Check for dynamic property access
    let dynamic_prop = regex::Regex::new(r"\w+\[")
        .ok()
        .and_then(|re| re.find(line));

    if dynamic_prop.is_some() {
        return Some(crate::RuneError::Analysis {
            location: format!("{path}:1"),
            message: "Dynamic property access is forbidden. Use Map<K,V> or fixed structs.".to_string(),
        });
    }

    // Check for integer division warning
    if line.contains(" / ")
        && regex::Regex::new(r"\d+ / \d+")
            .is_ok_and(|re| re.is_match(line))
    {
        // Integer division detected - this is just informational
    }

    None
}
