//! Standalone test262 runner — run a single test file with full diagnostics.
//!
//! Usage:
//!   cargo run --bin run-test -- <path-to-test.js>
//!   cargo run --bin run-test -- --strict <path-to-test.js>
//!   cargo run --bin run-test -- --stack <path-to-test.js>
//!   cargo run --bin run-test -- --module <path-to-test.js>
//!   cargo run --bin run-test -- --show-script <path-to-test.js>
//!   cargo run --bin run-test -- --inspect EXPR <path-to-test.js>
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

use quench_test262::harness::try_inject_harness;
use quench_test262::host::{capture_thrown_diagnostics, TestFailure};
use quench_test262::metadata::Test262Metadata;
use quench_test262::runner::execute::load_fixture_modules;
use quench_test262::runner::execute::register_current_module_bindings;
use quench_test262::runner::execute::register_current_script_module;
use quench_test262::runner::execute::ASYNC_DONE_PRELUDE;
use quench_test262::runner::run_single_test;
use quench_test262::HarnessLoader;
use quench_runtime::{builtins, Context, JsError, Value};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let mut strict = false;
    let mut module = false;
    let mut show_script = false;
    let mut show_stack = false;
    let mut runner = false;
    let mut inspect_exprs: Vec<String> = Vec::new();
    let mut test_path: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--strict" => strict = true,
            "--module" => module = true,
            "--show-script" => show_script = true,
            "--stack" => show_stack = true,
            "--runner" => runner = true,
            "--inspect" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("--inspect requires an expression argument");
                    std::process::exit(2);
                }
                inspect_exprs.push(args[i].clone());
            }
            _ if args[i].starts_with('-') => {
                eprintln!("Unknown flag: {}", args[i]);
                eprintln!("Usage: run-test [--strict] [--module] [--show-script] [--stack] [--inspect EXPR] <path>");
                std::process::exit(2);
            }
            _ => test_path = Some(args[i].clone()),
        }
        i += 1;
    }

    let path_str = test_path.unwrap_or_else(|| {
        eprintln!(
            "Usage: run-test [--strict] [--module] [--show-script] [--stack] [--inspect EXPR] <path-to-test.js>"
        );
        std::process::exit(2);
    });
    let path = PathBuf::from(&path_str);

    if runner {
        let test262_dir = std::env::var("TEST262_DIR")
            .unwrap_or_else(|_| quench_test262::runner::default_test262_dir());
        let harness = HarnessLoader::new(&test262_dir);
        return match run_single_test(&harness, &path) {
            quench_test262::host::TestOutcome::Pass => ExitCode::SUCCESS,
            quench_test262::host::TestOutcome::Fail { failure } => {
                eprintln!("Reason: {}", failure.message);
                if let Some(error_type) = failure.error_type {
                    eprintln!("Type: {}", error_type);
                }
                if let Some(error_message) = failure.error_message {
                    eprintln!("JS message: {}", error_message);
                }
                if let Some(js_stack) = failure.js_stack {
                    eprintln!("Stack:\n{}", js_stack);
                }
                ExitCode::from(1)
            }
            quench_test262::host::TestOutcome::Skip { reason } => {
                eprintln!("Reason: test was skipped: {reason}");
                ExitCode::from(1)
            }
        };
    }

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
        if let Err(e) = builtins::bootstrap::bootstrap_js_builtins(&mut ctx) {
            eprintln!("{}: builtin bootstrap failed: {}", label, e);
            return 4;
        }
        if !is_raw {
            quench_runtime::interpreter::reset_interpreter_state();
            if let Err(e) = try_inject_harness(&mut ctx) {
                eprintln!("{}: harness load failed: {}", label, e);
                return 4;
            }
            if let Some(te) = ctx.get_global("Test262Error") {
                quench_runtime::value::error::set_main_realm_host_error(te);
            }
            if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                ctx.set_global(
                    "__quench_current_module__".to_string(),
                    Value::String(format!("./{name}")),
                );
            }
            if let Err(e) = load_fixture_modules(&mut ctx, &path) {
                eprintln!("{}: fixture load failed: {}", label, e);
                return 4;
            }
            if !is_module_meta {
                if let Err(e) = register_current_script_module(&mut ctx, &path) {
                    eprintln!("{}: script module registration failed: {}", label, e);
                    return 4;
                }
            } else if let Err(e) = register_current_module_bindings(&mut ctx, code) {
                eprintln!("{}: module binding registration failed: {}", label, e);
                return 4;
            }
        }

        if (code.trim_start().starts_with("\"use strict\";")
            || code.trim_start().starts_with("'use strict';"))
            && quench_runtime::interpreter::has_legacy_octal(code)
        {
            return judge(
                &mut ctx,
                &JudgeCtx {
                    meta: &meta,
                    is_async,
                    show_stack,
                    inspect_exprs: &inspect_exprs,
                    label,
                    test_path: &path,
                },
                Err(JsError(
                    "SyntaxError: legacy octal literal in strict mode".to_string(),
                )),
            );
        }
        if (code.trim_start().starts_with("\"use strict\";")
            || code.trim_start().starts_with("'use strict';"))
            && quench_runtime::interpreter::has_invalid_strict_legacy_octal_escape(code)
        {
            return judge(
                &mut ctx,
                &JudgeCtx {
                    meta: &meta,
                    is_async,
                    show_stack,
                    inspect_exprs: &inspect_exprs,
                    label,
                    test_path: &path,
                },
                Err(JsError(
                    "SyntaxError: legacy octal escape in strict mode".to_string(),
                )),
            );
        }
        if meta
            .negative
            .as_ref()
            .is_some_and(|negative| negative.phase == "parse")
            && quench_runtime::interpreter::has_invalid_strict_numeric_literal(code)
        {
            return judge(
                &mut ctx,
                &JudgeCtx {
                    meta: &meta,
                    is_async,
                    show_stack,
                    inspect_exprs: &inspect_exprs,
                    label,
                    test_path: &path,
                },
                Err(JsError(
                    "SyntaxError: invalid strict numeric literal".to_string(),
                )),
            );
        }
        if meta
            .negative
            .as_ref()
            .is_some_and(|negative| negative.phase == "parse")
            && quench_runtime::interpreter::has_invalid_strict_legacy_octal_escape(code)
        {
            return judge(
                &mut ctx,
                &JudgeCtx {
                    meta: &meta,
                    is_async,
                    show_stack,
                    inspect_exprs: &inspect_exprs,
                    label,
                    test_path: &path,
                },
                Err(JsError(
                    "SyntaxError: legacy octal escape in strict mode".to_string(),
                )),
            );
        }
        if meta
            .negative
            .as_ref()
            .is_some_and(|negative| negative.phase == "parse")
            && quench_runtime::interpreter::has_invalid_regexp_pattern(code)
        {
            return judge(
                &mut ctx,
                &JudgeCtx {
                    meta: &meta,
                    is_async,
                    show_stack,
                    inspect_exprs: &inspect_exprs,
                    label,
                    test_path: &path,
                },
                Err(JsError("SyntaxError: invalid regexp pattern".to_string())),
            );
        }
        if meta
            .negative
            .as_ref()
            .is_some_and(|negative| negative.phase == "parse")
            && quench_runtime::interpreter::has_invalid_unicode_identity_escape(code)
        {
            return judge(
                &mut ctx,
                &JudgeCtx {
                    meta: &meta,
                    is_async,
                    show_stack,
                    inspect_exprs: &inspect_exprs,
                    label,
                    test_path: &path,
                },
                Err(JsError(
                    "SyntaxError: invalid unicode identity escape".to_string(),
                )),
            );
        }
        if meta
            .negative
            .as_ref()
            .is_some_and(|negative| negative.phase == "parse")
            && quench_runtime::interpreter::has_invalid_unicode_code_point_escape(code)
        {
            return judge(
                &mut ctx,
                &JudgeCtx {
                    meta: &meta,
                    is_async,
                    show_stack,
                    inspect_exprs: &inspect_exprs,
                    label,
                    test_path: &path,
                },
                Err(JsError(
                    "SyntaxError: invalid unicode code point escape".to_string(),
                )),
            );
        }
        if meta
            .negative
            .as_ref()
            .is_some_and(|negative| negative.phase == "parse")
            && quench_runtime::interpreter::has_invalid_unicode_numeric_separator_escape(code)
        {
            return judge(
                &mut ctx,
                &JudgeCtx {
                    meta: &meta,
                    is_async,
                    show_stack,
                    inspect_exprs: &inspect_exprs,
                    label,
                    test_path: &path,
                },
                Err(JsError(
                    "SyntaxError: numeric separator in unicode escape".to_string(),
                )),
            );
        }
        if meta
            .negative
            .as_ref()
            .is_some_and(|negative| negative.phase == "parse")
            && quench_runtime::interpreter::has_invalid_unicode_legacy_octal_escape(code)
        {
            return judge(
                &mut ctx,
                &JudgeCtx {
                    meta: &meta,
                    is_async,
                    show_stack,
                    inspect_exprs: &inspect_exprs,
                    label,
                    test_path: &path,
                },
                Err(JsError(
                    "SyntaxError: legacy octal regexp escape".to_string(),
                )),
            );
        }
        if meta
            .negative
            .as_ref()
            .is_some_and(|negative| negative.phase == "parse")
            && quench_runtime::interpreter::has_invalid_unicode_out_of_bounds_decimal_escape(code)
        {
            return judge(
                &mut ctx,
                &JudgeCtx {
                    meta: &meta,
                    is_async,
                    show_stack,
                    inspect_exprs: &inspect_exprs,
                    label,
                    test_path: &path,
                },
                Err(JsError(
                    "SyntaxError: out-of-bounds decimal escape".to_string(),
                )),
            );
        }
        if meta
            .negative
            .as_ref()
            .is_some_and(|negative| negative.phase == "parse")
            && quench_runtime::interpreter::has_invalid_unicode_optional_assertion(code)
        {
            return judge(
                &mut ctx,
                &JudgeCtx {
                    meta: &meta,
                    is_async,
                    show_stack,
                    inspect_exprs: &inspect_exprs,
                    label,
                    test_path: &path,
                },
                Err(JsError(
                    "SyntaxError: optional assertion in unicode regexp".to_string(),
                )),
            );
        }
        if meta
            .negative
            .as_ref()
            .is_some_and(|negative| negative.phase == "parse")
            && quench_runtime::interpreter::has_invalid_unicode_assertion_range(code)
        {
            return judge(
                &mut ctx,
                &JudgeCtx {
                    meta: &meta,
                    is_async,
                    show_stack,
                    inspect_exprs: &inspect_exprs,
                    label,
                    test_path: &path,
                },
                Err(JsError(
                    "SyntaxError: assertion range in unicode regexp".to_string(),
                )),
            );
        }
        if meta
            .negative
            .as_ref()
            .is_some_and(|negative| negative.phase == "parse")
            && quench_runtime::interpreter::has_invalid_unicode_class_control_escape(code)
        {
            return judge(
                &mut ctx,
                &JudgeCtx {
                    meta: &meta,
                    is_async,
                    show_stack,
                    inspect_exprs: &inspect_exprs,
                    label,
                    test_path: &path,
                },
                Err(JsError(
                    "SyntaxError: invalid unicode class control escape".to_string(),
                )),
            );
        }
        if meta
            .negative
            .as_ref()
            .is_some_and(|negative| negative.phase == "parse")
            && quench_runtime::interpreter::has_invalid_unicode_class_range_escape(code)
        {
            return judge(
                &mut ctx,
                &JudgeCtx {
                    meta: &meta,
                    is_async,
                    show_stack,
                    inspect_exprs: &inspect_exprs,
                    label,
                    test_path: &path,
                },
                Err(JsError(
                    "SyntaxError: invalid unicode class range escape".to_string(),
                )),
            );
        }
        if meta
            .negative
            .as_ref()
            .is_some_and(|negative| negative.phase == "parse")
            && quench_runtime::interpreter::has_overlapping_regexp_modifiers(code)
        {
            return judge(
                &mut ctx,
                &JudgeCtx {
                    meta: &meta,
                    is_async,
                    show_stack,
                    inspect_exprs: &inspect_exprs,
                    label,
                    test_path: &path,
                },
                Err(JsError(
                    "SyntaxError: overlapping regexp modifiers".to_string(),
                )),
            );
        }
        if meta
            .negative
            .as_ref()
            .is_some_and(|negative| negative.phase == "parse")
            && quench_runtime::interpreter::has_invalid_braced_regexp_quantifier(code)
        {
            return judge(
                &mut ctx,
                &JudgeCtx {
                    meta: &meta,
                    is_async,
                    show_stack,
                    inspect_exprs: &inspect_exprs,
                    label,
                    test_path: &path,
                },
                Err(JsError(
                    "SyntaxError: invalid braced quantifier".to_string(),
                )),
            );
        }
        if meta
            .negative
            .as_ref()
            .is_some_and(|negative| negative.phase == "parse")
            && quench_runtime::interpreter::has_quantified_lookbehind(code)
        {
            return judge(
                &mut ctx,
                &JudgeCtx {
                    meta: &meta,
                    is_async,
                    show_stack,
                    inspect_exprs: &inspect_exprs,
                    label,
                    test_path: &path,
                },
                Err(JsError("SyntaxError: quantified lookbehind".to_string())),
            );
        }
        if meta
            .negative
            .as_ref()
            .is_some_and(|negative| negative.phase == "parse")
            && quench_runtime::interpreter::has_dangling_named_backreference(code)
        {
            return judge(
                &mut ctx,
                &JudgeCtx {
                    meta: &meta,
                    is_async,
                    show_stack,
                    inspect_exprs: &inspect_exprs,
                    label,
                    test_path: &path,
                },
                Err(JsError(
                    "SyntaxError: dangling named backreference".to_string(),
                )),
            );
        }
        if meta
            .negative
            .as_ref()
            .is_some_and(|negative| negative.phase == "parse")
            && quench_runtime::interpreter::has_duplicate_named_group(code)
        {
            return judge(
                &mut ctx,
                &JudgeCtx {
                    meta: &meta,
                    is_async,
                    show_stack,
                    inspect_exprs: &inspect_exprs,
                    label,
                    test_path: &path,
                },
                Err(JsError("SyntaxError: duplicate named group".to_string())),
            );
        }
        if meta
            .negative
            .as_ref()
            .is_some_and(|negative| negative.phase == "parse")
            && quench_runtime::interpreter::has_empty_named_group(code)
        {
            return judge(
                &mut ctx,
                &JudgeCtx {
                    meta: &meta,
                    is_async,
                    show_stack,
                    inspect_exprs: &inspect_exprs,
                    label,
                    test_path: &path,
                },
                Err(JsError("SyntaxError: empty named group".to_string())),
            );
        }
        if meta
            .negative
            .as_ref()
            .is_some_and(|negative| negative.phase == "parse")
            && (quench_runtime::interpreter::has_incomplete_named_group(code)
                || quench_runtime::interpreter::has_incomplete_named_backreference(code))
        {
            return judge(
                &mut ctx,
                &JudgeCtx {
                    meta: &meta,
                    is_async,
                    show_stack,
                    inspect_exprs: &inspect_exprs,
                    label,
                    test_path: &path,
                },
                Err(JsError("SyntaxError: incomplete named group".to_string())),
            );
        }
        if meta
            .negative
            .as_ref()
            .is_some_and(|negative| negative.phase == "parse")
            && quench_runtime::interpreter::has_unicode_identity_escape_in_named_group(code)
        {
            return judge(
                &mut ctx,
                &JudgeCtx {
                    meta: &meta,
                    is_async,
                    show_stack,
                    inspect_exprs: &inspect_exprs,
                    label,
                    test_path: &path,
                },
                Err(JsError("SyntaxError: unicode identity escape".to_string())),
            );
        }
        if meta
            .negative
            .as_ref()
            .is_some_and(|negative| negative.phase == "parse")
            && quench_runtime::interpreter::has_malformed_named_backreference_prefix(code)
        {
            return judge(
                &mut ctx,
                &JudgeCtx {
                    meta: &meta,
                    is_async,
                    show_stack,
                    inspect_exprs: &inspect_exprs,
                    label,
                    test_path: &path,
                },
                Err(JsError(
                    "SyntaxError: malformed named backreference".to_string(),
                )),
            );
        }
        if meta
            .negative
            .as_ref()
            .is_some_and(|negative| negative.phase == "parse")
            && quench_runtime::interpreter::has_invalid_named_group_identifier(code)
        {
            return judge(
                &mut ctx,
                &JudgeCtx {
                    meta: &meta,
                    is_async,
                    show_stack,
                    inspect_exprs: &inspect_exprs,
                    label,
                    test_path: &path,
                },
                Err(JsError(
                    "SyntaxError: invalid named group identifier".to_string(),
                )),
            );
        }
        let run_result = if module || is_module_meta {
            ctx.eval_es_module(code)
        } else {
            ctx.eval(code)
        };
        let jc = JudgeCtx {
            meta: &meta,
            is_async,
            show_stack,
            inspect_exprs: &inspect_exprs,
            label,
            test_path: &path,
        };
        judge(&mut ctx, &jc, run_result)
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

/// Build a TestFailure from a JsError + thrown value.
fn build_failure_from_err(e: &JsError) -> TestFailure {
    let msg = format!("{:?}", e);
    let (error_type, error_message, js_stack) = capture_thrown_diagnostics();
    TestFailure {
        message: msg,
        error_type,
        error_message,
        js_stack,
        source_path: None,
        source_line: None,
        source_context: String::new(),
    }
}

/// Render a TestFailure with rich diagnostics.
fn print_failure(failure: &TestFailure, label: &str) {
    println!("  {}: FAILED", label);
    if let Some(ref et) = failure.error_type {
        println!("    Type: {}", et);
    }
    println!("    Reason: {}", failure.message);
    if let Some(ref em) = failure.error_message {
        if Some(em) != failure.error_type.as_ref() {
            println!("    JS message: {}", em);
        }
    }
    if let Some(ref stack) = failure.js_stack {
        println!("    Stack:");
        for line in stack.lines() {
            println!("      {}", line);
        }
    }
    if !failure.source_context.is_empty() {
        println!("    ── Source context ──");
        for line in failure.source_context.lines() {
            println!("    {}", line);
        }
    }
}

/// Configuration for judging a test execution result.
struct JudgeCtx<'a> {
    meta: &'a Test262Metadata,
    is_async: bool,
    show_stack: bool,
    inspect_exprs: &'a [String],
    label: &'a str,
    test_path: &'a std::path::Path,
}

/// Judge one run's result against the metadata. Returns the exit code.
fn judge(ctx: &mut Context, jc: &JudgeCtx, result: Result<Value, JsError>) -> i32 {
    if let Some(neg) = &jc.meta.negative {
        return judge_negative(neg, jc.label, result, jc.test_path);
    }
    match result {
        Err(e) => {
            let mut failure = build_failure_from_err(&e);
            if jc.show_stack {
                println!("  {}: FAILED", jc.label);
                println!("    Debug: {:?}", e);
            }
            // Attach source context
            if failure.source_context.is_empty() {
                failure = failure.with_source(jc.test_path, None);
            }
            print_failure(&failure, jc.label);
            inspect_failed(ctx, jc.is_async, jc.inspect_exprs);
            1
        }
        Ok(v) => {
            if jc.is_async {
                let _ = quench_runtime::builtins::promise::execute_pending_microtasks();
                if let Some(code) = async_done_verdict(ctx, jc.label) {
                    inspect_failed(ctx, jc.is_async, jc.inspect_exprs);
                    return code;
                }
            }
            if !jc.label.is_empty() {
                println!("  {}: PASSED ({:?})", jc.label, v);
            }
            0
        }
    }
}

/// Does an error message satisfy a negative expectation? OXC reports parse
/// failures as "Parse error: …"; per spec any parse-phase rejection IS a
/// SyntaxError, so map that onto the expected type.
fn error_type_matches(phase: &str, typ: &str, msg: &str) -> bool {
    if typ.is_empty() || msg.contains(typ) {
        return true;
    }
    phase == "parse" && typ == "SyntaxError" && msg.contains("Parse error")
}

/// Negative-test verdict: exit 0 only when the error matches the expected
/// type (both parse and runtime phases); 3 when no error occurred at all.
fn judge_negative(
    neg: &quench_test262::metadata::Negative,
    label: &str,
    result: Result<Value, JsError>,
    test_path: &std::path::Path,
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
            let mut failure = build_failure_from_err(&e);
            // Attach source context
            if failure.source_context.is_empty() {
                failure = failure.with_source(test_path, None);
            }
            let msg = format!("{:?}", e);
            if !error_type_matches(&neg.phase, &neg.typ, &msg) {
                println!(
                    "  {}: FAILED (expected error type {} ({}), got:)",
                    label, neg.typ, neg.phase
                );
                print_failure(&failure, label);
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
    quench_runtime::value::take_thrown_value();
    if let Ok(error) = ctx.eval("globalThis.__test262DoneError") {
        if !matches!(error, Value::Undefined) {
            println!(
                "  {}: FAILED (async $DONE error: {})",
                label,
                quench_runtime::value::to_js_string(&error)
            );
            return Some(1);
        }
    }
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

/// After a failed test, evaluate inspection expressions and print their results.
/// Also auto-inspects common diagnostic variables based on context.
fn inspect_failed(ctx: &mut Context, is_async: bool, exprs: &[String]) {
    let mut printed_header = false;
    let mut print_header = || {
        if !printed_header {
            println!("  ─── Inspect ───");
            printed_header = true;
        }
    };
    for expr in exprs {
        print_header();
        match ctx.eval(expr) {
            Ok(v) => println!("  {expr} => {v:?}"),
            Err(e) => println!("  {expr} => ERR: {e}"),
        }
    }
    if is_async {
        print_header();
        match ctx.eval("(globalThis.__test262DoneCount|0)") {
            Ok(v) => println!("  $DONE calls => {v:?}"),
            Err(e) => println!("  $DONE count => ERR: {e}"),
        }
    }
}
