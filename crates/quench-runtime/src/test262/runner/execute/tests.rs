use super::*;
use std::path::PathBuf;

#[test]
fn in_process_and_isolated_share_one_timeout() {
    // Both paths read TEST_TIMEOUT_SECS; pin the value so a slow test cannot
    // pass in-process (formerly 10s) and fail isolated (formerly 15s).
    assert_eq!(TEST_TIMEOUT_SECS, 15);
}

#[test]
fn first_existing_picks_debug_before_stale_release() {
    let dir = std::env::temp_dir().join(format!("quench-binpick-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("debug")).unwrap();
    std::fs::create_dir_all(dir.join("release")).unwrap();
    let release = dir.join("release/run-test");
    let debug = dir.join("debug/run-test");
    std::fs::write(&debug, "").unwrap();
    assert_eq!(preferred_run_test_binary(&dir), Some(debug.clone()));
    std::fs::write(&release, "").unwrap();
    assert_eq!(preferred_run_test_binary(&dir), Some(debug));
    assert_eq!(first_existing(&[dir.join("nope")]), None);
}

#[test]
fn isolated_run_finds_property_helper_from_any_cwd() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/statements/class/subclass/builtin-objects/String/length.js");
    let outcome = run_isolated(&path);
    assert!(
        !matches!(outcome, TestOutcome::Fail { ref failure } if failure.message.contains("propertyHelper.js")),
        "isolated run should resolve harness includes: {:?}",
        outcome
    );
}

#[test]
fn isolated_message_extracts_reason_line() {
    let stdout = "header\n❌ FAILED\n   Reason: Test262Error: boom\n";
    assert_eq!(
        isolated_message(b"", stdout.as_bytes()),
        "Test262Error: boom"
    );
}

#[test]
fn check_outcome_pass_and_fail() {
    let meta = Test262Metadata::default();
    assert_eq!(check_outcome(&meta, Ok(()), None), TestOutcome::Pass);
    assert!(matches!(
        check_outcome(&meta, Err("x".into()), None),
        TestOutcome::Fail { .. }
    ));
}

fn neg_meta(phase: &str, typ: &str) -> Test262Metadata {
    Test262Metadata {
        negative: Some(crate::test262::metadata::Negative {
            phase: phase.into(),
            typ: typ.into(),
        }),
        ..Default::default()
    }
}

#[test]
fn check_outcome_parse_negative_requires_matching_type() {
    let meta = neg_meta("parse", "SyntaxError");
    assert!(
        matches!(
            check_outcome(&meta, Err("ReferenceError: x is not defined".into()), None),
            TestOutcome::Fail { .. }
        ),
        "parse negative must fail when the error type does not match"
    );
    assert_eq!(
        check_outcome(&meta, Err("SyntaxError: unexpected token".into()), None),
        TestOutcome::Pass
    );
}

#[test]
fn check_outcome_does_not_match_error_type_in_user_message() {
    let meta = neg_meta("runtime", "TypeError");
    let outcome = check_outcome(
        &meta,
        Err("Error: message mentions TypeError but is not one".into()),
        None,
    );
    assert!(matches!(outcome, TestOutcome::Fail { .. }));
}

#[test]
fn check_outcome_infra_messages_never_pass_negative() {
    for msg in [
        "harness load failure: SyntaxError file missing",
        "timed out after 10s",
        "panicked",
        "failed to spawn test thread",
    ] {
        for phase in ["parse", "runtime"] {
            let meta = neg_meta(phase, "SyntaxError");
            assert!(
                matches!(
                    check_outcome(&meta, Err(msg.into()), None),
                    TestOutcome::Fail { .. }
                ),
                "'{}' must fail for phase {}",
                msg,
                phase
            );
        }
    }
}

#[test]
fn check_outcome_parse_negative_maps_oxc_parse_error_to_syntax_error() {
    let meta = neg_meta("parse", "SyntaxError");
    assert_eq!(
        check_outcome(&meta, Err("Parse error: [OxcDiagnostic …]".into()), None),
        TestOutcome::Pass
    );
    let rt = neg_meta("runtime", "SyntaxError");
    assert!(matches!(
        check_outcome(&rt, Err("Parse error: [OxcDiagnostic …]".into()), None),
        TestOutcome::Fail { .. }
    ));
}

#[test]
fn async_script_without_done_fails() {
    let script = format!(
        "{}Promise.resolve().then(function() {{ }});",
        ASYNC_DONE_PRELUDE
    );
    let r = run_async_script(&script, false);
    assert!(r.is_err(), "expected failure without $DONE, got {:?}", r);
}

#[test]
fn async_script_with_done_once_passes() {
    let script = format!(
        "{}Promise.resolve().then(function() {{ $DONE(); }});",
        ASYNC_DONE_PRELUDE
    );
    assert!(
        run_async_script(&script, false).is_ok(),
        "$DONE called exactly once must pass"
    );
}

#[test]
fn async_script_done_with_error_fails() {
    let script = format!(
        "{}Promise.resolve().then(function() {{ $DONE(new Error('boom')); }});",
        ASYNC_DONE_PRELUDE
    );
    let error = run_async_script(&script, false).unwrap_err();
    assert!(error.contains("boom"), "unexpected error: {}", error);
}

#[test]
fn strict_async_script_rejects_named_function_reassignment() {
    let script = format!(
        "\"use strict\";{}\
         var result; var ref = async function BindingIdentifier() {{ \
         (() => {{ BindingIdentifier = 1; }})(); }}; \
         ref().then(function() {{ result = 'resolved'; }}, function(error) {{ result = error.name; }}); \
         Promise.resolve().then(function() {{ if (result !== 'TypeError') throw new Error(result); $DONE(); }});",
        ASYNC_DONE_PRELUDE
    );
    let result = run_async_script(&script, false);
    assert_eq!(result, Ok(()));
}

#[test]
fn can_block_is_true_runs_instead_of_skipping() {
    let dir = std::env::temp_dir().join(format!("quench-cbit-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cbit.js");
    std::fs::write(
        &path,
        "/*---\ndescription: cbit\nflags: [CanBlockIsTrue]\n---*/\n1 + 1;\n",
    )
    .unwrap();
    let harness = HarnessLoader::new(&crate::test262::runner::default_test262_dir());
    let outcome = run_single_test(&harness, &path);
    assert_eq!(outcome, TestOutcome::Pass);
}

#[test]
fn function_prototype_constructor_descriptor_survives_prototype_accessor() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/statements/function/13.2-17-1.js");
    let harness = HarnessLoader::new(&crate::test262::runner::default_test262_dir());
    let outcome = run_single_test(&harness, &path);
    assert_eq!(outcome, TestOutcome::Pass);
}

#[test]
fn function_prototype_descriptor_is_not_configurable() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/statements/function/13.2-18-1.js");
    let harness = HarnessLoader::new(&crate::test262::runner::default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}
