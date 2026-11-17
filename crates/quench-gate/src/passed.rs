//! The pass record: a full gate run that passed on a clean tree adds a note to the commit in
//! `refs/notes/gate` naming its tree, and `--passed` answers whether the checked-out commit has
//! one for its tree. The pre-push hook reads the note, so a protected branch takes only a tree the
//! whole gate passed. The note stays in the clone that ran the gate (the git proxy takes no
//! `refs/notes` pushes): the integrator gates what it pushes and mirrors the tree hash on Linear.

use crate::{config::Config, process, step::Outcome};

/// The notes ref that holds pass records, and the line each record starts with.
const NOTES_REF: &str = "--ref=gate";
const RECORD_PREFIX: &str = "gate pass tree=";

/// The hash of the checked-out tree, when the work tree matches it (tracked files unchanged).
fn clean_tree(config: &Config) -> Result<String, String> {
    let status = process::run(config.command("git").args(["status", "--porcelain", "--untracked-files=no"]))?;
    if !status.success || !status.stdout.trim().is_empty() {
        return Err("the work tree has uncommitted changes to tracked files".into());
    }
    let tree = process::run(config.command("git").args(["rev-parse", "HEAD^{tree}"]))?;
    if tree.success { Ok(tree.stdout.trim().to_owned()) } else { Err(tree.excerpt()) }
}

/// Adds the record of a full run in which every step passed; says why when it adds none.
pub fn record(config: &Config, outcomes: &[Outcome]) -> Result<String, String> {
    if config.quick || !outcomes.iter().all(|outcome| outcome.passed) {
        return Err("only a full run with every step passing is recorded".into());
    }
    let tree = clean_tree(config)?;
    let steps: Vec<&str> = outcomes.iter().map(|outcome| outcome.name).collect();
    let note = format!("{RECORD_PREFIX}{tree} steps={}", steps.join(","));
    let added = process::run(config.command("git").args(["notes", NOTES_REF, "add", "-f", "-m", &note, "HEAD"]))?;
    if added.success { Ok(note) } else { Err(added.excerpt()) }
}

/// Whether the checked-out clean commit has a pass record for its tree.
pub fn check(config: &Config) -> Result<String, String> {
    let tree = clean_tree(config)?;
    let notes = process::run(config.command("git").args(["notes", NOTES_REF, "show", "HEAD"]))?;
    let wanted = format!("{RECORD_PREFIX}{tree} ");
    notes.stdout.lines().find(|line| line.starts_with(&wanted)).map(str::to_owned).ok_or_else(|| format!("no full gate pass is recorded for tree {tree}"))
}
