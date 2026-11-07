//! Fails when the repository says the same thing twice (`duplicates`) or decides the same case
//! analysis twice (`decision_trees`), when a function or file outgrows the manifesto's size
//! laws (`shape`), when a hot path hashes, scans or allocates (`hot_path`), when the engine
//! tests a guest program's own names (`guest_names`) or recognizes a function by its bytecode
//! shape (`fingerprints`), when a body spells an unnamed number
//! (`magic`), or when data is laid out against data-oriented design (`data_layout`).
//!
//! usage: quench-lint [--update] [ROOT...]   (default: crates quench-bench/src tools)

mod data_layout;
mod decision_trees;
mod duplicates;
mod fingerprints;
mod guest_names;
mod hot_path;
mod magic;
mod shape;

use std::{
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

const DEFAULT_ROOTS: &[&str] = &["crates", "quench-bench/src", "tools"];
/// Checked-out third-party corpora and build output, not Quench code.
const SKIPPED_DIRECTORIES: &[&str] = &["target", "testsuite", "node_modules", "jetstream", "js-engine-benchmark"];
/// Paths that hold only tests and benchmarks of Quench itself.
const TEST_PATHS: &[&str] = &["test", "primitive_bench"];

/// Whether a function, named by its `file::path` subject, lives in tests or benchmarks.
pub(crate) fn in_test_path(subject: &str) -> bool {
    let file = subject.split("::").next().unwrap_or(subject);
    TEST_PATHS.iter().any(|path| file.contains(path))
}

/// Whether an operator tests equality (`==` or `!=`).
pub(crate) fn is_equality(op: &syn::BinOp) -> bool {
    matches!(op, syn::BinOp::Eq(_) | syn::BinOp::Ne(_))
}

const OVERSIZED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/oversized.txt");
const UPDATE_FLAG: &str = "--update";

fn rust_files(directory: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for path in entries.flatten().map(|entry| entry.path()) {
        let skipped = path.file_name().is_some_and(|name| SKIPPED_DIRECTORIES.iter().any(|skip| name == *skip));
        if path.is_dir() && !skipped {
            rust_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

fn scan(files: &[PathBuf]) -> Result<(duplicates::Duplicates, shape::Measures, decision_trees::DecisionTrees), String> {
    let mut repeated = duplicates::Duplicates::default();
    let mut oversized = shape::Measures::new();
    let mut trees = decision_trees::DecisionTrees::default();
    for path in files {
        let file = path.display().to_string();
        let source = fs::read_to_string(path).map_err(|error| format!("{file}: {error}"))?;
        let syntax =
            syn::parse_file(&source).map_err(|error| format!("{file}:{}: {error}", error.span().start().line))?;
        repeated.scan(&file, &syntax);
        shape::scan(&file, &source, &syntax, &mut oversized, &mut trees);
    }
    Ok((repeated, oversized, trees))
}

fn main() -> ExitCode {
    let (update, roots): (Vec<String>, Vec<String>) = std::env::args().skip(1).partition(|arg| arg == UPDATE_FLAG);
    let roots = if roots.is_empty() { DEFAULT_ROOTS.iter().map(|root| root.to_string()).collect() } else { roots };
    let mut files = Vec::new();
    roots.iter().for_each(|root| rust_files(Path::new(root), &mut files));
    files.sort();
    let (repeated, oversized, trees) = match scan(&files) {
        Ok(found) => found,
        Err(error) => {
            eprintln!("quench-lint: cannot read {error}");
            return ExitCode::FAILURE;
        }
    };
    let recorded = shape::parse(&fs::read_to_string(OVERSIZED).unwrap_or_default());
    let shrunk: shape::Measures = recorded
        .iter()
        .filter_map(|(subject, limit)| Some((subject.clone(), (*oversized.get(subject)?).min(*limit))))
        .collect();
    let departures = shape::compare(if update.is_empty() { &recorded } else { &shrunk }, &oversized);
    if !update.is_empty() && departures == 0 {
        let _ = fs::write(OVERSIZED, shape::render(&oversized));
    }
    let violations = repeated.report() + trees.report() + departures;
    if violations == 0 {
        return ExitCode::SUCCESS;
    }
    eprintln!("quench-lint: {violations} violations in {} files", files.len());
    ExitCode::FAILURE
}
