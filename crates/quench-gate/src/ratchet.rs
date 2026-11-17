//! The known-failure ratchet of a test run: a failure outside its list fails the step, and a
//! listed test seen passing leaves the list, which only shrinks. Each failure is reported with
//! the thread that owns its code (`owners`).

use std::{collections::BTreeSet, fs, io, path::Path};

use crate::{owners, runtime_tests::Tally, step::Verdict};

/// The tests listed in `path`, one name per line.
fn listed(path: &Path) -> io::Result<BTreeSet<String>> {
    Ok(fs::read_to_string(path)?.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_owned).collect())
}

/// Holds `tally` to the list at `path`, removing the listed tests it saw pass; `what` names the run.
pub fn hold(tally: &Tally, path: &Path, what: &str) -> Verdict {
    let listed = listed(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let failed: BTreeSet<&String> = tally.failed.iter().collect();
    let new: Vec<&&String> = failed.iter().filter(|name| !listed.contains(**name)).collect();
    if !new.is_empty() {
        let lines: Vec<String> = new.iter().map(|name| format!("  {name} (owner {})", owners::of(name))).collect();
        return Err(format!("{} {what} tests fail that are not known failures:\n{}", new.len(), lines.join("\n")));
    }
    let passed: BTreeSet<&String> = tally.passed.iter().collect();
    let remaining: Vec<&String> = listed.iter().filter(|name| !passed.contains(name)).collect();
    let fixed = listed.len() - remaining.len();
    if fixed > 0 {
        let text: String = remaining.iter().map(|name| format!("{name}\n")).collect();
        fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(format!("{} {what} tests passed; {} known failures ({fixed} fixed and removed from the list)", tally.passed.len(), remaining.len()))
}
