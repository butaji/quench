//! Standalone test262 runner — run a single test file with full diagnostics.
//! Usage: TEST262_DIR=tests/test262 cargo run --bin run-test -- <path-to-test.js>

use std::path::PathBuf;
use quench_runtime::{Context, builtins, test262::harness::{try_inject_harness, HarnessLoader}};
use quench_runtime::test262::metadata::Test262Metadata;
use quench_runtime::value::Value;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: run-test <path-to-test.js>");
        std::process::exit(1);
    }

    let path = PathBuf::from(&args[1]);
    let source = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => { eprintln!("Error reading {}: {}", path.display(), e); std::process::exit(1); }
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

    let is_raw = meta.as_ref().map_or(false, |m| m.flags.contains(&"raw".to_string()));
    let script = if is_raw {
        source.clone()
    } else {
        let inc = meta.as_ref().map(|m| m.includes.clone()).unwrap_or_default();
        let test262_dir = std::env::var("TEST262_DIR").unwrap_or_else(|_| "tests/test262".to_string());
        let harness = HarnessLoader::new(&test262_dir);
        match harness.build_script(&source, &inc) {
            Ok(s) => s,
            Err(e) => { eprintln!("Harness: {}", e); std::process::exit(1); }
        }
    };

    println!("───────────────────── Source ─────────────────────");
    for (i, line) in source.lines().enumerate() {
        println!("{:4}: {}", i + 1, line);
    }
    println!("──────────────────────────────────────────────────\n");

    let mut ctx = Context::new().unwrap();
    builtins::register_builtins(&mut ctx);
    if !is_raw { let _ = try_inject_harness(&mut ctx); }

    match ctx.eval(&script) {
        Ok(Value::Undefined) => { println!("✅ PASSED"); std::process::exit(0); }
        Ok(v) => { println!("✅ PASSED (returned: {:?})", v); std::process::exit(0); }
        Err(e) => { println!("❌ FAILED\n   Error: {:?}", e); std::process::exit(1); }
    }
}
