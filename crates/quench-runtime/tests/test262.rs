//! test262 conformance integration test
//!
//! Run with:
//!   cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture

use quench_runtime::test262::runner::execute::run_single_test;
use quench_runtime::test262::{QuenchHost, Test262Host, Test262Runner, HarnessLoader};
use quench_runtime::test262::host::TestOutcome;
use std::path::PathBuf;

#[test]
fn test_harness_deep_equal_basic() {
    let mut host = QuenchHost::new();
    let result = host.run_script("assert.deepEqual([], [])");
    assert!(
        result.is_ok(),
        "deepEqual([], []) should pass: {:?}",
        result
    );
}

#[test]
fn test_test262_error_global() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "assert.sameValue(typeof Test262Error, 'function', 'Test262Error should be a function')",
    );
    assert!(result.is_ok(), "Test262Error global check: {:?}", result);
}

#[test]
fn test_assert_same_value_basic() {
    let mut host = QuenchHost::new();
    let result = host.run_script("assert.sameValue(1, 1, 'one equals one')");
    assert!(result.is_ok(), "sameValue(1,1) should pass: {:?}", result);
}

#[test]
fn test_assert_same_value_nan() {
    let mut host = QuenchHost::new();
    let result = host.run_script("assert.sameValue(NaN, NaN, 'NaN equals NaN')");
    assert!(result.is_ok(), "sameValue(NaN,NaN) should pass: {:?}", result);
}

#[test]
fn test_assert_same_value_negative_zero() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "assert.sameValue(-0, -0, '-0 equals -0'); assert.sameValue(+0, +0, '+0 equals +0')",
    );
    assert!(result.is_ok(), "sameValue zero: {:?}", result);
}

#[test]
fn test_assert_throws_basic() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "assert.throws(TypeError, function() { null.x }, 'null.x throws TypeError')",
    );
    assert!(result.is_ok(), "assert.throws should pass: {:?}", result);
}

#[test]
fn test_for_in_with_defined_property() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "var o = {a: 1, b: 2}; var keys = []; for (var k in o) { keys.push(k) } assert.sameValue(keys.length, 2)",
    );
    assert!(result.is_ok());
}

// ── Per-iteration let/const binding (spec §14.7.1.1) ─────────────────────────

#[test]
fn test_for_loop_let_per_iteration_basic() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        r#"
        var result = [];
        for (let i = 0; i < 3; i++) {
            result.push(function() { return i; });
        }
        assert.sameValue(result[0](), 0, "first closure sees i=0");
        assert.sameValue(result[1](), 1, "second closure sees i=1");
        assert.sameValue(result[2](), 2, "third closure sees i=2");
        "#,
    );
    assert!(result.is_ok(), "per-iteration let binding failed: {:?}", result);
}

#[test]
fn test_for_loop_let_per_iteration_increment() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        r#"
        var result = [];
        for (let i = 0; i < 3; ) {
            result.push(function() { return i; });
            i++;
        }
        assert.sameValue(result[0](), 0, "first closure sees i=0");
        assert.sameValue(result[1](), 1, "second closure sees i=1");
        assert.sameValue(result[2](), 2, "third closure sees i=2");
        "#,
    );
    assert!(result.is_ok(), "per-iteration let with ++i failed: {:?}", result);
}

#[test]
fn test_for_loop_let_per_iteration_multiple() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        r#"
        var result = [];
        for (let i = 0, j = 10; i < 3; i++, j++) {
            result.push(function() { return i + j; });
        }
        assert.sameValue(result[0](), 10, "i=0, j=10 → 10");
        assert.sameValue(result[1](), 12, "i=1, j=11 → 12");
        assert.sameValue(result[2](), 14, "i=2, j=12 → 14");
        "#,
    );
    assert!(result.is_ok(), "multiple let bindings failed: {:?}", result);
}

#[test]
fn test_for_loop_let_closure_sees_body_value() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        r#"
        var results = [];
        for (let n = 1; n <= 2; n++) {
            var captured = n;
            results.push(function() { return captured; });
        }
        assert.sameValue(results[0](), 2);
        assert.sameValue(results[1](), 2);
        "#,
    );
    assert!(result.is_ok(), "var closure test failed: {:?}", result);
}

#[test]
fn test_for_loop_const_per_iteration() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        r#"
        var result = [];
        for (const i = 0; i < 3; i++) {
            result.push(function() { return i; });
        }
        assert.sameValue(result[0](), 0);
        assert.sameValue(result[1](), 1);
        assert.sameValue(result[2](), 2);
        "#,
    );
    assert!(result.is_ok(), "per-iteration const binding failed: {:?}", result);
}

// ── Test isolation regression ────────────────────────────────────────────────

#[test]
fn test_reset_interpreter_state_clears_control_flow() {
    quench_runtime::interpreter::reset_interpreter_state();
    assert!(
        quench_runtime::interpreter::take_control_flow().is_none(),
        "control flow should be None after reset"
    );
    assert!(
        !quench_runtime::interpreter::is_strict_mode(),
        "strict mode should be false after reset"
    );
}

#[test]
fn test_quench_host_state_isolation() {
    let tests = vec![
        "var x = 1;",
        "for (var i = 0; i < 3; i++) { }",
        "var a = []; for (let i = 0; i < 3; i++) { a.push(i); }",
        "try { throw new Error('test'); } catch(e) { }",
        "(function() { return 42; })()",
    ];
    let mut host = QuenchHost::new();
    for (i, test) in tests.iter().enumerate() {
        let result = host.run_script(test);
        assert!(result.is_ok(), "test {} should pass: {:?}", i, result);
    }
}

// ── Runner-path reproduction ─────────────────────────────────────────────────

/// Replicate the EXACT runner path: run_single_test -> run_prepared ->
/// build_script -> run_with_timeout -> execute_script -> run_script.
/// This tests the exact code path the digest runner uses.
#[test]
fn test_runner_path_per_iteration_binding() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
    let test262_dir = repo_root.join("tests/test262");
    let harness = HarnessLoader::new(test262_dir.to_str().unwrap());
    let mut host = QuenchHost::new();

    let test_path = test262_dir.join(
        "test/language/statements/let/syntax/\
         let-iteration-variable-is-freshly-allocated-for-each-iteration-single-let-binding.js"
    );

    let outcome = run_single_test(&mut host, &harness, &test_path);
    match outcome {
        TestOutcome::Pass => {} // good
        TestOutcome::Fail { failure } => {
            panic!("runner path failed: {} (type={:?})",
                failure.message, failure.error_type);
        }
        TestOutcome::Skip { reason } => {
            panic!("runner path skipped: {}", reason);
        }
    }
}

/// Run multiple per-iteration tests through the runner path back-to-back.
#[test]
fn test_runner_path_multi_let_per_iteration() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
    let test262_dir = repo_root.join("tests/test262");
    let harness = HarnessLoader::new(test262_dir.to_str().unwrap());
    let mut host = QuenchHost::new();

    let tests = vec![
        "let-iteration-variable-is-freshly-allocated-for-each-iteration-single-let-binding.js",
        "let-iteration-variable-is-freshly-allocated-for-each-iteration-multi-let-binding.js",
        "let-closure-inside-condition.js",
    ];

    for name in &tests {
        let test_path = test262_dir.join("test/language/statements/let/syntax").join(name);
        let outcome = run_single_test(&mut host, &harness, &test_path);
        match outcome {
            TestOutcome::Pass => {}
            TestOutcome::Fail { failure } => {
                panic!("{} failed: {} (type={:?})",
                    name, failure.message, failure.error_type);
            }
            TestOutcome::Skip { reason } => {
                panic!("{} skipped: {}", name, reason);
            }
        }
    }
}

// ── Stage 30 staged runner ───────────────────────────────────────────────────

#[test]
#[ignore = "staged test262 runner"]
fn test262_staged() {
    let test262_dir = {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
        std::env::var("TEST262_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| repo_root.join("tests/test262"))
    };
    let digest = std::env::var("TEST262_DIGEST")
        .ok()
        .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let runner = Test262Runner::new(test262_dir);
    let mut host = QuenchHost::new();
    let summary = runner.run(&mut host);
    if summary.skipped > 0 && !digest {
        let mut reason_counts: std::collections::BTreeMap<&str, usize> =
            std::collections::BTreeMap::new();
        for (_path, reason) in quench_runtime::test262::skip::crash_files() {
            *reason_counts.entry(*reason).or_default() += 1;
        }
        panic!(
            "Stage {} incomplete: {} skipped (skips never count as passes). \
             Configured skip reasons: {:?}. Fix the crash or remove the stale skip entry.",
            current_stage_label(),
            summary.skipped,
            reason_counts,
        );
    }
    if summary.failed > 0 {
        if digest {
            std::process::exit(1);
        } else {
            panic!(
                "Stage {} failed: {}/{} passed. First failure: {:?}",
                current_stage_label(),
                summary.passed,
                summary.passed + summary.failed,
                summary.first_failure,
            );
        }
    }
}

fn current_stage_label() -> String {
    std::env::var("TEST262_STAGE")
        .unwrap_or_else(|_| quench_runtime::test262::runner::default_stage().to_string())
}

#[test]
#[ignore = "run with --ignored"]
fn test262_one() {
    let test_path = std::env::var("TEST262_FILE").expect("TEST262_FILE env var required");
    let path = std::path::Path::new(&test_path);
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
    let test262_dir = std::env::var("TEST262_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root.join("tests/test262"));

    let runner = Test262Runner::new(test262_dir);
    let src = std::fs::read_to_string(path).expect("read test file");
    let meta = quench_runtime::test262::metadata::Test262Metadata::parse(&src).unwrap_or_default();
    let mut host = QuenchHost::new();
    let script = runner
        .harness
        .build_script(&src, &meta.includes)
        .expect("build script");
    let start = std::time::Instant::now();
    let result = host.run_script(&script);
    let elapsed = start.elapsed();
    println!("Time: {:?}", elapsed);
    match result {
        Ok(()) => println!("PASS"),
        Err(e) => panic!("FAIL: {}", e),
    }
}

// Reproducer: for (let i = 0; i < 2; ++i) {} must terminate (no infinite loop)
#[test]
fn for_loop_with_let_should_terminate() {
    use std::sync::mpsc;
    use std::time::Duration;

    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let mut host = QuenchHost::new();
        let result = host.run_script("for (let i = 0; i < 2; ++i) {}");
        let _ = tx.send(result);
    });

    match rx.recv_timeout(Duration::from_secs(3)) {
        Ok(result) => {
            assert!(result.is_ok(), "for loop must terminate, got: {:?}", result);
        }
        Err(_) => {
            panic!("TIMEOUT: for loop did not terminate in 3s");
        }
    }

    assert!(
        handle.join().is_ok(),
        "eval thread panicked"
    );
}

#[test]
fn for_loop_let_body_update_should_terminate() {
    use std::sync::mpsc;
    use std::time::Duration;

    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let mut host = QuenchHost::new();
        let result = host.run_script("var x = 0; for (let y = 0; y < 5; ) { y++; x++; } x");
        let _ = tx.send(result);
    });

    match rx.recv_timeout(Duration::from_secs(3)) {
        Ok(result) => {
            assert!(result.is_ok(), "for loop with body update must terminate, got: {:?}", result);
        }
        Err(_) => {
            panic!("TIMEOUT: for loop with body update did not terminate in 3s");
        }
    }

    assert!(
        handle.join().is_ok(),
        "eval thread panicked"
    );
}

#[test]
fn for_loop_let_per_iteration_binding_closure() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r = ctx.eval(
        r#"
        "use strict";
        var a = [];
        for (let i = 0; i < 5; ++i) {
            a.push(function() { return i; });
        }
        var results = a.map(function(f) { return f(); });
        var pass = true;
        for (var k = 0; k < 5; ++k) {
            if (results[k] !== k) pass = false;
        }
        pass
        "#,
    );
    assert!(r.is_ok(), "eval failed: {:?}", r);
    let v = r.as_ref().unwrap();
    assert!(
        quench_runtime::value::coerce::to_bool(v),
        "closures should capture per-iteration i values, got: {:?}",
        v
    );
}

#[test]
fn for_loop_multi_let_per_iteration_binding() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r = ctx.eval(
        r#"
        "use strict";
        var a = [];
        for (let i = 0, j = 10; i < 3; ++i, ++j) {
            a.push(function() { return i * 100 + j; });
        }
        var pass = true;
        if (a[0]() !== 10) pass = false;
        if (a[1]() !== 111) pass = false;
        if (a[2]() !== 212) pass = false;
        pass
        "#,
    );
    assert!(r.is_ok(), "eval failed: {:?}", r);
    let v = r.as_ref().unwrap();
    assert!(
        quench_runtime::value::coerce::to_bool(v),
        "closures should capture per-iteration i,j values, got: {:?}",
        v
    );
}
