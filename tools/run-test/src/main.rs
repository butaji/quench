//! Standalone test262 runner — run a single test file with full diagnostics.
//!
//! Usage:
//!   cargo run --bin run-test -- <path-to-test.js>
//!   cargo run --bin run-test -- --strict <path-to-test.js>
//!   cargo run --bin run-test -- --stack <path-to-test.js>
//!   cargo run --bin run-test -- --module <path-to-test.js>
//!   cargo run --bin run-test -- --show-script <path-to-test.js>
//!
//! Env: TEST262_DIR=<path-to-test262>
//!
//! Exit codes:
//!   0 = pass (for negative tests: the expected error type was thrown)
//!   1 = fail (includes negative error-type mismatch, async $DONE misuse)
//!   2 = usage error (bad flags / missing path)
//!   3 = negative test wrongly passed (expected error, none occurred)
//!   4 = harness/build/read failure (infrastructure, never a test verdict)

use std::path::PathBuf;
use std::process::ExitCode;

use quench_runtime::test262::harness::try_inject_harness;
use quench_runtime::test262::metadata::Test262Metadata;
use quench_runtime::test262::HarnessLoader;
use quench_runtime::{builtins, Context, JsError, Value};

/// Async prelude: $DONE records invocations on globalThis and rethrows error
/// arguments; the count is verified after eval (microtasks drained by then).
/// Must match runner/execute.rs ASYNC_DONE_PRELUDE.
const ASYNC_DONE_PRELUDE: &str = "var $DONE = function(error) { \
if (error !== undefined && error !== null) throw error; \
globalThis.__test262DoneCount = (globalThis.__test262DoneCount|0) + 1; \
if (globalThis.__test262DoneCount > 1) throw new Test262Error('$DONE called twice'); \
};\n";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let mut strict = false;
    let mut module = false;
    let mut show_script = false;
    let mut show_stack = false;
    let mut test_path: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--strict" => strict = true,
            "--module" => module = true,
            "--show-script" => show_script = true,
            "--stack" => show_stack = true,
            _ if args[i].starts_with('-') => {
                eprintln!("Unknown flag: {}", args[i]);
                eprintln!("Usage: run-test [--strict] [--module] [--show-script] [--stack] <path>");
                std::process::exit(2);
            }
            _ => test_path = Some(args[i].clone()),
        }
        i += 1;
    }

    let path_str = test_path.unwrap_or_else(|| {
        eprintln!(
            "Usage: run-test [--strict] [--module] [--show-script] [--stack] <path-to-test.js>"
        );
        std::process::exit(2);
    });
    let path = PathBuf::from(&path_str);

    let source = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {}: {}", path.display(), e);
            std::process::exit(4);
        }
    };

    println!("╔══════════════════════════════════════════════════╗");
    println!(
        "║  Test: {}",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    println!("╚══════════════════════════════════════════════════╝");

    let meta = Test262Metadata::parse(&source);
    if let Some(ref m) = meta {
        if let Some(ref d) = m.description {
            println!("  Description: {}", d);
        }
        if !m.features.is_empty() {
            println!("  Features: {}", m.features.join(", "));
        }
        if let Some(ref e) = m.esid {
            println!("  Spec: §{}", e);
        }
        if let Some(ref n) = m.negative {
            println!("  Expected: {} ({})", n.typ, n.phase);
        }
        if !m.includes.is_empty() {
            println!("  Includes: {}", m.includes.join(", "));
        }
        if !m.flags.is_empty() {
            println!("  Flags: {}", m.flags.join(", "));
        }
    }
    println!();

    let meta = meta.unwrap_or_default();
    let has_flag = |flag: &str| meta.flags.iter().any(|f| f == flag);
    let is_raw = has_flag("raw");
    let is_async = has_flag("async");
    let is_module_meta = has_flag("module");
    let only_strict = has_flag("onlyStrict");
    let no_strict = is_raw || has_flag("noStrict");

    let script = if is_raw {
        source.clone()
    } else {
        let test262_dir =
            std::env::var("TEST262_DIR").unwrap_or_else(|_| "tests/test262".to_string());
        let harness = HarnessLoader::new(&test262_dir);
        match harness.build_script(&source, &meta.includes) {
            Ok(s) => {
                if is_async {
                    format!("{}{}", ASYNC_DONE_PRELUDE, s)
                } else {
                    s
                }
            }
            Err(e) => {
                eprintln!("Harness: {}", e);
                std::process::exit(4);
            }
        }
    };

    let run_mode = if module || is_module_meta {
        "module"
    } else if is_async {
        "async"
    } else {
        "script"
    };
    println!("  Mode: {}", run_mode);
    if strict || only_strict {
        println!("  Strict: yes");
    }

    if show_script {
        println!("\n────────────── Generated Script ──────────────");
        for (ln, l) in script.lines().enumerate() {
            println!("{:4}: {}", ln + 1, l);
        }
        println!("───────────────────────────────────────────────\n");
    } else {
        println!("\n───────────────── Source ─────────────────────");
        for (i, line) in source.lines().enumerate() {
            println!("{:4}: {}", i + 1, line);
        }
        println!("───────────────────────────────────────────────\n");
    }

    let do_run = |code: &str, label: &str| -> i32 {
        let mut ctx = match Context::new() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{}: Context::new failed: {:?}", label, e);
                return 4;
            }
        };
        builtins::register_builtins(&mut ctx);
        if !is_raw {
            if let Err(e) = try_inject_harness(&mut ctx) {
                eprintln!("{}: harness load failed: {}", label, e);
                return 4;
            }
            // Set MAIN_REALM_TEST262_ERROR for this realm so create_js_error_with_type
            // uses the correct constructor (see value/error.rs for details).
            if let Some(te) = ctx.get_global("Test262Error") {
                quench_runtime::value::error::set_main_realm_test262_error(te);
            }
        }

        let run_result = if module || is_module_meta {
            ctx.eval_es_module(code)
        } else {
            ctx.eval(code)
        };
        judge(&mut ctx, &meta, is_async, show_stack, label, run_result)
    };

    let mut exit_code = 0;

    // Run sloppy mode (unless onlyStrict)
    if !only_strict {
        let mode_label = if strict { "sloppy" } else { "" };
        let code = do_run(&script, mode_label);
        if exit_code == 0 {
            exit_code = code;
        }
    }

    // Run strict mode
    if strict || only_strict || (!no_strict && !only_strict) {
        let strict_script = format!("\"use strict\";\n{}", script);
        let code = do_run(
            &strict_script,
            if strict || only_strict {
                "strict"
            } else {
                "strict (auto)"
            },
        );
        if exit_code == 0 {
            exit_code = code;
        }
    }

    println!();
    if exit_code == 0 {
        println!("✅ ALL PASSED");
    } else {
        println!("❌ FAILED (exit code {})", exit_code);
    }
    std::process::exit(exit_code);
}

/// Judge one run's result against the metadata. Returns the exit code.
/// Negative tests pass ONLY when the thrown error matches the expected type.
fn judge(
    ctx: &mut Context,
    meta: &Test262Metadata,
    is_async: bool,
    show_stack: bool,
    label: &str,
    result: Result<Value, JsError>,
) -> i32 {
    if let Some(neg) = &meta.negative {
        return judge_negative(neg, label, result);
    }
    match result {
        Err(e) => {
            println!("  {}: FAILED: {:?}", label, e);
            if show_stack {
                println!("  Error (full): {:?}", e);
            }
            1
        }
        Ok(v) => {
            if is_async {
                if let Some(code) = async_done_verdict(ctx, label) {
                    return code;
                }
            }
            if !label.is_empty() {
                println!("  {}: PASSED ({:?})", label, v);
            }
            0
        }
    }
}

/// Does an error message satisfy a negative expectation? OXC reports parse
/// failures as "Parse error: …"; per spec any parse-phase rejection IS a
/// SyntaxError, so map that onto the expected type.
/// Mirrors runner/execute.rs::error_type_matches (private module).
fn error_type_matches(phase: &str, typ: &str, msg: &str) -> bool {
    if typ.is_empty() || msg.contains(typ) {
        return true;
    }
    phase == "parse" && typ == "SyntaxError" && msg.contains("Parse error")
}

/// Negative-test verdict: exit 0 only when the error matches the expected
/// type (both parse and runtime phases); 3 when no error occurred at all.
fn judge_negative(
    neg: &quench_runtime::test262::metadata::Negative,
    label: &str,
    result: Result<Value, JsError>,
) -> i32 {
    match result {
        Ok(v) => {
            println!(
                "  {}: FAILED (negative test wrongly passed: expected {} ({}), got {:?})",
                label, neg.typ, neg.phase, v
            );
            3
        }
        Err(e) => {
            let msg = format!("{:?}", e);
            if !error_type_matches(&neg.phase, &neg.typ, &msg) {
                println!(
                    "  {}: FAILED (expected error type {} ({}), got: {})",
                    label, neg.typ, neg.phase, msg
                );
                return 1;
            }
            if !label.is_empty() {
                println!("  {}: PASSED (expected {} thrown)", label, neg.typ);
            }
            0
        }
    }
}

/// Verify an async test called $DONE exactly once. None = ok, Some(1) = fail.
fn async_done_verdict(ctx: &mut Context, label: &str) -> Option<i32> {
    match ctx.eval("globalThis.__test262DoneCount|0") {
        Ok(Value::Number(1.0)) => None,
        other => {
            println!(
                "  {}: FAILED (async test did not call $DONE exactly once: {:?})",
                label, other
            );
            Some(1)
        }
    }
}
