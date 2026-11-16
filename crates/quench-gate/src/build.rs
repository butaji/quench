//! Step 1: the workspace binaries the later steps run, built in the experiment profile.

use crate::{config::Config, lint::LINT, process, step::Verdict};

/// The JavaScript host the kernels run on.
pub const NODE: &str = "quench-node";

/// The binaries the gate runs: the linter, the JavaScript host, and the conformance runners.
pub const BINARIES: [&str; 4] = [LINT, NODE, "run-test", "run"];

pub fn run(config: &Config) -> Verdict {
    let mut command = config.cargo("build");
    BINARIES.iter().for_each(|name| {
        command.args(["--bin", name]);
    });
    let built = process::run(&mut command)?;
    if built.success { Ok(format!("{} binaries", BINARIES.len())) } else { Err(built.excerpt()) }
}
