use super::*;
use std::path::PathBuf;

#[test]
fn in_process_and_isolated_share_one_timeout() {
    // Both paths read TEST_TIMEOUT_SECS; pin the value so a slow test cannot
    // pass in-process (formerly 10s) and fail isolated (formerly 15s).
    assert_eq!(TEST_TIMEOUT_SECS, 30);
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
fn optional_eval_call_is_indirect() {
    use crate::test262::harness::HarnessLoader;
    use crate::test262::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root)
        .join("test/language/expressions/optional-chaining/eval-optional-call.js");
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn super_assignment_checks_this_before_computed_key() {
    use crate::test262::harness::HarnessLoader;
    use crate::test262::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root)
        .join("test/language/expressions/super/prop-expr-uninitialized-this-putvalue.js");
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn instanceof_propagates_prototype_getter_error() {
    use crate::test262::harness::HarnessLoader;
    use crate::test262::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root)
        .join("test/language/expressions/instanceof/prototype-getter-with-object-throws.js");
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
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
fn for_await_async_iterator_completion_reaches_done() {
    let script = format!(
        "{}{}",
        ASYNC_DONE_PRELUDE,
        include_str!("../../../../../../tests/test262/test/language/statements/for-await-of/ticks-with-async-iter-resolved-promise-and-constructor-lookup-two.js")
    );
    assert_eq!(run_async_script(&script, false), Ok(()));
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
fn dynamic_import_loads_test262_fixture_module() {
    use crate::test262::harness::HarnessLoader;
    use crate::test262::runner::{default_test262_dir, run_single_test};

    let path = std::path::PathBuf::from(default_test262_dir())
        .join("test/language/expressions/dynamic-import/namespace/await-ns-define-own-property.js");
    let harness = HarnessLoader::new(&default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn dynamic_import_fixture_exports_values() {
    use crate::test262::harness::HarnessLoader;
    use crate::test262::runner::{default_test262_dir, run_single_test};

    let path = std::path::PathBuf::from(default_test262_dir())
        .join("test/language/expressions/dynamic-import/namespace/await-ns-prop-descs.js");
    let harness = HarnessLoader::new(&default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn dynamic_import_fixture_exports_nested_namespace() {
    use crate::test262::harness::HarnessLoader;
    use crate::test262::runner::{default_test262_dir, run_single_test};

    let path = std::path::PathBuf::from(default_test262_dir()).join(
        "test/language/expressions/dynamic-import/namespace/await-ns-get-nested-namespace-dflt-direct.js",
    );
    let harness = HarnessLoader::new(&default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn object_proto_methods_do_not_trigger_duplicate_proto_error() {
    use crate::test262::harness::HarnessLoader;
    use crate::test262::runner::{default_test262_dir, run_single_test};

    let path = std::path::PathBuf::from(default_test262_dir())
        .join("test/language/expressions/object/__proto__-permitted-dup.js");
    let harness = HarnessLoader::new(&default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn object_proto_shorthand_properties_do_not_trigger_duplicate_proto_error() {
    use crate::test262::harness::HarnessLoader;
    use crate::test262::runner::{default_test262_dir, run_single_test};

    let path = std::path::PathBuf::from(default_test262_dir())
        .join("test/language/expressions/object/__proto__-permitted-dup-shorthand.js");
    let harness = HarnessLoader::new(&default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn object_methods_cannot_be_constructed() {
    use crate::test262::harness::HarnessLoader;
    use crate::test262::runner::{default_test262_dir, run_single_test};

    let path = std::path::PathBuf::from(default_test262_dir())
        .join("test/language/expressions/object/method-definition/name-invoke-ctor.js");
    let harness = HarnessLoader::new(&default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn async_await_private_call_resolves_through_direct_then() {
    let script = format!(
        "{}class C {{ static #x(value) {{ return value; }} static async y(value) {{ return await this.#x(value); }} }} C.y(1).then(() => $DONE(), $DONE);",
        ASYNC_DONE_PRELUDE
    );
    assert_eq!(run_async_script(&script, false), Ok(()));
}

#[test]
fn exponentiation_one_to_infinity_is_nan() {
    use crate::test262::harness::HarnessLoader;
    use crate::test262::runner::{default_test262_dir, run_single_test};

    let path = std::path::PathBuf::from(default_test262_dir())
        .join("test/language/expressions/exponentiation/applying-the-exp-operator_A7.js");
    let harness = HarnessLoader::new(&default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn reflect_construct_invokes_class_target_with_new_target() {
    use crate::test262::harness::HarnessLoader;
    use crate::test262::runner::{default_test262_dir, run_single_test};

    let path = std::path::PathBuf::from(default_test262_dir())
        .join("test/language/expressions/super/call-construct-invocation.js");
    let harness = HarnessLoader::new(&default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn reflect_construct_uses_new_target_realm_prototype() {
    use crate::test262::harness::HarnessLoader;
    use crate::test262::runner::{default_test262_dir, run_single_test};

    let path = std::path::PathBuf::from(default_test262_dir())
        .join("test/language/expressions/super/realm.js");
    let harness = HarnessLoader::new(&default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn object_rest_proxy_skips_excluded_symbol_descriptors() {
    use crate::test262::harness::HarnessLoader;
    use crate::test262::runner::{default_test262_dir, run_single_test};

    let path = std::path::PathBuf::from(default_test262_dir()).join(
        "test/language/expressions/object/dstr/object-rest-proxy-gopd-not-called-on-excluded-keys.js",
    );
    let harness = HarnessLoader::new(&default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
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
