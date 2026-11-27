//! Step: the trained link order (`crates/quench/link/x86_64-linux.order`) still names the
//! binary's functions. It names them with crate hashes; when those move, lld silently ignores the
//! stale names (richards' no-jit text then spread over 2.6 MB more pages). Below 90% present,
//! retrain with `tools/link-order.sh`.

use crate::{build::NODE, config::Config, process, step::Verdict};

const CHECK: &str = "tools/check-link-order.sh";
const ORDER: &str = "crates/quench/link/x86_64-linux.order";

pub fn run(config: &Config) -> Verdict {
    let binary = config.binary(NODE)?;
    let check = process::run(config.command(CHECK).arg(binary).arg(ORDER))?;
    if !check.success {
        return Err(check.excerpt());
    }
    Ok(check.stdout.trim().to_owned())
}
