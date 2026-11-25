//! Step `fallbacks`: the expensive fallbacks (`quench_runtime::fallback`) the corpus reaches may
//! only go away. Each corpus program runs under every compiling mode with
//! `QUENCH_DEV=strict-fallback=report,sync-compile`; the distinct fallbacks, one line per
//! `kind operation (reason)`, are held to `crates/quench-tier-corpus/fallbacks.txt`. A fallback the
//! file does not list fails the step (fix it, or it is a regression); a listed one the corpus no
//! longer reaches fails too, with the line to delete, so the list only shrinks. `quench-lint
//! --census` counts the file's lines per kind, and no count may rise. With `QUENCH_DEBT_WRITE=1`
//! the step rewrites the file with what it found, refusing to add a line.
//!
//! The corpus is the C-parity kernels (`crates/quench-tier-corpus/cparity/js`) and every `.js`
//! file in the directories `QUENCH_FALLBACK_CORPUS` names (colon-separated), such as the V8-v7
//! files. Alone: `quench-gate --only fallbacks`.

use std::{collections::BTreeSet, env, fs, path::PathBuf};

use crate::{build::NODE, config::Config, process, step::Verdict};

/// The offender list, one `kind operation (reason)` per line, sorted.
pub const LIST: &str = "crates/quench-tier-corpus/fallbacks.txt";
const KERNELS: &str = "crates/quench-tier-corpus/cparity/js";
const EXTRA_CORPUS: &str = "QUENCH_FALLBACK_CORPUS";
const DEBT_WRITE: &str = "QUENCH_DEBT_WRITE";
/// The modes that compile; `--no-jit` runs no compiled code, so it has no fallback.
const MODES: [Option<&str>; 4] = [None, Some("--b-jit"), Some("--q-jit"), Some("--u-jit")];
const DEV: &str = "strict-fallback=report,sync-compile";
const REPORT_PREFIX: &str = "QUENCH_FALLBACK ";
/// A corpus run longer than this fails the step.
const RUN_SECONDS: u32 = 300;
const HEADER: &str = "# Expensive fallbacks the corpus reaches (quench-gate step `fallbacks`, QUENCH_DEV=strict-fallback=report).\n\
# One `kind operation (reason)` per line. The list may only shrink; quench-lint --census counts it per kind.\n";

fn corpus(config: &Config) -> Result<Vec<PathBuf>, String> {
    let mut directories = vec![config.root.join(KERNELS)];
    if let Some(extra) = env::var_os(EXTRA_CORPUS) {
        directories.extend(env::split_paths(&extra));
    }
    let mut files = Vec::new();
    for directory in directories {
        let entries = fs::read_dir(&directory).map_err(|error| format!("{}: {error}", directory.display()))?;
        files.extend(entries.flatten().map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|extension| extension == "js")));
    }
    files.sort();
    Ok(files)
}

/// The offender of a report line: `kind operation (reason)`, without its location and count.
pub fn offender(line: &str) -> Option<String> {
    let rest = line.strip_prefix(REPORT_PREFIX)?;
    let close = rest.rfind(") ")?;
    Some(rest[..=close].to_owned())
}

fn listed(text: &str) -> BTreeSet<String> {
    text.lines().filter(|line| !line.is_empty() && !line.starts_with('#')).map(str::to_owned).collect()
}

pub fn run(config: &Config) -> Verdict {
    let binary = config.binary(NODE)?;
    let mut found = BTreeSet::new();
    let mut failures = Vec::new();
    for file in corpus(config)? {
        for mode in MODES {
            let mut command = config.command(&binary);
            command.env("QUENCH_DEV", DEV).env_remove("NODE_OPTIONS").env_remove("NODE_PATH");
            command.args(mode).arg(&file);
            let finished = process::run_within(&command, RUN_SECONDS)?;
            if !finished.success {
                failures.push(format!("{} {}: {}", file.display(), mode.unwrap_or("default"), finished.excerpt()));
            }
            found.extend(finished.stderr.lines().filter_map(offender));
        }
    }
    if !failures.is_empty() {
        return Err(failures.join("\n"));
    }
    let path = config.root.join(LIST);
    let recorded = listed(&fs::read_to_string(&path).unwrap_or_default());
    let new: Vec<_> = found.difference(&recorded).collect();
    let gone: Vec<_> = recorded.difference(&found).collect();
    if env::var_os(DEBT_WRITE).is_some_and(|value| value == "1") && (new.is_empty() || recorded.is_empty()) {
        let body: String = found.iter().map(|line| format!("{line}\n")).collect();
        fs::write(&path, format!("{HEADER}{body}")).map_err(|error| format!("{}: {error}", path.display()))?;
        return Ok(format!("{} fallbacks written ({} gone)", found.len(), gone.len()));
    }
    let mut problems: Vec<String> = new.iter().map(|line| format!("new fallback (fix it): {line}")).collect();
    problems.extend(gone.iter().map(|line| format!("fixed, delete the line (or QUENCH_DEBT_WRITE=1): {line}")));
    if problems.is_empty() { Ok(format!("{} fallbacks, none new", found.len())) } else { Err(problems.join("\n")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offender_drops_location_and_count() {
        assert_eq!(
            offender("QUENCH_FALLBACK deopt GetField (left, shape) bench/a.js:12:5 40").as_deref(),
            Some("deopt GetField (left, shape)")
        );
        assert_eq!(offender("QUENCH_FALLBACK jit-reject ? (no-unit) a.js f3@0 1").as_deref(), Some("jit-reject ? (no-unit)"));
        assert_eq!(offender("something else"), None);
    }
}
