//! Step 2: the lint ratchets are exact. `quench-lint --lower` lowers each recorded measure
//! that shrank (and drops each that no longer offends) in `oversized.txt`, the only automatic
//! edit of the list, then fails on any measure above its record, any unrecorded one, and any
//! repeated function, constant or decision.

use crate::{config::Config, process, step::Verdict};

pub const LINT: &str = "quench-lint";
const PACKING_REPORT: &str = "--packing";
const PACKING_LINES: &str = "line-packing ";
const PACKING_TOTAL: &str = "line-packing total ";
const LOWERING_FLAG: &str = "--lower";
/// The prefixes `quench-lint --lower` reports each edit of the list under.
const EDITS: [&str; 2] = ["lowered ", "dropped "];

pub fn run(config: &Config) -> Verdict {
    let lint = config.binary(LINT)?;
    let finished = process::run(config.command(lint).arg(LOWERING_FLAG))?;
    if !finished.success {
        return Err(finished.excerpt());
    }
    let edits: Vec<&str> = finished.stdout.lines().filter(|line| EDITS.iter().any(|edit| line.starts_with(edit))).collect();
    let packing = process::run(config.command(config.binary(LINT)?).arg(PACKING_REPORT))?;
    let total = packing.stdout.lines().find_map(|line| line.strip_prefix(PACKING_TOTAL)).unwrap_or_default();
    let mut report = vec![format!("ratchets hold, {} entries lowered; line-packing (reported, not gated): {total} packed lines", edits.len())];
    report.extend(packing.stdout.lines().filter(|line| line.starts_with(PACKING_LINES) && !line.starts_with(PACKING_TOTAL)).map(str::to_owned));
    report.extend(edits.into_iter().map(str::to_owned));
    Ok(report.join("\n"))
}
