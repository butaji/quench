use super::*;
use std::path::PathBuf;

#[test]
fn fixture_declarations_are_evaluated_before_default_expression() {
    let (eval_source, _, _, _, _) =
        fixture_exports_from_source(0, "const y = 42;\nexport default y;\n").unwrap();
    assert_eq!(
        eval_source,
        "const y = 42;\nglobalThis.__quench_fixture_default_0 = y;"
    );
}

#[test]
fn in_process_and_isolated_share_one_timeout() {
    // Both paths read TEST_TIMEOUT_SECS; pin the value so a slow test cannot
    // pass in-process (formerly 10s) and fail isolated (formerly 15s).
    assert_eq!(test_timeout_secs(), 120);
}

#[test]
fn large_scripts_get_the_deep_execution_stack() {
    let path = PathBuf::from("ordinary.js");
    assert_eq!(worker_stack_size("x", &path), 64 * 1024 * 1024);
    assert_eq!(
        worker_stack_size(&"x".repeat(100_001), &path),
        1024 * 1024 * 1024
    );
    assert_eq!(
        worker_stack_size("UnicodeIDStart", &path),
        1024 * 1024 * 1024
    );
    assert_eq!(
        worker_stack_size("x", &PathBuf::from("nativeFunctionMatcher.js")),
        1024 * 1024 * 1024
    );
}

#[test]
fn initialized_context_exposes_the_main_realm_host_error() {
    let ctx = initialize_test_context(false).unwrap();
    assert!(ctx.get_global("Test262Error").is_some());
}

#[test]
fn initialized_test_context_hides_async_function_constructor() {
    let ctx = initialize_test_context(false).unwrap();
    assert_eq!(ctx.get_global("AsyncFunction"), None);
}

#[test]
fn outcome_classification_preserves_positive_and_runtime_failures() {
    use crate::metadata::{Negative, Test262Metadata};

    let positive = Test262Metadata::default();
    assert_eq!(check_outcome(&positive, Ok(()), None), TestOutcome::Pass);
    assert!(matches!(
        check_outcome(&positive, Err("TypeError: boom".into()), None),
        TestOutcome::Fail { failure } if failure.message == "TypeError: boom"
    ));

    let negative = Test262Metadata {
        negative: Some(Negative {
            phase: "runtime".into(),
            typ: "TypeError".into(),
        }),
        ..Test262Metadata::default()
    };
    assert_eq!(
        check_outcome(&negative, Err("TypeError: boom".into()), None),
        TestOutcome::Pass
    );
}

#[test]
fn outcome_classification_rejects_infrastructure_failures_as_expected_errors() {
    use crate::metadata::{Negative, Test262Metadata};

    let negative = Test262Metadata {
        negative: Some(Negative {
            phase: "runtime".into(),
            typ: "TypeError".into(),
        }),
        ..Test262Metadata::default()
    };
    assert!(matches!(
        check_outcome(&negative, Err("timed out after 30s".into()), None),
        TestOutcome::Fail { failure } if failure.message.starts_with("infrastructure failure")
    ));
}

#[test]
fn parse_syntax_error_matches_only_parse_phase_expectations() {
    use crate::metadata::{Negative, Test262Metadata};

    let parse_negative = Test262Metadata {
        negative: Some(Negative {
            phase: "parse".into(),
            typ: "SyntaxError".into(),
        }),
        ..Test262Metadata::default()
    };
    assert_eq!(
        check_outcome(
            &parse_negative,
            Err("Parse error: invalid token".into()),
            None
        ),
        TestOutcome::Pass
    );
}

#[test]
fn negative_type_mismatch_preserves_test_path_diagnostics() {
    use crate::metadata::{Negative, Test262Metadata};

    let path = PathBuf::from("test/harness/diagnostic.js");
    let meta = Test262Metadata {
        negative: Some(Negative {
            phase: "runtime".into(),
            typ: "TypeError".into(),
        }),
        ..Test262Metadata::default()
    };

    let outcome = check_outcome(&meta, Err("RangeError: boom".into()), Some(&path));
    assert!(matches!(
        outcome,
        TestOutcome::Fail { failure }
            if failure.message.contains("expected TypeError")
                && failure.source_path.as_deref() == Some("test/harness/diagnostic.js")
    ));
}

#[test]
fn plain_js_error_diagnostics_fall_back_to_error_text() {
    let outcome = check_outcome(
        &crate::metadata::Test262Metadata::default(),
        Err("TypeError: Cannot read properties of null or undefined".into()),
        None,
    );
    assert!(matches!(
        outcome,
        TestOutcome::Fail { failure }
            if failure.error_type.as_deref() == Some("TypeError")
                && failure.error_message.as_deref()
                    == Some("Cannot read properties of null or undefined")
    ));
}

#[test]
fn stage_zero_deep_equal_primitives_passes_through_runner() {
    use crate::harness::HarnessLoader;
    use crate::runner::default_test262_dir;

    let root = default_test262_dir();
    let harness = HarnessLoader::new(&root);
    let path = PathBuf::from(&root).join("test/harness/deepEqual-primitives.js");
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn stage_zero_native_function_matcher_passes_through_runner() {
    use crate::runner::default_test262_dir;

    let root = default_test262_dir();
    let path = PathBuf::from(&root).join("test/harness/nativeFunctionMatcher.js");
    assert_eq!(run_isolated(&path), TestOutcome::Pass);
}

#[test]
fn stage_zero_typed_array_conversions_pass_through_runner() {
    use crate::runner::default_test262_dir;

    let root = default_test262_dir();
    let path = PathBuf::from(&root).join("test/harness/testTypedArray-conversions.js");
    assert_eq!(run_isolated(&path), TestOutcome::Pass);
}

#[test]
fn isolated_runner_matches_in_process_strictness() {
    use crate::harness::HarnessLoader;
    use crate::runner::default_test262_dir;

    let path = std::env::temp_dir().join(format!(
        "quench-runner-strictness-{}-{}.js",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(
        &path,
        "/*---\ndescription: runner strictness invariant\n---*/\nundeclaredRunnerBinding = 1;\n",
    )
    .unwrap();
    let harness = HarnessLoader::new(&default_test262_dir());
    let in_process = run_single_test(&harness, &path);
    let isolated = run_isolated(&path);
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(in_process, TestOutcome::Fail { .. }));
    assert!(
        matches!(isolated, TestOutcome::Fail { ref failure } if failure.message.contains("strict:")),
        "isolated: {isolated:?}, in-process: {in_process:?}"
    );
}

#[test]
fn digest_workers_are_capped_for_process_isolation() {
    assert_eq!(crate::runner::digest::worker_count(64), 8);
}

#[test]
fn with_proxy_destructuring_binding_lookup_is_stack_safe() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../../tests/test262/test/language/expressions/assignment/destructuring/keyed-destructuring-property-reference-target-evaluation-order-with-bindings.js",
    );
    assert_eq!(run_isolated(&path), TestOutcome::Pass);
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
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root)
        .join("test/language/expressions/optional-chaining/eval-optional-call.js");
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn only_strict_numeric_negative_test_is_rejected_during_parse() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root).join("test/language/literals/numeric/7.8.3-1gs.js");
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn bigint_hex_literals_are_not_rejected_as_legacy_octal() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root).join("test/language/expressions/equals/bigint-and-bigint.js");
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn eval_super_property_from_class_method_is_valid() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path =
        PathBuf::from(&root).join("test/language/expressions/super/prop-dot-cls-val-from-eval.js");
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn isolated_large_output_test_does_not_block_on_pipes() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/test262/test/language/expressions/left-shift/S11.7.1_A4_T1.js");
    assert_eq!(run_isolated(&path), TestOutcome::Pass);
}

#[test]
fn private_name_in_computed_field_throws_type_error() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root).join(
        "test/language/statements/class/elements/private-field-is-visible-in-computed-properties.js",
    );
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn isolated_dynamic_import_registers_current_module_bindings() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../../tests/test262/test/language/expressions/dynamic-import/imported-self-update.js",
    );
    assert_eq!(run_isolated(&path), TestOutcome::Pass);
}

#[test]
fn indirectly_exported_function_binding_is_initialized_before_evaluation() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root).join("test/language/module-code/instn-iee-bndng-fun.js");
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn regexp_modifier_overlap_is_rejected_during_parse() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root)
        .join("test/language/literals/regexp/early-err-arithmetic-modifiers-add-remove-i.js");
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn regexp_modifier_duplicate_is_rejected_during_parse() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root).join(
        "test/language/literals/regexp/early-err-arithmetic-modifiers-code-point-repeat-i-1.js",
    );
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn regexp_modifier_without_colon_is_rejected_during_parse() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root)
        .join("test/language/literals/regexp/early-err-arithmetic-modifiers-no-colon-1.js");
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn regexp_modifier_with_empty_flags_is_rejected_during_parse() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root)
        .join("test/language/literals/regexp/early-err-arithmetic-modifiers-both-empty.js");
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn regexp_modifier_duplicate_without_subtraction_is_rejected_during_parse() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root)
        .join("test/language/literals/regexp/early-err-modifiers-code-point-repeat-i-1.js");
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn invalid_braced_regexp_quantifier_is_rejected_during_parse() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root)
        .join("test/language/literals/regexp/invalid-braced-quantifier-exact.js");
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn quantified_lookbehind_is_rejected_during_parse() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path =
        PathBuf::from(&root).join("test/language/literals/regexp/invalid-optional-lookbehind.js");
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn regexp_dangling_named_backreference_is_rejected_during_parse() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root)
        .join("test/language/literals/regexp/named-groups/invalid-dangling-groupname-2.js");
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn regexp_duplicate_named_group_is_rejected_during_parse() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root)
        .join("test/language/literals/regexp/named-groups/invalid-duplicate-groupspecifier-2.js");
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn regexp_empty_named_group_is_rejected_during_parse() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root)
        .join("test/language/literals/regexp/named-groups/invalid-empty-groupspecifier.js");
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn regexp_incomplete_named_backreference_is_rejected_during_parse() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root)
        .join("test/language/literals/regexp/named-groups/invalid-incomplete-groupname-2.js");
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn regexp_identity_escape_in_unicode_capture_is_rejected_during_parse() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root)
        .join("test/language/literals/regexp/named-groups/invalid-identity-escape-in-capture-u.js");
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn regexp_malformed_named_backreference_prefix_is_rejected_during_parse() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root)
        .join("test/language/literals/regexp/named-groups/invalid-incomplete-groupname-6.js");
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn regexp_invalid_named_group_identifier_character_is_rejected_during_parse() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root).join(
        "test/language/literals/regexp/named-groups/invalid-non-id-continue-groupspecifier.js",
    );
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn module_resolution_error_is_raised_before_module_body() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root)
        .join("test/language/module-code/ambiguous-export-bindings/error-export-from-named.js");
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn module_resolution_error_propagates_through_named_import() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root)
        .join("test/language/module-code/ambiguous-export-bindings/error-import-named.js");
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn module_resolution_error_propagates_through_side_effect_import() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root)
        .join("test/language/module-code/import-attributes/import-attribute-newlines.js");
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn dynamic_import_namespace_fixture_exports_uninitialized_bindings() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root).join(
        "test/language/expressions/dynamic-import/namespace/await-ns-get-nested-namespace-props-nrml.js",
    );
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn fixture_star_as_namespace_contains_exported_bindings() {
    let root = crate::runner::default_test262_dir();
    let path = PathBuf::from(&root).join(
        "test/language/expressions/dynamic-import/namespace/await-ns-get-nested-namespace-props-nrml.js",
    );
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    load_fixture_modules(&mut ctx, &path).unwrap();
    let Value::Object(module) = ctx
        .get_module("./get-nested-namespace-props-nrml-1_FIXTURE.js")
        .unwrap()
    else {
        panic!("expected module");
    };
    let Value::Object(namespace) = module.borrow().get("exportns").unwrap() else {
        panic!("expected namespace export");
    };
    assert!(namespace.borrow().has("starAsVarDecl"));
}

#[test]
fn module_can_dynamically_import_itself() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root)
        .join("test/language/expressions/dynamic-import/eval-export-dflt-cls-anon.js");
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn dynamic_import_evaluates_fixture_initialization_errors() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root).join(
        "test/language/expressions/dynamic-import/catch/nested-arrow-import-catch-eval-rqstd-abrupt-typeerror.js",
    );
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn static_block_nested_constructor_resolves_await_as_binding() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path =
        PathBuf::from(&root).join("test/language/expressions/class/static-init-await-reference.js");
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn compound_assignment_deleted_object_binding_throws() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root).join(
        "test/language/expressions/compound-assignment/compound-assignment-operator-calls-putvalue-lref--v--1.js",
    );
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn super_assignment_checks_this_before_computed_key() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let root = default_test262_dir();
    let path = PathBuf::from(&root)
        .join("test/language/expressions/super/prop-expr-uninitialized-this-putvalue.js");
    let harness = HarnessLoader::new(&root);
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn instanceof_propagates_prototype_getter_error() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

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
        negative: Some(crate::metadata::Negative {
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
fn async_done_replacement_on_global_object_is_observed_by_helper() {
    let script = format!(
        "{}globalThis.$DONE = function() {{}}; Promise.resolve().then(function() {{ $DONE(); }});",
        ASYNC_DONE_PRELUDE
    );
    assert_eq!(run_async_script(&script, false), Ok(()));
}

#[test]
fn async_helper_observes_replaced_done_callback() {
    let script = format!(
        "{}{}globalThis.$DONE = function() {{}}; asyncTest(function() {{ return Promise.resolve(); }});",
        ASYNC_DONE_PRELUDE,
        include_str!("../../../../../../tests/test262/harness/asyncHelpers.js")
    );
    assert_eq!(run_async_script(&script, false), Ok(()));
}

#[test]
fn async_returns_undefined_test_passes_through_runner() {
    use crate::harness::HarnessLoader;
    use crate::runner::default_test262_dir;

    let root = default_test262_dir();
    let path =
        PathBuf::from(&root).join("test/harness/asyncHelpers-asyncTest-returns-undefined.js");
    let outcome = run_single_test(&HarnessLoader::new(&root), &path);
    assert!(matches!(outcome, TestOutcome::Pass), "outcome: {outcome:?}");
}

#[test]
fn dynamic_import_rejection_reaches_catch_handler() {
    let script = format!(
        "{}import('./missing-module.js').catch(function(error) {{ if (error === undefined) throw new Error('missing error'); $DONE(); }}, $DONE);",
        ASYNC_DONE_PRELUDE
    );
    assert_eq!(run_async_script(&script, false), Ok(()));
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
fn async_await_monkey_patched_promise_done_completes() {
    let script = format!(
        "{}{}",
        ASYNC_DONE_PRELUDE,
        include_str!(
            "../../../../../../tests/test262/test/language/expressions/await/await-monkey-patched-promise.js"
        )
    );
    assert_eq!(run_async_script(&script, false), Ok(()));
}

#[test]
fn dynamic_import_loads_test262_fixture_module() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let path = std::path::PathBuf::from(default_test262_dir())
        .join("test/language/expressions/dynamic-import/namespace/await-ns-define-own-property.js");
    let harness = HarnessLoader::new(&default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn dynamic_import_fixture_exports_values() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let path = std::path::PathBuf::from(default_test262_dir())
        .join("test/language/expressions/dynamic-import/namespace/await-ns-prop-descs.js");
    let harness = HarnessLoader::new(&default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn dynamic_import_rejects_ambiguous_star_reexport() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let path = std::path::PathBuf::from(default_test262_dir()).join(
        "test/language/expressions/dynamic-import/catch/nested-arrow-import-catch-instn-iee-err-ambiguous-import.js",
    );
    let harness = HarnessLoader::new(&default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn dynamic_import_rejects_circular_named_reexport() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let path = std::path::PathBuf::from(default_test262_dir()).join(
        "test/language/expressions/dynamic-import/catch/nested-arrow-import-catch-instn-iee-err-circular.js",
    );
    let harness = HarnessLoader::new(&default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn module_linking_rejects_missing_named_export_from_fixture() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let path = std::path::PathBuf::from(default_test262_dir())
        .join("test/language/module-code/instn-named-err-not-found-as.js");
    let harness = HarnessLoader::new(&default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn module_linking_rejects_missing_named_alias_from_empty_fixture() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let path = std::path::PathBuf::from(default_test262_dir())
        .join("test/language/module-code/instn-iee-err-not-found-as.js");
    let harness = HarnessLoader::new(&default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn dynamic_import_rejects_script_code_fixture_as_syntax_error() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let path = std::path::PathBuf::from(default_test262_dir()).join(
        "test/language/expressions/dynamic-import/catch/nested-arrow-import-catch-eval-script-code-target.js",
    );
    let harness = HarnessLoader::new(&default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn stage44_probe_module_code_exports() {
    let root = crate::runner::default_test262_dir();
    let path = std::path::PathBuf::from(&root)
        .join("test/language/expressions/dynamic-import/namespace/await-ns-prop-descs.js");
    let mut ctx = quench_runtime::Context::new().expect("context");
    quench_runtime::builtins::register_builtins(&mut ctx);
    load_fixture_modules(&mut ctx, &path).expect("fixtures");

    let script = "import('./module-code_FIXTURE.js').then(ns => globalThis.__ns__ = ns);";
    ctx.eval(script).unwrap();
    quench_runtime::builtins::promise::execute_pending_microtasks().unwrap();

    assert_eq!(
        ctx.eval("typeof local1").unwrap(),
        Value::String("string".into())
    );
    assert_eq!(ctx.eval("local2").unwrap(), Value::String("TC39".into()));

    let module = ctx
        .get_module("./module-code_FIXTURE.js")
        .expect("module-code module");
    let Value::Object(module) = module else {
        panic!("module-code fixture is not an object");
    };

    let indirect = module.borrow().get("indirect");
    assert_eq!(indirect, Some(Value::String("Test262".into())));
    assert_eq!(
        module.borrow().get_own_value("local1"),
        Some(Value::String("Test262".into()))
    );
    assert_eq!(
        module.borrow().get_own_value("renamed"),
        Some(Value::String("TC39".into())),
    );
    assert_eq!(
        module.borrow().get_own_value("indirect"),
        Some(Value::String("Test262".into()))
    );

    assert_eq!(
        module.borrow().get("local1"),
        Some(Value::String("Test262".into()))
    );
    assert_eq!(
        module.borrow().get("renamed"),
        Some(Value::String("TC39".into()))
    );
    assert_eq!(
        module.borrow().get("indirect"),
        Some(Value::String("Test262".into()))
    );
    assert_eq!(module.borrow().get("default"), Some(Value::Number(42.0)));

    let ns = ctx.eval("globalThis.__ns__").unwrap();
    let Value::Object(ns_obj) = ns else {
        panic!("import result is not object: {ns:?}");
    };
    assert_eq!(
        ns_obj.borrow().get("indirect"),
        Some(Value::String("Test262".into()))
    );
}

#[test]
fn fixture_alias_reads_non_exported_declaration() {
    let root = crate::runner::default_test262_dir();
    let path = std::path::PathBuf::from(&root)
        .join("test/language/expressions/dynamic-import/namespace/await-ns-prop-descs.js");
    let mut ctx = quench_runtime::Context::new().expect("context");
    quench_runtime::builtins::register_builtins(&mut ctx);
    load_fixture_modules(&mut ctx, &path).expect("fixtures");
    let module = ctx.get_module("./module-code_FIXTURE.js").expect("module");
    let Value::Object(module) = module else {
        panic!("module is not an object")
    };
    assert_eq!(
        module.borrow().get_own_value("renamed"),
        Some(Value::String("TC39".into()))
    );
}

#[test]
fn dynamic_import_fixture_exports_nested_namespace() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let path = std::path::PathBuf::from(default_test262_dir()).join(
        "test/language/expressions/dynamic-import/namespace/await-ns-get-nested-namespace-dflt-direct.js",
    );
    let harness = HarnessLoader::new(&default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn dynamic_import_fixture_default_function_is_callable() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let path = std::path::PathBuf::from(default_test262_dir())
        .join("test/language/expressions/dynamic-import/update-to-dynamic-import.js");
    let harness = HarnessLoader::new(&default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn dynamic_import_fixture_default_function_is_registered_as_function() {
    let root = crate::runner::default_test262_dir();
    let path = std::path::PathBuf::from(&root)
        .join("test/language/expressions/dynamic-import/update-to-dynamic-import_FIXTURE.js");
    let mut ctx = quench_runtime::Context::new().expect("context");
    quench_runtime::builtins::register_builtins(&mut ctx);
    load_fixture_modules(&mut ctx, &path).expect("fixtures");
    let module = ctx
        .get_module("./update-to-dynamic-import_FIXTURE.js")
        .expect("fixture module");
    let Value::Object(module) = module else {
        panic!("module entry is not an object");
    };
    let Value::Function(_) = module.borrow().get("default").expect("default export") else {
        panic!(
            "default is not a function: {:?}",
            module.borrow().get("default").unwrap()
        );
    };
    let x = module.borrow().get("x").expect("x export");
    assert_eq!(x, quench_runtime::Value::String("first".to_string()));
}

#[test]
fn dynamic_import_fixture_default_function_source_chunks() {
    let root = crate::runner::default_test262_dir();
    let path = std::path::PathBuf::from(&root)
        .join("test/language/expressions/dynamic-import/update-to-dynamic-import_FIXTURE.js");
    let source = std::fs::read_to_string(path).expect("fixture");
    let (eval_source, side_effect_source, _, _, _) =
        fixture_exports_from_source(0, &source).unwrap();
    assert!(
        side_effect_source.contains("Function"),
        "eval: {eval_source}\nside: {side_effect_source}"
    );
    assert!(
        quench_runtime::parser::parse_script(&eval_source).is_ok(),
        "eval source failed parse:\n{eval_source}"
    );

    let path = std::path::PathBuf::from(&root)
        .join("test/language/expressions/dynamic-import/custom-tostring_FIXTURE.js");
    let source = std::fs::read_to_string(path).expect("fixture");
    let (eval_source, side_effect_source, _, _, _) =
        fixture_exports_from_source(1, &source).unwrap();
    assert_eq!(side_effect_source.trim(), "");
    assert!(eval_source.starts_with("//"));
}

#[test]
fn dynamic_import_fixture_custom_to_string_and_value_of_are_used() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let path = std::path::PathBuf::from(default_test262_dir())
        .join("test/language/expressions/dynamic-import/custom-primitive.js");
    let harness = HarnessLoader::new(&default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn stage44_probe_custom_primitive_exports() {
    let root = crate::runner::default_test262_dir();
    let path = std::path::PathBuf::from(&root)
        .join("test/language/expressions/dynamic-import/custom-primitive.js");
    let mut ctx = quench_runtime::Context::new().expect("context");
    quench_runtime::builtins::register_builtins(&mut ctx);
    load_fixture_modules(&mut ctx, &path).expect("fixtures");

    let to_string = ctx
        .get_module("./custom-tostring_FIXTURE.js")
        .expect("custom-tostring module");
    let Value::Object(to_string_module) = to_string else {
        panic!("custom-tostring module is not an object");
    };
    assert!(matches!(
        to_string_module.borrow().get("toString"),
        Some(Value::Function(_))
    ));

    let value_of = ctx
        .get_module("./custom-valueof_FIXTURE.js")
        .expect("custom-valueof module");
    let Value::Object(value_of_module) = value_of else {
        panic!("custom-valueof module is not an object");
    };
    assert!(matches!(
        value_of_module.borrow().get("valueOf"),
        Some(Value::Function(_))
    ));
}

#[test]
fn dynamic_import_fixture_indirect_default_import_is_promise() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let path = std::path::PathBuf::from(default_test262_dir())
        .join("test/language/expressions/dynamic-import/indirect-resolution.js");
    let harness = HarnessLoader::new(&default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn stage44_probe_nested_namespace_import() {
    let root = crate::runner::default_test262_dir();
    let path = std::path::PathBuf::from(&root)
        .join("test/language/expressions/dynamic-import/namespace/await-ns-get-nested-namespace-dflt-direct.js");
    let mut ctx = quench_runtime::Context::new().expect("context");
    quench_runtime::builtins::register_builtins(&mut ctx);
    load_fixture_modules(&mut ctx, &path).expect("fixtures");

    ctx.eval("import('./get-nested-namespace-dflt-skip-prod_FIXTURE.js').then(ns => globalThis.__nested_namespace__ = ns);")
        .unwrap();
    quench_runtime::builtins::promise::execute_pending_microtasks().unwrap();
    let ns = ctx.eval("globalThis.__nested_namespace__").unwrap();
    let Value::Object(ns_obj) = ns else {
        panic!("nested import result is not object: {ns:?}");
    };

    assert!(matches!(
        ns_obj.borrow().get("productionNS2"),
        Some(Value::Object(_))
    ));
}

#[test]
fn stage44_probe_module_code_reexports() {
    use crate::runner::execute::fixture_exports_from_source;

    let root = crate::runner::default_test262_dir();
    let path = std::path::PathBuf::from(&root)
        .join("test/language/expressions/dynamic-import/namespace/module-code_FIXTURE.js");
    let source = std::fs::read_to_string(path).expect("read fixture");
    let (_eval, _side_effect, _exports, _default_import, reexports) =
        fixture_exports_from_source(0, &source).expect("parse");
    let mut saw = false;
    let mut total_named = 0usize;
    for reexport in reexports {
        if let crate::runner::execute::PendingReExport::Named {
            source,
            local,
            exported,
        } = reexport
        {
            total_named += 1;
            eprintln!("named {source}::{local}->{exported}");
            if source == "./module-code_FIXTURE.js" && local == "local1" && exported == "indirect" {
                saw = true;
            }
        } else {
            eprintln!("non-named reexport");
        }
    }
    eprintln!("total named={total_named}");
    assert!(saw, "expected reexport mapping was not parsed");
}

#[test]
fn stage44_probe_indirect_default_import() {
    use crate::runner::execute::fixture_exports_from_source;

    let root = crate::runner::default_test262_dir();
    let path = std::path::PathBuf::from(&root)
        .join("test/language/expressions/dynamic-import/indirect-resolution-1_FIXTURE.js");
    let source = std::fs::read_to_string(path).expect("read fixture");
    let (_eval, _side, _exports, default_import, _reexports) =
        fixture_exports_from_source(0, &source).expect("parse");
    assert_eq!(
        default_import,
        Some("./indirect-resolution-2_FIXTURE.js".to_string())
    );
}

#[test]
fn stage44_probe_indirect_default_import_is_promise_object() {
    let root = crate::runner::default_test262_dir();
    let path = std::path::PathBuf::from(&root)
        .join("test/language/expressions/dynamic-import/indirect-resolution.js");
    let mut ctx = quench_runtime::Context::new().expect("context");
    quench_runtime::builtins::register_builtins(&mut ctx);
    load_fixture_modules(&mut ctx, &path).expect("fixtures");

    let module = ctx
        .get_module("./indirect-resolution-1_FIXTURE.js")
        .expect("module1");
    let Value::Object(module) = module else {
        panic!("module not object");
    };
    let default_export = module
        .borrow()
        .get("default")
        .expect("default export exists");
    let Value::Object(default_obj) = default_export else {
        panic!("default export is not object: {default_export:?}");
    };
    assert!(
        default_obj.borrow().promise_data.is_some(),
        "default export is not a Promise"
    );
}

#[test]
fn object_proto_methods_do_not_trigger_duplicate_proto_error() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let path = std::path::PathBuf::from(default_test262_dir())
        .join("test/language/expressions/object/__proto__-permitted-dup.js");
    let harness = HarnessLoader::new(&default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn object_proto_shorthand_properties_do_not_trigger_duplicate_proto_error() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let path = std::path::PathBuf::from(default_test262_dir())
        .join("test/language/expressions/object/__proto__-permitted-dup-shorthand.js");
    let harness = HarnessLoader::new(&default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn object_methods_cannot_be_constructed() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

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
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let path = std::path::PathBuf::from(default_test262_dir())
        .join("test/language/expressions/exponentiation/applying-the-exp-operator_A7.js");
    let harness = HarnessLoader::new(&default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn reflect_construct_invokes_class_target_with_new_target() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let path = std::path::PathBuf::from(default_test262_dir())
        .join("test/language/expressions/super/call-construct-invocation.js");
    let harness = HarnessLoader::new(&default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn reflect_construct_uses_new_target_realm_prototype() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

    let path = std::path::PathBuf::from(default_test262_dir())
        .join("test/language/expressions/super/realm.js");
    let harness = HarnessLoader::new(&default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn object_rest_proxy_skips_excluded_symbol_descriptors() {
    use crate::harness::HarnessLoader;
    use crate::runner::{default_test262_dir, run_single_test};

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
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
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
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
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
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn dynamic_import_script_evaluates_itself_once() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/expressions/dynamic-import/eval-self-once-script.js");
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn dynamic_import_then_chain_calls_done_once() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/expressions/dynamic-import/usage/nested-arrow-import-then-returns-thenable.js");
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn module_fixture_is_available_to_static_import() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/expressions/import.meta/distinct-for-each-module.js");
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn number_nan_is_non_deletable() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/types/object/S8.6.1_A3.js");
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn string_primitive_constructor_is_string() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/types/string/S8.4_A12.js");
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn strict_function_rejects_static_binding() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/directive-prologue/func-decl-parse.js");
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn break_cannot_cross_static_initialization_boundary() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/statements/break/static-init-without-label.js");
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn class_method_rejects_super_call_in_parameter_default() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/statements/class/definition/early-errors-class-method-formals-contains-super-call.js");
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn mixed_static_private_accessors_are_rejected() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/statements/class/private-non-static-getter-static-setter-early-error.js");
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn class_static_block_rejects_arguments_identifier() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/statements/class/static-init-invalid-arguments.js");
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn class_static_block_rejects_await_expression() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/statements/class/static-init-invalid-await.js");
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn class_static_block_rejects_yield_and_duplicate_labels() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    for relative in [
        "tests/test262/test/language/statements/class/static-init-invalid-yield.js",
        "tests/test262/test/language/statements/class/static-init-invalid-label-dup.js",
    ] {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .unwrap()
            .join(relative);
        assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
    }
}

#[test]
fn class_heritage_expression_is_strict() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/statements/class/strict-mode/with.js");
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn const_declaration_rejects_let_binding_name() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/statements/const/syntax/const-declaring-let-split-across-two-lines.js");
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn const_update_assignment_throws_type_error() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/statements/const/syntax/const-invalid-assignment-next-expression-for.js");
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn strict_for_in_destructuring_rejects_eval_and_arguments_targets() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    for relative in [
        "tests/test262/test/language/statements/for-in/dstr/array-elem-target-simple-strict.js",
        "tests/test262/test/language/statements/for-in/dstr/obj-id-init-simple-strict.js",
        "tests/test262/test/language/statements/for-in/dstr/obj-id-simple-strict.js",
    ] {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .unwrap()
            .join(relative);
        assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
    }
}

#[test]
fn function_construct_with_primitive_prototype_uses_object_prototype() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    for relative in [
        "tests/test262/test/language/statements/function/S13.2.2_A3_T1.js",
        "tests/test262/test/language/statements/function/S13.2.2_A3_T2.js",
    ] {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .unwrap()
            .join(relative);
        assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
    }
}

#[test]
fn if_branches_support_tail_call_optimization() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    for relative in [
        "tests/test262/test/language/statements/if/tco-if-body.js",
        "tests/test262/test/language/statements/if/tco-else-body.js",
    ] {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .unwrap()
            .join(relative);
        assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
    }
}

#[test]
fn labeled_statements_support_tail_call_optimization() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/statements/labeled/tco.js");
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn generator_class_method_rejects_yield_parameter_binding() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/statements/class/definition/methods-gen-yield-as-function-expression-binding-identifier.js");
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn for_loop_increment_closure_captures_iteration_binding() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/statements/for/scope-body-lex-open.js");
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn for_of_destructuring_closes_iterator_on_rest_reference_completion() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    for relative in [
        "tests/test262/test/language/statements/for-of/dstr/array-elem-trlg-iter-rest-rtrn-close.js",
        "tests/test262/test/language/statements/for-of/dstr/array-rest-iter-rtrn-close.js",
    ] {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .unwrap()
            .join(relative);
        assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
    }
}

#[test]
fn runner_throw_type_error_is_an_object() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/built-ins/ThrowTypeError/throws-type-error.js");
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn runner_deferred_and_eager_imports_do_not_panic() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/import/import-defer/evaluation-sync/module-imported-defer-and-eager.js");
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn runner_deferred_namespace_evaluates_on_export_access() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/import/import-defer/evaluation-triggers/trigger-exported-string-get.js");
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn runner_deferred_namespace_then_access_does_not_evaluate() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/import/import-defer/evaluation-triggers/ignore-exported-then-get.js");
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn runner_deferred_namespace_has_triggers_evaluation() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/import/import-defer/evaluation-triggers/trigger-not-exported-string-hasProperty.js");
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn runner_deferred_namespace_meta_operations_trigger_evaluation() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    for name in [
        "trigger-not-exported-string-defineOwnProperty.js",
        "trigger-not-exported-string-delete.js",
        "trigger-not-exported-string-getOwnProperty.js",
    ] {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .unwrap()
            .join("tests/test262/test/language/import/import-defer/evaluation-triggers")
            .join(name);
        assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
    }
}

#[test]
fn runner_deferred_namespace_own_keys_trigger_evaluation() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/import/import-defer/evaluation-triggers/trigger-ownPropertyKeys.js");
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn runner_deferred_namespace_super_access_obeys_key_trigger_rules() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    for name in [
        "ignore-exported-then-super-get.js",
        "trigger-exported-string-super-property-set-exported.js",
        "trigger-not-exported-string-super-property-set-exported.js",
    ] {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .unwrap()
            .join("tests/test262/test/language/import/import-defer/evaluation-triggers")
            .join(name);
        assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
    }
}

#[test]
fn runner_deferred_namespace_field_definition_triggers_evaluation() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/import/import-defer/evaluation-triggers/trigger-not-exported-string-super-property-define.js");
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn runner_deferred_namespace_identity_crosses_fixture_modules() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join(
            "tests/test262/test/language/import/import-defer/deferred-namespace-object/identity.js",
        );
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn runner_deferred_namespace_constructor_error_is_an_object() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/import/import-defer/deferred-namespace-object/exotic-object-behavior.js");
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn runner_deferred_import_reports_fixture_syntax_error_eagerly() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/import/import-defer/errors/syntax-error/import-defer-of-syntax-error-fails.js");
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn runner_deferred_module_rethrows_same_evaluation_error() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/import/import-defer/errors/module-throws/trigger-evaluation.js");
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn runner_deferred_import_after_failed_evaluation_links() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/import/import-defer/errors/module-throws/defer-import-after-evaluation.js");
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn runner_deferred_tla_namespace_does_not_evaluate_during_linking() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/import/import-defer/errors/get-other-while-evaluating-async/main.js");
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn runner_deferred_current_module_access_throws_while_evaluating() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join(
            "tests/test262/test/language/import/import-defer/errors/get-self-while-evaluating.js",
        );
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn runner_deferred_resolution_error_rejects_import() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/import/import-defer/errors/resolution-error/import-defer-of-missing-module-fails.js");
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn runner_deferred_dependency_access_throws_while_evaluating() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    for path in [
        "errors/get-other-while-evaluating/main.js",
        "errors/get-self-while-defer-evaluating/main.js",
    ] {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .unwrap()
            .join("tests/test262/test/language/import/import-defer")
            .join(path);
        assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
    }
}

#[test]
fn runner_deferred_tla_preserves_flattening_order() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/import/import-defer/evaluation-top-level-await/flattening-order/main.js");
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn runner_static_text_import_produces_string_default() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/import/import-attributes/text-empty.js");
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn runner_text_import_reads_javascript_named_fixture_as_text() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/import/import-attributes/text-javascript.js");
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn runner_json_import_preserves_object_value() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/import/import-attributes/json-value-object.js");
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn runner_json_import_link_errors_prevent_evaluation() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    for name in ["json-invalid.js", "json-named-bindings.js"] {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .unwrap()
            .join("tests/test262/test/language/import/import-attributes")
            .join(name);
        assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
    }
}

#[test]
fn runner_json_import_reuses_default_value() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/import/import-attributes/json-idempotency.js");
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn runner_bytes_import_preserves_binary_payload() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/import/import-bytes/bytes-from-png.js");
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn runner_self_text_import_reads_current_source() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/import/import-attributes/text-self.js");
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}

#[test]
fn runner_deferred_namespace_has_deferred_module_tag() {
    let harness = HarnessLoader::new(&crate::runner::default_test262_dir());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("tests/test262/test/language/import/import-defer/deferred-namespace-object/to-string-tag.js");
    assert_eq!(run_single_test(&harness, &path), TestOutcome::Pass);
}
