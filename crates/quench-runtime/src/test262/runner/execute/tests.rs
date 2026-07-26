use super::*;
use std::path::PathBuf;

#[test]
fn in_process_and_isolated_share_one_timeout() {
    // Both paths read TEST_TIMEOUT_SECS; pin the value so a slow test cannot
    // pass in-process (formerly 10s) and fail isolated (formerly 15s).
    assert_eq!(TEST_TIMEOUT_SECS, 15);
}

#[test]
fn first_existing_picks_release_before_debug() {
    let dir = std::env::temp_dir().join(format!("quench-binpick-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let release = dir.join("release");
    let debug = dir.join("debug");
    std::fs::write(&debug, "").unwrap();
    assert_eq!(
        first_existing(&[release.clone(), debug.clone()]),
        Some(debug.clone())
    );
    std::fs::write(&release, "").unwrap();
    assert_eq!(first_existing(&[release, debug]), Some(dir.join("release")));
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
        !matches!(outcome, TestOutcome::Fail { ref reason } if reason.contains("propertyHelper.js")),
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
    assert_eq!(check_outcome(&meta, Ok(())), TestOutcome::Pass);
    assert!(matches!(
        check_outcome(&meta, Err("x".into())),
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
            check_outcome(&meta, Err("ReferenceError: x is not defined".into())),
            TestOutcome::Fail { .. }
        ),
        "parse negative must fail when the error type does not match"
    );
    assert_eq!(
        check_outcome(&meta, Err("SyntaxError: unexpected token".into())),
        TestOutcome::Pass
    );
}

#[test]
fn check_outcome_infra_messages_never_pass_negative() {
    // Even when the message happens to contain the expected type name,
    // an infrastructure failure is never a test result.
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
                    check_outcome(&meta, Err(msg.into())),
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
    // OXC reports parse failures as "Parse error: …" — per spec that IS
    // a SyntaxError, so a parse-negative expecting SyntaxError must pass.
    let meta = neg_meta("parse", "SyntaxError");
    assert_eq!(
        check_outcome(&meta, Err("Parse error: [OxcDiagnostic …]".into())),
        TestOutcome::Pass
    );
    // …but a runtime-phase negative must not get the same free pass.
    let rt = neg_meta("runtime", "SyntaxError");
    assert!(matches!(
        check_outcome(&rt, Err("Parse error: [OxcDiagnostic …]".into())),
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
    assert!(run_async_script(&script, false).is_err());
}

#[test]
fn can_block_is_true_skips() {
    let dir = std::env::temp_dir().join(format!("quench-cbit-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cbit.js");
    std::fs::write(
        &path,
        "/*---\ndescription: cbit\nflags: [CanBlockIsTrue]\n---*/\n1 + 1;\n",
    )
    .unwrap();
    let harness = HarnessLoader::new(&crate::test262::runner::default_test262_dir());
    let mut host = QuenchHost::new();
    let outcome = run_single_test(&mut host, &harness, &path);
    assert!(
        matches!(outcome, TestOutcome::Skip { .. }),
        "CanBlockIsTrue must skip: {:?}",
        outcome
    );
}
