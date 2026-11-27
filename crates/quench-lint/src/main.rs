//! Fails when the repository says the same thing twice (`duplicates`) or decides the same case
//! analysis twice (`decision_trees`), when a function or file outgrows the manifesto's size
//! laws (`shape`), when a hot path hashes, scans or allocates (`hot_path`), when the engine
//! tests a guest program's own names (`guest_names`) or recognizes a function by its bytecode
//! shape (`fingerprints`), when an engine concept gains a second definition or an analysis a
//! second run (`concepts`), when a body spells an unnamed number
//! (`magic`), when data is laid out against data-oriented design (`data_layout`), or when
//! code tests a function's address for identity (`fn_identity`), when a fast path carries its
//! slow path inline (`fast_paths`), when the opcode table is spelled outside its one home
//! (`opcode_table`), or when statements are packed
//! onto long lines (`line_packing`, reported by `--packing`, never gating).
//!
//! usage: quench-lint [--update | --lower] [--packing] [ROOT...]   |   quench-lint --census [--update-census]
//! (default roots: crates quench-bench/src tools; `--lower` reads the whole tree only)

mod census;
mod concepts;
mod census_metrics;
mod data_layout;
mod decision_trees;
mod duplicates;
mod fast_paths;
mod fn_identity;
mod fingerprints;
mod guest_names;
mod hot_path;
mod line_packing;
mod magic;
mod native_forms;
mod opcode_table;
mod shape;
mod test_paths;

pub(crate) use test_paths::in_test_path;

use std::{
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

const DEFAULT_ROOTS: &[&str] = &["crates", "quench-bench/src", "tools"];
/// Checked-out third-party corpora and build output, not Quench code.
const SKIPPED_DIRECTORIES: &[&str] = &["target", "testsuite", "node_modules", "jetstream", "js-engine-benchmark"];

/// Whether an operator tests equality (`==` or `!=`).
pub(crate) fn is_equality(op: &syn::BinOp) -> bool {
    matches!(op, syn::BinOp::Eq(_) | syn::BinOp::Ne(_))
}

const OVERSIZED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/oversized.txt");
const UPDATE_FLAG: &str = "--update";
const LOWER_FLAG: &str = "--lower";
const PACKING_FLAG: &str = "--packing";
const FLAG_PREFIX: &str = "--";

/// What a run was asked: how it may rewrite the recorded list, and the roots it reads.
struct Request {
    update: bool,
    lower: bool,
    packing: bool,
    roots: Vec<String>,
}

impl Request {
    fn parse(args: Vec<String>) -> Result<Self, String> {
        let (flags, roots): (Vec<String>, Vec<String>) = args.into_iter().partition(|arg| arg.starts_with(FLAG_PREFIX));
        let asked = |flag: &str| flags.iter().any(|candidate| candidate == flag);
        let (update, lower, packing) = (asked(UPDATE_FLAG), asked(LOWER_FLAG), asked(PACKING_FLAG));
        if lower && !roots.is_empty() {
            return Err(format!("{LOWER_FLAG} reads the whole tree: give no ROOT"));
        }
        let roots = if roots.is_empty() { DEFAULT_ROOTS.iter().map(|root| root.to_string()).collect() } else { roots };
        Ok(Self { update, lower, packing, roots })
    }
}

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

fn scan(files: &[PathBuf], packed: &mut shape::Measures) -> Result<(duplicates::Duplicates, shape::Measures, decision_trees::DecisionTrees), String> {
    let mut repeated = duplicates::Duplicates::default();
    let mut oversized = shape::Measures::new();
    let mut trees = decision_trees::DecisionTrees::default();
    for path in files {
        let file = path.display().to_string();
        let source = fs::read_to_string(path).map_err(|error| format!("{file}: {error}"))?;
        let syntax =
            syn::parse_file(&source).map_err(|error| format!("{file}:{}: {error}", error.span().start().line))?;
        repeated.scan(&file, &syntax);
        opcode_table::scan(&file, &source, &mut oversized);
        line_packing::scan(&file, &source, &syntax, packed);
        shape::scan(&file, &source, &syntax, &mut oversized, &mut trees);
    }
    Ok((repeated, oversized, trees))
}

/// `listing` with the entries that shrank or no longer offend lowered, written back to the list;
/// the listing as it was if it needs no lowering or cannot be written.
fn lower(listing: String, current: &shape::Measures) -> String {
    let (lowered, changes) = shape::lower(&listing, current);
    if changes.is_empty() {
        return listing;
    }
    changes.iter().for_each(|change| println!("{change}"));
    match fs::write(OVERSIZED, &lowered) {
        Ok(()) => lowered,
        Err(error) => {
            eprintln!("quench-lint: cannot write {OVERSIZED}: {error}");
            listing
        }
    }
}

/// Compares the measures with the recorded list, rewritten first as the request allows;
/// returns the departures.
fn settle(oversized: &shape::Measures, request: &Request) -> usize {
    let mut listing = fs::read_to_string(OVERSIZED).unwrap_or_default();
    if request.lower {
        listing = lower(listing, oversized);
    }
    // A guest-name test and a stray opcode table have no allowance: `oversized.txt` cannot grandfather one.
    let unallowed = [guest_names::MEASURE, opcode_table::MEASURE];
    let recorded: shape::Measures = shape::parse(&listing)
        .into_iter()
        .filter(|(subject, _)| !unallowed.iter().any(|measure| subject.starts_with(measure)))
        .collect();
    let mut shrunk: shape::Measures = recorded
        .iter()
        .filter_map(|(subject, limit)| Some((subject.clone(), (*oversized.get(subject)?).min(*limit))))
        .collect();
    shape::first_baseline(oversized, &recorded, &mut shrunk);
    let departures = shape::compare(if request.update { &shrunk } else { &recorded }, oversized);
    if request.update && departures == 0 {
        let _ = fs::write(OVERSIZED, shape::render(oversized));
    }
    departures
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == census::CENSUS_FLAG) {
        return census::run(&args);
    }
    let request = match Request::parse(args) {
        Ok(request) => request,
        Err(error) => {
            eprintln!("quench-lint: {error}");
            return ExitCode::FAILURE;
        }
    };
    let mut files = Vec::new();
    request.roots.iter().for_each(|root| rust_files(Path::new(root), &mut files));
    files.sort();
    test_paths::declare(&files);
    let mut packed = shape::Measures::new();
    let (repeated, mut oversized, trees) = match scan(&files, &mut packed) {
        Ok(found) => found,
        Err(error) => {
            eprintln!("quench-lint: cannot read {error}");
            return ExitCode::FAILURE;
        }
    };
    native_forms::scan(&files, &mut oversized);
    concepts::scan(&files, &mut oversized);
    if request.packing {
        line_packing::report(&packed);
    }
    let violations = repeated.report() + trees.report() + settle(&oversized, &request);
    if violations == 0 {
        return ExitCode::SUCCESS;
    }
    eprintln!("quench-lint: {violations} violations in {} files", files.len());
    ExitCode::FAILURE
}
