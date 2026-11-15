//! Step 4: oracle and engagement. Every runtime test module that runs a program through the
//! differential oracle (`vm/test_oracle.rs`: the interpreter and a tier must end alike, and
//! the tier's events must show it engaged) must pass, and the selection must match tests.
//! The modules are found by what they import, so a new oracle user joins the gate unasked.

use std::{
    collections::BTreeSet,
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

use crate::{config::Config, runtime_tests, step::Verdict};

const SOURCES: &str = "crates/quench-runtime/src";
const ORACLE_IMPORT: &str = "test_oracle::";
const SOURCE_EXTENSION: &str = "rs";
const MODULE_FILE: &str = "mod";
const PATH_SEPARATOR: &str = "::";

fn rust_files(directory: &Path, found: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            rust_files(&path, found)?;
        } else if path.extension().is_some_and(|extension| extension == SOURCE_EXTENSION) {
            found.push(path);
        }
    }
    Ok(())
}

/// The path of the module `file` defines, below the crate's `sources` directory.
fn module_path(sources: &Path, file: &Path) -> Option<String> {
    let relative = file.strip_prefix(sources).ok()?.with_extension("");
    let mut parts: Vec<&str> = relative.iter().filter_map(OsStr::to_str).collect();
    if parts.last() == Some(&MODULE_FILE) {
        parts.pop();
    }
    Some(parts.join(PATH_SEPARATOR))
}

/// The test filters of the modules under `sources` that import the oracle, one per module.
fn filters(sources: &Path) -> io::Result<Vec<String>> {
    let mut files = Vec::new();
    rust_files(sources, &mut files)?;
    files.sort();
    let mut found = Vec::new();
    for file in files {
        if fs::read_to_string(&file)?.contains(ORACLE_IMPORT) {
            found.extend(module_path(sources, &file).map(|path| format!("{path}{PATH_SEPARATOR}")));
        }
    }
    Ok(found)
}

/// The oracle tests expected to fail until their cause is fixed, one name per line.
const KNOWN_FAILURES: &str = "crates/quench-gate/known_failures.txt";

/// The known failures, sorted by name.
fn known(path: &Path) -> io::Result<BTreeSet<String>> {
    Ok(fs::read_to_string(path)?.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_owned).collect())
}

/// The ratchet: a failure outside the list fails the step, and a listed test that no longer
/// fails leaves the list, which only shrinks.
pub fn run(config: &Config) -> Verdict {
    let sources = config.root.join(SOURCES);
    let filters = filters(&sources).map_err(|error| format!("{}: {error}", sources.display()))?;
    let tally = runtime_tests::tally(config, &filters)?;
    let path = config.root.join(KNOWN_FAILURES);
    let listed = known(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let failed: BTreeSet<String> = tally.failed.into_iter().collect();
    let new: Vec<&String> = failed.difference(&listed).collect();
    if !new.is_empty() {
        return Err(format!("{} oracle tests fail that are not known failures:\n{}", new.len(), new.iter().map(|name| format!("  {name}")).collect::<Vec<_>>().join("\n")));
    }
    let remaining: Vec<&String> = listed.intersection(&failed).collect();
    let fixed = listed.len() - remaining.len();
    if fixed > 0 {
        let text: String = remaining.iter().map(|name| format!("{name}\n")).collect();
        fs::write(&path, text).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(format!("{} oracle tests passed in {} modules; {} known failures ({fixed} fixed and removed from the list)", tally.passed.len(), filters.len(), remaining.len()))
}
