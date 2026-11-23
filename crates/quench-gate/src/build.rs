//! Step 1: the workspace binaries the later steps run, built in the experiment profile, and
//! the AOT-enabled builds (the compiler and the conformance runners) in their own target
//! directory, so the interpreter's binaries are not replaced.

use std::process::Command;

use crate::{config::Config, lint::LINT, process, step::Verdict};

/// The JavaScript host the kernels run on.
pub const NODE: &str = "quench";

/// The test262 runners: the whole suite, and one test.
pub const TEST262_SUITE: &str = "run-all";
pub const TEST262_CASE: &str = "run-test";
/// The WebAssembly spec-suite runner.
pub const WASM_SUITE: &str = "run";
/// The packages holding the conformance runners.
pub const TEST262_PACKAGE: &str = "quench-test262";
pub const WASM_PACKAGE: &str = "quench-wasm-test";

/// The binaries the gate runs: the linter, the JavaScript host, and the conformance runners.
pub const BINARIES: [&str; 5] = [LINT, NODE, TEST262_SUITE, TEST262_CASE, WASM_SUITE];
/// The directory, under the target directory, the AOT-enabled binaries are built into.
pub const AOT_TARGET: &str = "aot";

/// `cargo build` of `packages` with the `aot` feature, into the AOT target directory.
fn aot_build(config: &Config, packages: &[&str], bins: &[&str]) -> Command {
    let mut command = config.cargo("build");
    for package in packages {
        command.args(["-p", *package]);
    }
    command.args(["--features", "aot", "--target-dir"]).arg(config.target.join(AOT_TARGET));
    for bin in bins {
        command.args(["--bin", *bin]);
    }
    command
}

/// Runs `command` to success, or reports the end of its output.
fn build(mut command: Command) -> Result<(), String> {
    let built = process::run(&mut command)?;
    if built.success { Ok(()) } else { Err(built.excerpt()) }
}

pub fn run(config: &Config) -> Verdict {
    let mut plain = config.cargo("build");
    BINARIES.iter().for_each(|name| {
        plain.args(["--bin", name]);
    });
    build(plain)?;
    // One AOT build for all three packages: built apart, each resolved its own feature set for
    // quench-runtime, so the runtime compiled twice more (about 25 min each at --jobs 1).
    build(aot_build(config, &[NODE, TEST262_PACKAGE, WASM_PACKAGE], &[NODE, TEST262_SUITE, WASM_SUITE]))?;
    Ok(format!("{} binaries, and the AOT builds", BINARIES.len()))
}
