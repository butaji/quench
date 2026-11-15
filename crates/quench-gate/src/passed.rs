//! The pass record: a full gate run that passed on a clean tree leaves a file named by that
//! tree's hash, and `--passed` answers whether the checked-out tree has one. The pre-push hook
//! reads it, so a branch reaches the integration branch only as a tree the whole gate passed.

use std::{fs, path::PathBuf};

use crate::{config::Config, process, step::Outcome};

/// Where pass records live, under the target directory.
const RECORDS: &str = "quench-gate/passed";

/// The hash of the checked-out tree, when the work tree matches it (tracked files unchanged).
fn clean_tree(config: &Config) -> Result<String, String> {
    let status = process::run(config.command("git").args(["status", "--porcelain", "--untracked-files=no"]))?;
    if !status.success || !status.stdout.trim().is_empty() {
        return Err("the work tree has uncommitted changes to tracked files".into());
    }
    let tree = process::run(config.command("git").args(["rev-parse", "HEAD^{tree}"]))?;
    if tree.success { Ok(tree.stdout.trim().to_owned()) } else { Err(tree.excerpt()) }
}

fn record_path(config: &Config, tree: &str) -> PathBuf {
    config.target.join(RECORDS).join(tree)
}

/// Writes the record of a full run in which every step passed; says why when it writes none.
pub fn record(config: &Config, outcomes: &[Outcome]) -> Result<PathBuf, String> {
    if config.quick || !outcomes.iter().all(|outcome| outcome.passed) {
        return Err("only a full run with every step passing is recorded".into());
    }
    let path = record_path(config, &clean_tree(config)?);
    let summary: String = outcomes.iter().map(|outcome| format!("{} {}\n", outcome.name, outcome.detail.lines().next().unwrap_or_default())).collect();
    fs::create_dir_all(path.parent().unwrap_or(&config.target)).and_then(|()| fs::write(&path, summary)).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    Ok(path)
}

/// Whether the checked-out clean tree has a pass record.
pub fn check(config: &Config) -> Result<PathBuf, String> {
    let path = record_path(config, &clean_tree(config)?);
    if path.exists() { Ok(path) } else { Err(format!("no full gate pass is recorded for this tree ({})", path.display())) }
}
