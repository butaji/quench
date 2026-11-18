//! Step: the known-failure lists only shrink. `ratchet::hold` removes a listed test once it
//! passes, but nothing stopped a commit from adding a name by hand to turn a new failure green.
//! This step compares each list with the same file on trunk and fails on any name trunk does not
//! list, so a new failure must be fixed, never accepted.

use std::collections::BTreeSet;

use crate::{config::Config, process, step::Verdict};

/// The lists held to shrink-only, relative to the repository root.
pub const LISTS: [&str; 2] = ["crates/quench-gate/known_failures.txt", "crates/quench-gate/known_debug_failures.txt"];
/// The branch every change lands through; its lists are the ceiling.
const TRUNK: &str = "origin/v2-merged";

/// The names in a list's text, one per line.
pub fn names(text: &str) -> BTreeSet<&str> {
    text.lines().map(str::trim).filter(|line| !line.is_empty()).collect()
}

/// The names `current` lists that `ceiling` does not.
pub fn added<'a>(ceiling: &str, current: &'a str) -> Vec<&'a str> {
    let ceiling = names(ceiling);
    names(current).into_iter().filter(|name| !ceiling.contains(name)).collect()
}

pub fn run(config: &Config) -> Verdict {
    let mut report = Vec::new();
    let mut total = 0;
    for list in LISTS {
        let current = std::fs::read_to_string(config.root.join(list)).map_err(|error| format!("{list}: {error}"))?;
        total += names(&current).len();
        let shown = process::run(config.command("git").args(["show", &format!("{TRUNK}:{list}")]))?;
        if !shown.success {
            return Ok(format!("{TRUNK} is not fetched; lists unchecked ({total}+ known failures)"));
        }
        let ceiling = shown.stdout;
        report.extend(added(&ceiling, &current).into_iter().map(|name| format!("  {list}: {name}")));
    }
    if report.is_empty() {
        return Ok(format!("{total} known failures, none added relative to {TRUNK}"));
    }
    Err(format!("{} names were added to the known-failure lists; fix the failures instead:\n{}", report.len(), report.join("\n")))
}

#[cfg(test)]
mod tests {
    use super::added;

    #[test]
    fn a_name_missing_from_the_ceiling_is_an_addition_and_a_removal_is_not() {
        assert_eq!(added("a\nb\n", "b\nc\n"), vec!["c"]);
        assert!(added("a\nb\n", "a\n").is_empty());
    }
}
