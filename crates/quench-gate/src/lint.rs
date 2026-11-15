//! Step 2: the lint ratchets are exact. `quench-lint --lower` lowers each recorded measure
//! that shrank (and drops each that no longer offends) in `oversized.txt`, the only automatic
//! edit of the list, then fails on any measure above its record, any unrecorded one, and any
//! repeated function, constant or decision.

use crate::{config::Config, process, step::Verdict};

const LINT: &str = "quench-lint";
const LOWER_FLAG: &str = "--lower";
/// The prefixes `quench-lint --lower` reports each edit of the list under.
const EDITS: [&str; 2] = ["lowered ", "dropped "];

pub fn run(config: &Config) -> Verdict {
    let lint = config.binary(LINT)?;
    let finished = process::run(config.command(lint).arg(LOWER_FLAG))?;
    if !finished.success {
        return Err(finished.excerpt());
    }
    let edits: Vec<&str> = finished.stdout.lines().filter(|line| EDITS.iter().any(|edit| line.starts_with(edit))).collect();
    let mut report = vec![format!("ratchets hold, {} entries lowered", edits.len())];
    report.extend(edits.into_iter().map(str::to_owned));
    Ok(report.join("\n"))
}
