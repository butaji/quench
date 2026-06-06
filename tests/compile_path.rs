//! Integration tests for the compile path.
//!
//! These tests invoke `runts build --plugin ratatui` on static examples
//! and verify that the generated binary prints expected output.

use std::process::Command;
use std::time::Duration;

fn runts_bin() -> &'static str {
    option_env!("CARGO_BIN_EXE_runts").unwrap_or("./target/release/runts")
}

fn build_example(name: &str) -> std::process::Output {
    Command::new(runts_bin())
        .args(["build", "--plugin", "ratatui", &format!("examples/{}", name)])
        .env("RUNTS_KEEP_BUILD", "1")
        .output()
        .expect("failed to run runts build")
}

fn run_built_binary(name: &str) -> std::process::Output {
    let bin_path = format!("examples/{}/.runts/build/target/release/runts-app", name);
    Command::new(&bin_path)
        .output()
        .expect("failed to run built binary")
}

#[test]
#[ignore = "slow: runs cargo build in release mode"]
fn compile_ink_text_props() {
    let out = build_example("ink-text-props");
    assert!(
        out.status.success(),
        "runts build failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let run = run_built_binary("ink-text-props");
    assert!(
        run.status.success(),
        "binary exited with error: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(stdout.contains("Text Styling Props"), "missing title: {}", stdout);
    assert!(stdout.contains("Deprecated feature"), "missing text: {}", stdout);
    assert!(stdout.contains("HIGHLIGHTED"), "missing text: {}", stdout);
}

#[test]
#[ignore = "slow: runs cargo build in release mode"]
fn compile_ink_bordered() {
    let out = build_example("ink-bordered");
    assert!(
        out.status.success(),
        "runts build failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let run = run_built_binary("ink-bordered");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(stdout.contains("Bordered Example"), "missing title: {}", stdout);
}

#[test]
#[ignore = "slow: runs cargo build in release mode"]
fn compile_ink_box() {
    let out = build_example("ink-box");
    assert!(
        out.status.success(),
        "runts build failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let run = run_built_binary("ink-box");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(stdout.contains("Box Example") || stdout.contains("Box"), "missing text: {}", stdout);
}

#[test]
#[ignore = "slow: runs cargo build in release mode"]
fn compile_ink_aligned() {
    let out = build_example("ink-aligned");
    assert!(
        out.status.success(),
        "runts build failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let run = run_built_binary("ink-aligned");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(stdout.contains("Centered") || stdout.contains("centered"), "missing text: {}", stdout);
}

#[test]
#[ignore = "slow: runs cargo build in release mode"]
fn compile_ink_background_color() {
    let out = build_example("ink-background-color");
    assert!(
        out.status.success(),
        "runts build failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let run = run_built_binary("ink-background-color");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(stdout.contains("Background") || stdout.contains("background"), "missing text: {}", stdout);
}
