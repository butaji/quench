//! Refresh quench-node's pinned Node source set from a Node checkout.
//!
//! The quench-node build reads Node only through `crates/quench-node/node`
//! (`node_source_set.rs`). This copies exactly that set (the JavaScript library, its
//! bundled dependencies, the headers the build derives constants from, nghttp2 and Node's
//! license) from a checkout, replacing the previous set, and records the checkout's version
//! and commit. Run it from the repository root after moving the `tests/node` pin:
//!
//!   cargo run -p quench-node-test --bin sync-node-sources -- [NODE_CHECKOUT]
//!
//! `NODE_CHECKOUT` defaults to `tests/node`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

include!("../../../quench-node/src/node_source_dependencies.rs");
include!("../../../quench-node/src/node_source_set.rs");

/// The default checkout: the pinned Node submodule.
const DEFAULT_CHECKOUT: &str = "tests/node";
/// The quench-node crate, relative to the repository root.
const QUENCH_NODE_CRATE: &str = "crates/quench-node";
/// The version header and the definitions that make up Node's version.
const VERSION_HEADER: &str = "src/node_version.h";
const VERSION_FIELDS: [&str; 3] = ["NODE_MAJOR_VERSION", "NODE_MINOR_VERSION", "NODE_PATCH_VERSION"];

fn main() -> ExitCode {
    let checkout = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| DEFAULT_CHECKOUT.into()));
    match sync(&checkout, &Path::new(QUENCH_NODE_CRATE).join(NODE_SOURCE_DIR)) {
        Ok(count) => {
            println!("synced {count} files from {}", checkout.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("sync-node-sources: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The source set's files in `checkout`, relative to it, in sorted order.
fn source_files(checkout: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files: Vec<PathBuf> = NODE_SOURCE_FILES
        .iter()
        .chain(std::iter::once(&NODE_SOURCE_LICENSE))
        .map(PathBuf::from)
        .chain(BUNDLED_DEPENDENCIES.iter().map(|(_, path)| PathBuf::from(path)))
        .collect();
    for (tree, extensions) in NODE_SOURCE_TREES {
        let mut pending = vec![checkout.join(tree)];
        while let Some(directory) = pending.pop() {
            let entries = std::fs::read_dir(&directory).map_err(|error| format!("{}: {error}", directory.display()))?;
            for entry in entries {
                let path = entry.map_err(|error| error.to_string())?.path();
                if path.is_dir() {
                    pending.push(path);
                } else if path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extensions.contains(&extension))
                {
                    files.push(path.strip_prefix(checkout).map_err(|error| error.to_string())?.to_path_buf());
                }
            }
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

/// The checkout's `MAJOR.MINOR.PATCH` (`VERSION_HEADER`).
fn node_version(checkout: &Path) -> Result<String, String> {
    let header = std::fs::read_to_string(checkout.join(VERSION_HEADER)).map_err(|error| format!("{VERSION_HEADER}: {error}"))?;
    let field = |name: &str| {
        header
            .lines()
            .find_map(|line| {
                let mut words = line.split_whitespace();
                (words.next() == Some("#define") && words.next() == Some(name)).then(|| words.next()).flatten()
            })
            .ok_or_else(|| format!("{VERSION_HEADER} defines no {name}"))
    };
    Ok(VERSION_FIELDS.iter().map(|name| field(name)).collect::<Result<Vec<_>, _>>()?.join("."))
}

/// The checkout's commit, when it is a Git checkout.
fn checkout_commit(checkout: &Path) -> Option<String> {
    let output = std::process::Command::new("git").arg("-C").arg(checkout).args(["rev-parse", "HEAD"]).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn sync(checkout: &Path, destination: &Path) -> Result<usize, String> {
    let version = node_version(checkout)?;
    let files = source_files(checkout)?;
    if destination.exists() {
        std::fs::remove_dir_all(destination).map_err(|error| format!("{}: {error}", destination.display()))?;
    }
    for file in &files {
        let target = destination.join(file);
        std::fs::create_dir_all(target.parent().expect("a source file has a directory")).map_err(|error| error.to_string())?;
        std::fs::copy(checkout.join(file), &target).map_err(|error| format!("{}: {error}", file.display()))?;
    }
    let commit = checkout_commit(checkout).map_or_else(String::new, |commit| format!("commit {commit}\n"));
    std::fs::write(destination.join(NODE_SOURCE_RECORD), format!("node v{version}\n{commit}"))
        .map_err(|error| error.to_string())?;
    Ok(files.len())
}
