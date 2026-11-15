//! Step 4: oracle and engagement. Every runtime test module that runs a program through the
//! differential oracle (`vm/test_oracle.rs`: the interpreter and a tier must end alike, and
//! the tier's events must show it engaged) must pass, and the selection must match tests.
//! The modules are found by what they import, so a new oracle user joins the gate unasked.

use std::{
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

pub fn run(config: &Config) -> Verdict {
    let sources = config.root.join(SOURCES);
    let filters = filters(&sources).map_err(|error| format!("{}: {error}", sources.display()))?;
    let passed = runtime_tests::run(config, &filters)?;
    Ok(format!("{passed} oracle tests passed in {} modules", filters.len()))
}
