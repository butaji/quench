//! test262 runner tests

#[cfg(test)]
mod tests {
    use crate::test262::errors::{check_negative_test, error_types_match};
    use crate::test262::metadata::Negative;
    use crate::test262::runner::{TestOutcome, should_skip};
    use crate::JsError;

    #[test]
    fn test_error_types_match_case_insensitive() {
        assert!(error_types_match("ReferenceError", "referenceerror"));
        assert!(!error_types_match("ReferenceError", "TypeError"));
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
        assert!(matches!(outcome, TestOutcome::Pass));
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
        assert!(matches!(outcome, TestOutcome::Fail { .. }));
    }

    #[test]
    fn test_should_skip_flags() {
        let mut meta = crate::test262::metadata::Test262Metadata::default();
        meta.flags = vec!["module".to_string()];
        assert!(should_skip(&meta).is_some());

        // onlyStrict is supported (handled specially by runner.rs)
        meta.flags = vec!["onlyStrict".to_string()];
        assert!(should_skip(&meta).is_none());
    }

    #[test]
    fn test_should_skip_features() {
        let mut meta = crate::test262::metadata::Test262Metadata::default();
        meta.features = vec!["Promise".to_string()];
        assert!(should_skip(&meta).is_some());

        meta.features = vec!["class".to_string()];
        assert!(should_skip(&meta).is_some());
    }

    #[test]
    fn test_should_skip_none() {
        let meta = crate::test262::metadata::Test262Metadata::default();
        assert!(should_skip(&meta).is_none());
    }
}
