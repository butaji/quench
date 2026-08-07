//! Inspect a test262 test file — show parsed metadata without running.
//!
//! Usage:
//!   cargo run --bin inspect-test -- <path-to-test.js>
//!   cargo run --bin inspect-test -- --source <path-to-test.js>

use std::path::PathBuf;
use quench_runtime::test262::metadata::Test262Metadata;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut show_source = false;
    let mut test_path: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--source" | "-s" => show_source = true,
            _ if args[i].starts_with('-') => {
                eprintln!("Unknown flag: {}", args[i]);
                eprintln!("Usage: inspect-test [--source] <path-to-test.js>");
                std::process::exit(1);
            }
            _ => test_path = Some(args[i].clone()),
        }
        i += 1;
    }

    let path_str = test_path.unwrap_or_else(|| {
        eprintln!("Usage: inspect-test [--source] <path-to-test.js>");
        std::process::exit(1);
    });
    let path = PathBuf::from(&path_str);

    let source = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {}: {}", path.display(), e);
            std::process::exit(1);
        }
    };

    println!("╔══════════════════════════════════════════════════╗");
    println!("║  Test: {}", path.file_name().unwrap_or_default().to_string_lossy());
    println!("╚══════════════════════════════════════════════════╝");
    println!("  Path: {}", path.display());

    if let Some(meta) = Test262Metadata::parse(&source) {
        println!();
        println!("── Metadata ─────────────────────────────────────");
        if let Some(ref d) = meta.description {
            println!("  Description: {}", d);
        }
        if let Some(ref e) = meta.esid {
            println!("  Spec: §{}", e);
        }
        if let Some(ref i) = meta.info {
            println!("  Info: {}", i.lines().next().unwrap_or(i));
        }
        if !meta.flags.is_empty() {
            println!("  Flags: {:?}", meta.flags);
        }
        if !meta.features.is_empty() {
            println!("  Features: {:?}", meta.features);
        }
        if !meta.includes.is_empty() {
            println!("  Includes: {:?}", meta.includes);
        }
        if let Some(ref n) = meta.negative {
            println!("  Negative: phase={}, type={}", n.phase, n.typ);
        }
    } else {
        println!("  No frontmatter metadata found");
    }

    println!();
    println!("── Summary ───────────────────────────────────────");
    let lines: Vec<&str> = source.lines().collect();
    println!("  Lines: {}", lines.len());
    println!("  Chars: {}", source.len());

    // Show first non-comment, non-metadata line
    let first_code = lines.iter().find(|l| {
        let t = l.trim();
        !t.is_empty() && !t.starts_with("/*---") && !t.starts_with("---*/")
    });
    if let Some(line) = first_code {
        println!("  First code: {}", line.trim());
    }

    // Detect test structure
    let has_assert = source.contains("assert.");
    let has_throw = source.contains("throw");
    let has_done = source.contains("$DONE");
    let has_module = source.contains("export");
    println!("  assert.*: {}, throw: {}, $DONE: {}, export: {}",
        if has_assert { "yes" } else { "no" },
        if has_throw { "yes" } else { "no" },
        if has_done { "yes" } else { "no" },
        if has_module { "yes" } else { "no" },
    );

    if show_source {
        println!();
        println!("── Source ────────────────────────────────────────");
        for (i, line) in lines.iter().enumerate() {
            println!("{:4}: {}", i + 1, line);
        }
    }

    std::process::exit(0);
}
