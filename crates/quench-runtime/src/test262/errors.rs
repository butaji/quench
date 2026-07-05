//! test262 negative test error matching
//!
//! Provides precise matching of expected error types and phases for test262 negative tests.
//! Uses error class inheritance to support tests expecting base Error class.

use crate::test262::metadata::Negative;
use crate::JsError;

/// Complete list of JavaScript error types including base Error class
const ERROR_TYPES: &[&str] = &[
    "Error",
    "SyntaxError",
    "ReferenceError",
    "TypeError",
    "RangeError",
    "URIError",
    "EvalError",
    "InternalError",
    "AggregateError",
];

/// Extract the error type from an error message using precise matching
/// Only matches if the error type is at the start of the message (after whitespace)
pub fn extract_error_type(err_msg: &str) -> Option<String> {
    let msg = err_msg.trim();
    
    // Try to match error type at start (e.g., "ReferenceError: message" or "ReferenceError(message)")
    for et in ERROR_TYPES {
        // Check for "TypeError:" or "TypeError(" pattern
        if msg.starts_with(et) {
            let rest = &msg[et.len()..];
            if rest.is_empty() || rest.starts_with(':') || rest.starts_with('(') {
                return Some(et.to_string());
            }
        }
        // Also check for quoted error types like "\"ReferenceError:"
        if msg.starts_with(&format!("\"{}:", et)) {
            return Some(et.to_string());
        }
        // Check for single-quoted error types like 'ReferenceError:'
        if msg.starts_with(&format!("'{}:", et)) {
            return Some(et.to_string());
        }
    }
    None
}

/// Check if this error is a parse error based on error type
pub fn is_parse_error(err_msg: &str) -> bool {
    // Parse errors are SyntaxErrors from the parser
    extract_error_type(err_msg)
        .map(|t| t == "SyntaxError")
        .unwrap_or(false)
}

/// Check if error types match with inheritance support
/// Returns true if:
/// - The types are identical (case-insensitive)
/// - The actual type is a subtype of the expected type
pub fn error_types_match(expected: &str, actual: &str) -> bool {
    let expected_lower = expected.to_lowercase();
    let actual_lower = actual.to_lowercase();
    
    // Exact match
    if expected_lower == actual_lower {
        return true;
    }
    
    // Inheritance check: if expected is base Error, any JS error matches
    if expected_lower == "error" {
        return ERROR_TYPES.iter()
            .any(|et| et.to_lowercase() == actual_lower);
    }
    
    false
}

/// Check if actual error is a subtype of expected (for inheritance)
pub fn is_subtype_of(actual: &str, expected: &str) -> bool {
    let actual_lower = actual.to_lowercase();
    let expected_lower = expected.to_lowercase();
    
    if actual_lower == expected_lower {
        return true;
    }
    
    // If expected is Error, any error type is a subtype
    if expected_lower == "error" {
        return ERROR_TYPES.iter()
            .any(|et| et.to_lowercase() == actual_lower);
    }
    
    false
}

/// Phase detection based on error type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ErrorPhase {
    Parse,
    Runtime,
}

impl ErrorPhase {
    pub fn from_error_type(err_type: &str) -> Self {
        if err_type == "SyntaxError" {
            ErrorPhase::Parse
        } else {
            ErrorPhase::Runtime
        }
    }
    
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "parse" => ErrorPhase::Parse,
            "runtime" => ErrorPhase::Runtime,
            _ => ErrorPhase::Runtime, // Default to runtime
        }
    }
}

/// Detailed error info for precise matching
#[derive(Debug, Clone)]
pub struct ErrorInfo {
    pub error_type: String,
    pub phase: ErrorPhase,
    pub message: String,
}

impl ErrorInfo {
    pub fn from_result(result: &Result<(), JsError>) -> Option<Self> {
        match result {
            Err(e) => {
                let err_msg = e.0.as_str();
                let error_type = extract_error_type(err_msg)
                    .unwrap_or_else(|| "Unknown".to_string());
                let phase = ErrorPhase::from_error_type(&error_type);
                Some(ErrorInfo {
                    error_type,
                    phase,
                    message: err_msg.to_string(),
                })
            }
            Ok(_) => None,
        }
    }
}

/// Check negative test outcome with precise type and phase matching
pub fn check_negative_test(neg: &Negative, result: &Result<(), JsError>) -> super::TestOutcome {
    match result {
        Ok(_) => super::TestOutcome::Fail {
            error: format!("Expected {} (phase: {}) but test passed", neg.typ, neg.phase),
        },
        Err(ref e) => check_negative_test_error(neg, e),
    }
}

fn check_negative_test_error(neg: &Negative, e: &JsError) -> super::TestOutcome {
    let err_msg = format!("{:?}", e);
    let err_info = ErrorInfo::from_result(&Err(e.clone()));
    
    if let Some(info) = err_info {
        let expected_phase = ErrorPhase::from_str(&neg.phase);
        let phase_matches = phase_matches_expected(&expected_phase, &info.phase);
        let type_matches = is_subtype_of(&info.error_type, &neg.typ);
        
        if phase_matches && type_matches {
            super::TestOutcome::Pass
        } else {
            super::TestOutcome::Fail {
                error: format_negative_error_mismatch(neg, &info, &expected_phase),
            }
        }
    } else {
        super::TestOutcome::Fail {
            error: format!("Expected {} but got unparseable error: {}", neg.typ, err_msg),
        }
    }
}

fn phase_matches_expected(expected: &ErrorPhase, actual: &ErrorPhase) -> bool {
    expected == &ErrorPhase::Runtime || (expected == &ErrorPhase::Parse && actual == &ErrorPhase::Parse)
}

fn format_negative_error_mismatch(neg: &Negative, info: &ErrorInfo, expected_phase: &ErrorPhase) -> String {
    let phase_str = match expected_phase {
        ErrorPhase::Parse => "parse",
        ErrorPhase::Runtime => "runtime",
    };
    let actual_phase_str = match info.phase {
        ErrorPhase::Parse => "parse",
        ErrorPhase::Runtime => "runtime",
    };
    format!(
        "Expected {} (phase: {}) but got {} (phase: {}): {}",
        neg.typ, phase_str, info.error_type, actual_phase_str, info.message
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_error_type_reference_error() {
        assert_eq!(
            extract_error_type("ReferenceError: x is not defined"),
            Some("ReferenceError".to_string())
        );
    }

    #[test]
    fn test_extract_error_type_type_error() {
        assert_eq!(
            extract_error_type("TypeError: expected object"),
            Some("TypeError".to_string())
        );
    }

    #[test]
    fn test_extract_error_type_with_quotes() {
        assert_eq!(
            extract_error_type("\"ReferenceError: x is not defined\""),
            Some("ReferenceError".to_string())
        );
    }

    #[test]
    fn test_extract_error_type_no_match() {
        assert_eq!(extract_error_type("Some other error"), None);
        assert_eq!(extract_error_type(""), None);
    }

    #[test]
    fn test_extract_error_type_syntax_error() {
        assert_eq!(
            extract_error_type("SyntaxError: Unexpected token"),
            Some("SyntaxError".to_string())
        );
    }

    #[test]
    fn test_extract_error_type_base_error() {
        assert_eq!(
            extract_error_type("Error: something went wrong"),
            Some("Error".to_string())
        );
    }

    #[test]
    fn test_error_types_match_exact() {
        assert!(error_types_match("ReferenceError", "referenceerror"));
        assert!(error_types_match("SyntaxError", "SyntaxError"));
        assert!(error_types_match("TypeError", "TypeError"));
    }

    #[test]
    fn test_error_types_match_no_match() {
        assert!(!error_types_match("ReferenceError", "TypeError"));
        assert!(!error_types_match("SyntaxError", "ReferenceError"));
    }

    #[test]
    fn test_error_types_match_inheritance() {
        // Error base class should match any error type
        assert!(error_types_match("Error", "TypeError"));
        assert!(error_types_match("Error", "ReferenceError"));
        assert!(error_types_match("Error", "SyntaxError"));
    }

    #[test]
    fn test_is_parse_error() {
        assert!(is_parse_error("SyntaxError: Unexpected token"));
        assert!(is_parse_error("SyntaxError"));
        assert!(!is_parse_error("ReferenceError: x is not defined"));
        assert!(!is_parse_error("TypeError: expected object"));
    }

    #[test]
    fn test_error_phase_from_type() {
        assert_eq!(ErrorPhase::from_error_type("SyntaxError"), ErrorPhase::Parse);
        assert_eq!(ErrorPhase::from_error_type("ReferenceError"), ErrorPhase::Runtime);
        assert_eq!(ErrorPhase::from_error_type("TypeError"), ErrorPhase::Runtime);
        assert_eq!(ErrorPhase::from_error_type("Error"), ErrorPhase::Runtime);
    }

    #[test]
    fn test_error_phase_from_str() {
        assert_eq!(ErrorPhase::from_str("parse"), ErrorPhase::Parse);
        assert_eq!(ErrorPhase::from_str("runtime"), ErrorPhase::Runtime);
        assert_eq!(ErrorPhase::from_str("Parse"), ErrorPhase::Parse);
        assert_eq!(ErrorPhase::from_str("RUNTIME"), ErrorPhase::Runtime);
    }

    #[test]
    fn test_error_info_from_result() {
        let err = JsError::new("ReferenceError: x is not defined".to_string());
        let result: Result<(), JsError> = Err(err);
        let info = ErrorInfo::from_result(&result).unwrap();
        
        assert_eq!(info.error_type, "ReferenceError");
        assert_eq!(info.phase, ErrorPhase::Runtime);
        assert!(info.message.contains("x is not defined"));
    }

    #[test]
    fn test_error_info_from_result_parse_error() {
        let err = JsError::new("SyntaxError: Unexpected token".to_string());
        let result: Result<(), JsError> = Err(err);
        let info = ErrorInfo::from_result(&result).unwrap();
        
        assert_eq!(info.error_type, "SyntaxError");
        assert_eq!(info.phase, ErrorPhase::Parse);
    }

    #[test]
    fn test_check_negative_test_pass_runtime() {
        let neg = Negative {
            phase: "runtime".to_string(),
            typ: "ReferenceError".to_string(),
        };
        let err = JsError::new("ReferenceError: x is not defined".to_string());
        let result: Result<(), JsError> = Err(err);
        let outcome = check_negative_test(&neg, &result);
        assert!(matches!(outcome, super::super::TestOutcome::Pass));
    }

    #[test]
    fn test_check_negative_test_pass_with_inheritance() {
        // Test expecting Error but getting TypeError should pass
        let neg = Negative {
            phase: "runtime".to_string(),
            typ: "Error".to_string(),
        };
        let err = JsError::new("TypeError: expected object".to_string());
        let result: Result<(), JsError> = Err(err);
        let outcome = check_negative_test(&neg, &result);
        assert!(matches!(outcome, super::super::TestOutcome::Pass));
    }

    #[test]
    fn test_check_negative_test_fail_wrong_type() {
        let neg = Negative {
            phase: "runtime".to_string(),
            typ: "TypeError".to_string(),
        };
        let err = JsError::new("ReferenceError: x is not defined".to_string());
        let result: Result<(), JsError> = Err(err);
        let outcome = check_negative_test(&neg, &result);
        assert!(matches!(outcome, super::super::TestOutcome::Fail { .. }));
    }

    #[test]
    fn test_check_negative_test_fail_passed() {
        let neg = Negative {
            phase: "runtime".to_string(),
            typ: "ReferenceError".to_string(),
        };
        let result: Result<(), JsError> = Ok(());
        let outcome = check_negative_test(&neg, &result);
        assert!(matches!(outcome, super::super::TestOutcome::Fail { .. }));
    }

    #[test]
    fn test_check_negative_test_parse_phase() {
        let neg = Negative {
            phase: "parse".to_string(),
            typ: "SyntaxError".to_string(),
        };
        let err = JsError::new("SyntaxError: Unexpected token".to_string());
        let result: Result<(), JsError> = Err(err);
        let outcome = check_negative_test(&neg, &result);
        assert!(matches!(outcome, super::super::TestOutcome::Pass));
    }

    #[test]
    fn test_check_negative_test_parse_phase_wrong_type() {
        // Runtime error when parse error expected
        let neg = Negative {
            phase: "parse".to_string(),
            typ: "SyntaxError".to_string(),
        };
        let err = JsError::new("ReferenceError: x is not defined".to_string());
        let result: Result<(), JsError> = Err(err);
        let outcome = check_negative_test(&neg, &result);
        assert!(matches!(outcome, super::super::TestOutcome::Fail { .. }));
    }

    #[test]
    fn test_is_subtype_of() {
        assert!(is_subtype_of("TypeError", "Error"));
        assert!(is_subtype_of("ReferenceError", "Error"));
        assert!(is_subtype_of("SyntaxError", "Error"));
        assert!(is_subtype_of("TypeError", "TypeError"));
        assert!(!is_subtype_of("TypeError", "ReferenceError"));
    }
}
