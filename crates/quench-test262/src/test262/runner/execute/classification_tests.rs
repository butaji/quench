use super::*;
use crate::runner::execute::classification_helpers::*;
use std::path::PathBuf;

#[test]
fn classification_table_driven_negative_phases() {
    let cases: Vec<(&str, &str, &str, bool)> = vec![
        (
            "parse",
            "SyntaxError",
            "SyntaxError: unexpected token",
            true,
        ),
        (
            "parse",
            "SyntaxError",
            "ReferenceError: x is not defined",
            false,
        ),
        (
            "runtime",
            "TypeError",
            "TypeError: cannot read property",
            true,
        ),
        ("runtime", "RangeError", "TypeError: not a range", false),
        (
            "runtime",
            "Test262Error",
            "Test262Error: deep equal failed",
            true,
        ),
        ("runtime", "Test262Error", "AssertionError: nope", false),
    ];
    for (phase, typ, msg, expected_pass) in cases {
        let meta = meta_with(phase, typ);
        let outcome = check_outcome(&meta, Err(msg.into()), None);
        assert_eq!(
            is_pass(&outcome),
            expected_pass,
            "phase={phase} typ={typ} msg={msg:?}"
        );
    }
}

#[test]
fn classification_table_driven_parse_error_oxc_mapping() {
    let parse_neg = meta_with("parse", "SyntaxError");
    assert!(matches!(
        check_outcome(&parse_neg, Err("Parse error: expected token".into()), None),
        TestOutcome::Pass
    ));
    let runtime_neg = meta_with("runtime", "SyntaxError");
    assert!(matches!(
        check_outcome(
            &runtime_neg,
            Err("Parse error: expected token".into()),
            None
        ),
        TestOutcome::Fail { .. }
    ));
    let range_neg = meta_with("parse", "RangeError");
    assert!(matches!(
        check_outcome(
            &range_neg,
            Err("Parse error: numeric overflow".into()),
            None
        ),
        TestOutcome::Fail { .. }
    ));
}

#[test]
fn classification_nested_colon_messages_do_not_mismatch() {
    let meta = meta_with("runtime", "Error");
    let outcome = check_outcome(
        &meta,
        Err("JsError(\"Error: failed at module:line:col\")".into()),
        None,
    );
    assert!(
        is_pass(&outcome),
        "Error: msg with nested colons after first colon must still match Error type: {outcome:?}"
    );
}

#[test]
fn classification_missing_thrown_object_falls_back_to_message_parsing() {
    let meta = positive_meta();
    let outcome = check_outcome(
        &meta,
        Err("JsError(\"TypeError: undefined reference\")".into()),
        None,
    );
    let TestOutcome::Fail { failure } = outcome else {
        panic!("expected Fail");
    };
    assert_eq!(failure.error_type.as_deref(), Some("TypeError"));
    assert_eq!(
        failure.error_message.as_deref(),
        Some("undefined reference")
    );
}

#[test]
fn classification_missing_thrown_object_with_no_js_error_wrapper() {
    let meta = positive_meta();
    let outcome = check_outcome(&meta, Err("oops".into()), None);
    let TestOutcome::Fail { failure } = outcome else {
        panic!("expected Fail");
    };
    assert_eq!(failure.message, "oops");
    assert_eq!(failure.error_type, None);
    assert_eq!(failure.error_message, None);
}

#[test]
fn classification_infra_messages_for_negative_metadata_fail() {
    let cases: &[&str] = &[
        "harness load failure: SyntaxError file missing",
        "builtin bootstrap failure: JsError(\"oh no\")",
        "timed out after 30s",
        "failed to spawn test thread",
    ];
    for phase in ["parse", "runtime"] {
        for msg in cases {
            let meta = meta_with(phase, "SyntaxError");
            let outcome = check_outcome(&meta, Err((*msg).into()), None);
            assert!(
                matches!(outcome, TestOutcome::Fail { .. }),
                "phase={phase} msg={msg:?}"
            );
        }
    }
}

#[test]
fn classification_js_thrown_messages_with_panic_substring_are_not_infra() {
    let js_throws = [
        "JsError(\"Error: thread panicked at index.js:5\")",
        "JsError(\"Error: stack overflow in deeply nested recursion\")",
        "JsError(\"Error: panicked during async resolution\")",
        "JsError(\"Error: harness load failure was simulated by test\")",
        "JsError(\"Error: failed to spawn subworker in user code\")",
        "JsError(\"Error: timed out by user Promise.race\")",
        "JsError(\"TypeError: panicked in reducer\")",
        "JsError(\"RangeError: stack overflow in tail recursion\")",
    ];
    for phase in ["parse", "runtime"] {
        for msg in js_throws {
            for typ in ["Error", "TypeError", "RangeError", "Test262Error"] {
                let meta = meta_with(phase, typ);
                let outcome = check_outcome(&meta, Err(msg.into()), None);
                assert!(
                    is_not_infra(&outcome),
                    "phase={phase} typ={typ} msg={msg:?} outcome={outcome:?}"
                );
            }
        }
    }
}

#[test]
fn classification_js_thrown_with_panic_substring_keeps_structured_diagnostics() {
    let meta = meta_with("runtime", "TypeError");
    let outcome = check_outcome(
        &meta,
        Err("JsError(\"Error: thread panicked at index.js:5\")".into()),
        Some(&PathBuf::from("test/diagnostic/path.js")),
    );
    let TestOutcome::Fail { failure } = outcome else {
        panic!("expected Fail for type-mismatched JS throw, got {outcome:?}");
    };
    assert_eq!(failure.error_type.as_deref(), Some("Error"));
    assert_eq!(
        failure.error_message.as_deref(),
        Some("thread panicked at index.js:5")
    );
    assert_eq!(
        failure.source_path.as_deref(),
        Some("test/diagnostic/path.js")
    );
}

#[test]
fn classification_positive_passes_when_test_completes() {
    assert!(is_pass(&check_outcome(&positive_meta(), Ok(()), None)));
}

#[test]
fn classification_positive_fails_when_test_throws() {
    let outcome = check_outcome(
        &positive_meta(),
        Err("TypeError: unexpected throw".into()),
        None,
    );
    assert!(matches!(outcome, TestOutcome::Fail { .. }));
}

#[test]
fn classification_positive_infra_failure_does_not_double_classify() {
    let outcome = check_outcome(&positive_meta(), Err("panicked".into()), None);
    let TestOutcome::Fail { failure } = outcome else {
        panic!("expected Fail");
    };
    assert_eq!(failure.message, "panicked");
}

#[test]
fn classification_negative_passing_test_fails() {
    let meta = meta_with("runtime", "TypeError");
    let outcome = check_outcome(&meta, Ok(()), None);
    let TestOutcome::Fail { failure } = outcome else {
        panic!("expected Fail");
    };
    assert!(failure.message.contains("expected error but passed"));
}

#[test]
fn classification_path_is_preserved_on_type_mismatch() {
    let path = PathBuf::from("test/harness/diagnostic.js");
    let meta = meta_with("runtime", "TypeError");
    let outcome = check_outcome(&meta, Err("RangeError: boom".into()), Some(&path));
    let TestOutcome::Fail { failure } = outcome else {
        panic!("expected Fail");
    };
    assert!(failure.message.contains("expected TypeError"));
    assert!(failure.message.contains("RangeError: boom"));
    assert_eq!(
        failure.source_path.as_deref(),
        Some("test/harness/diagnostic.js")
    );
}

#[test]
fn error_type_matches_accepts_empty_type() {
    assert!(error_type_matches("runtime", "", "TypeError: anything"));
    assert!(error_type_matches("parse", "", ""));
    assert!(error_type_matches("runtime", "", "anything"));
}

#[test]
fn error_type_matches_bare_kind_colon_message() {
    assert!(error_type_matches(
        "runtime",
        "TypeError",
        "TypeError: boom"
    ));
    assert!(error_type_matches(
        "runtime",
        "RangeError",
        "RangeError: boom"
    ));
    assert!(!error_type_matches(
        "runtime",
        "TypeError",
        "RangeError: boom"
    ));
}

#[test]
fn error_type_matches_error_without_message() {
    assert!(error_type_matches(
        "runtime",
        "Test262Error",
        "JsError(\"Test262Error\")"
    ));
}

#[test]
fn error_type_matches_envelope_wrapped_message() {
    assert!(error_type_matches(
        "runtime",
        "TypeError",
        "JsError(\"TypeError: boom\")"
    ));
    assert!(!error_type_matches(
        "runtime",
        "TypeError",
        "JsError(\"RangeError: boom\")"
    ));
}

#[test]
fn error_type_matches_nested_colons_only_split_first() {
    assert!(error_type_matches(
        "runtime",
        "Error",
        "JsError(\"Error: failed at module:line:col\")"
    ));
    assert!(error_type_matches(
        "runtime",
        "TypeError",
        "JsError(\"TypeError: parse JSON: unexpected token at position 0\")"
    ));
}

#[test]
fn error_type_matches_oxc_parse_error_maps_to_syntax_error_only_at_parse_phase() {
    assert!(error_type_matches(
        "parse",
        "SyntaxError",
        "Parse error: invalid token"
    ));
    assert!(!error_type_matches(
        "runtime",
        "SyntaxError",
        "Parse error: invalid token"
    ));
    assert!(!error_type_matches(
        "parse",
        "RangeError",
        "Parse error: invalid token"
    ));
}

#[test]
fn js_envelope_inner_extracts_inner_from_outer_wrapper() {
    let s = "expected TypeError but got: JsError(\"TypeError: boom\")";
    assert_eq!(js_envelope_inner(s), Some("TypeError: boom"));
    assert_eq!(js_envelope_inner("JsError(\"plain\")"), Some("plain"));
    assert_eq!(js_envelope_inner("no envelope here"), None);
}

#[test]
fn is_js_throw_msg_recognizes_known_envelope_kinds() {
    for kind in [
        "Error",
        "EvalError",
        "RangeError",
        "ReferenceError",
        "SyntaxError",
        "TypeError",
        "URIError",
        "AggregateError",
        "Test262Error",
    ] {
        assert!(
            is_js_throw_msg(&format!("JsError(\"{kind}: any\")")),
            "{kind} should be recognized as a JS throw envelope"
        );
    }
    assert!(!is_js_throw_msg("plain text"));
    assert!(!is_js_throw_msg("JsError(\"NotARealKind: anything\")"));
}

#[test]
fn check_outcome_envelope_typed_message_with_panic_substring_is_not_infra() {
    let meta = meta_with("runtime", "TypeError");
    let outcome = check_outcome(&meta, Err("TypeError: panicked mid-stack".into()), None);
    assert!(is_pass(&outcome), "outcome: {outcome:?}");
}

#[test]
fn check_outcome_envelope_typed_message_with_timed_out_substring_is_not_infra() {
    let meta = meta_with("runtime", "RangeError");
    let outcome = check_outcome(
        &meta,
        Err("RangeError: timed out by user race".into()),
        None,
    );
    assert!(is_pass(&outcome), "outcome: {outcome:?}");
}

#[test]
fn check_outcome_envelope_typed_message_with_harness_load_failure_substring_is_not_infra() {
    let meta = meta_with("runtime", "Error");
    let outcome = check_outcome(
        &meta,
        Err("Error: harness load failure was simulated by test".into()),
        None,
    );
    assert!(is_pass(&outcome), "outcome: {outcome:?}");
}

#[test]
fn check_outcome_envelope_typed_message_with_failed_to_spawn_substring_is_not_infra() {
    let meta = meta_with("runtime", "Error");
    let outcome = check_outcome(
        &meta,
        Err("Error: failed to spawn child in user code".into()),
        None,
    );
    assert!(is_pass(&outcome), "outcome: {outcome:?}");
}

#[test]
fn check_outcome_envelope_typed_message_with_oxc_parse_error_is_not_infra() {
    let meta = meta_with("runtime", "TypeError");
    let outcome = check_outcome(
        &meta,
        Err("TypeError: Parse error: tokenizer saw something".into()),
        None,
    );
    assert!(is_pass(&outcome), "outcome: {outcome:?}");
}

#[test]
fn check_outcome_infra_marker_inside_expected_error_wrapping_still_classifies_as_type_mismatch() {
    let meta = meta_with("runtime", "TypeError");
    let outcome = check_outcome(
        &meta,
        Err(
            "expected TypeError but got: JsError(\"RangeError: panic happened at index.js:5\")"
                .into(),
        ),
        None,
    );
    let TestOutcome::Fail { failure } = outcome else {
        panic!("expected Fail for type mismatch, got pass");
    };
    assert!(failure.message.contains("expected TypeError"));
    assert!(failure.message.contains("RangeError"));
    assert!(!failure.message.starts_with("infrastructure failure"));
}

#[test]
fn check_outcome_bare_negative_type_matches() {
    let meta = meta_with("runtime", "TypeError");
    assert!(is_pass(&check_outcome(
        &meta,
        Err("TypeError: boom".into()),
        None
    )));
    assert!(is_pass(&check_outcome(
        &meta,
        Err("JsError(\"TypeError: boom\")".into()),
        None
    )));
}

#[test]
fn check_outcome_bare_negative_type_mismatch() {
    let meta = meta_with("runtime", "TypeError");
    assert!(matches!(
        check_outcome(&meta, Err("RangeError: boom".into()), None),
        TestOutcome::Fail { .. }
    ));
    assert!(matches!(
        check_outcome(&meta, Err("JsError(\"RangeError: boom\")".into()), None),
        TestOutcome::Fail { .. }
    ));
}

#[test]
fn check_outcome_oxc_parse_error_matches_parse_phase_syntax_error() {
    let meta = meta_with("parse", "SyntaxError");
    assert!(is_pass(&check_outcome(
        &meta,
        Err("Parse error: invalid token".into()),
        None
    )));
    assert!(is_pass(&check_outcome(
        &meta,
        Err("Parse error: [OxcDiagnostic …]".into()),
        None
    )));
}

#[test]
fn check_outcome_oxc_parse_error_does_not_satisfy_runtime_phase() {
    let meta = meta_with("runtime", "SyntaxError");
    assert!(matches!(
        check_outcome(&meta, Err("Parse error: invalid token".into()), None),
        TestOutcome::Fail { .. }
    ));
}
