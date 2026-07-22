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

use std::path::PathBuf;
use quench_runtime::{Context, builtins, test262::harness::{try_inject_harness, HarnessLoader}};
use quench_runtime::test262::metadata::Test262Metadata;
use quench_runtime::value::Value;

fn main() {
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
        eprintln!("Usage: run-test [--strict] [--module] [--show-script] [--stack] <path-to-test.js>");
        std::process::exit(2);
    });
    let path = PathBuf::from(&path_str);

    let source = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => { eprintln!("Error reading {}: {}", path.display(), e); std::process::exit(2); }
    };

    println!("╔══════════════════════════════════════════════════╗");
    println!("║  Test: {}", path.file_name().unwrap_or_default().to_string_lossy());
    println!("╚══════════════════════════════════════════════════╝");

    let meta = Test262Metadata::parse(&source);
    if let Some(ref m) = meta {
        if let Some(ref d) = m.description { println!("  Description: {}", d); }
        if !m.features.is_empty() { println!("  Features: {}", m.features.join(", ")); }
        if let Some(ref e) = m.esid { println!("  Spec: §{}", e); }
        if let Some(ref n) = m.negative { println!("  Expected: {} ({})", n.typ, n.phase); }
        if !m.includes.is_empty() { println!("  Includes: {}", m.includes.join(", ")); }
        if !m.flags.is_empty() { println!("  Flags: {}", m.flags.join(", ")); }
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
        let test262_dir = std::env::var("TEST262_DIR").unwrap_or_else(|_| "tests/test262".to_string());
        let harness = HarnessLoader::new(&test262_dir);
        match harness.build_script(&source, &meta.includes) {
            Ok(s) => {
                if is_async {
                    let prelude = "var $DONE = function(error) { if (error !== undefined && error !== null) throw error; };\n";
                    format!("{}{}", prelude, s)
                } else {
                    s
                }
            }
            Err(e) => { eprintln!("Harness: {}", e); std::process::exit(2); }
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
    if strict || only_strict { println!("  Strict: yes"); }

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
            Err(e) => { eprintln!("{}: Context::new failed: {:?}", label, e); return 1; }
        };
        builtins::register_builtins(&mut ctx);
        if !is_raw {
            if let Err(e) = try_inject_harness(&mut ctx) {
                eprintln!("{}: harness load failed: {}", label, e);
                return 1;
            }
        }

        let run_result = if module || is_module_meta {
            ctx.eval_es_module(code)
        } else {
            ctx.eval(code)
        };

        match run_result {
            Ok(Value::Undefined) => {
                if !label.is_empty() { println!("  {}: PASSED (undefined)", label); }
                0
            }
            Ok(v) => {
                if !label.is_empty() { println!("  {}: PASSED ({:?})", label, v); }
                0
            }
            Err(e) => {
                let msg = format!("{:?}", e);
                println!("  {}: FAILED: {}", label, msg);
                if show_stack {
                    // JsError format already includes message; for stack we'd need
                    // deeper integration. For now, show the full debug output.
                    println!("  Error (full): {:?}", e);
                }
                1
            }
        }
    };

    let mut exit_code = 0;

    // Run sloppy mode (unless onlyStrict)
    if !only_strict {
        let mode_label = if strict { "sloppy" } else { "" };
        exit_code |= do_run(&script, mode_label);
    }

    // Run strict mode
    if strict || only_strict || (!no_strict && !only_strict) {
        let strict_script = format!("\"use strict\";\n{}", script);
        exit_code |= do_run(&strict_script, if strict || only_strict { "strict" } else { "strict (auto)" });
    }

    println!();
    if exit_code == 0 {
        println!("✅ ALL PASSED");
    } else {
        println!("❌ FAILED (exit code {})", exit_code);
    }
    std::process::exit(exit_code);
}
