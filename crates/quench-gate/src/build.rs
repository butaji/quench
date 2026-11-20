//! Step 1: the workspace binaries the later steps run, built in the experiment profile.

use crate::{config::Config, lint::LINT, process, step::Verdict};

/// The JavaScript host the kernels run on.
pub const NODE: &str = "quench";

/// The test262 runners: the whole suite, and one test.
pub const TEST262_SUITE: &str = "run-all";
pub const TEST262_CASE: &str = "run-test";
/// The WebAssembly spec-suite runner.
pub const WASM_SUITE: &str = "run";

/// The binaries the gate runs: the linter, the JavaScript host, and the conformance runners.
pub const BINARIES: [&str; 5] = [LINT, NODE, TEST262_SUITE, TEST262_CASE, WASM_SUITE];

pub fn run(config: &Config) -> Verdict {
    let mut command = config.cargo("build");
    BINARIES.iter().for_each(|name| {
        command.args(["--bin", name]);
    });
    let built = process::run(&mut command)?;
    if built.success { Ok(format!("{} binaries", BINARIES.len())) } else { Err(built.excerpt()) }
}
