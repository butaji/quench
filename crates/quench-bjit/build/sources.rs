//! The files whose change must recompile the stencils, read from the include graph: every file
//! the build script or the stencil sources name in a `#[path = "..."]` attribute, transitively,
//! with the directory a module file owns (`numeric.rs` beside `numeric/`) and the stencil directory itself.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

/// The attribute that names a module's file.
const PATH_ATTRIBUTE: &str = "#[path = \"";
/// The files the graph starts from, relative to the crate: the build script and the stencil directory.
const ROOTS: [&str; 2] = ["build.rs", "stencils"];

/// The files and directories to watch, relative to `crate_dir`.
pub fn watched(crate_dir: &Path) -> BTreeSet<PathBuf> {
    let mut watched = BTreeSet::new();
    let mut pending: Vec<PathBuf> = ROOTS.iter().map(|root| crate_dir.join(root)).collect();
    while let Some(path) = pending.pop() {
        if !watched.insert(path.clone()) {
            continue;
        }
        pending.extend(members(&path));
        if let Some(owned) = owned_directory(&path) {
            pending.push(owned);
        }
    }
    watched
}

/// The directory a module file owns: the one named as the file without its extension.
fn owned_directory(file: &Path) -> Option<PathBuf> {
    let owned = file.with_extension("");
    (file.extension().is_some() && owned.is_dir()).then_some(owned)
}

/// What a directory holds, or the files a source file names in `#[path]` attributes.
fn members(path: &Path) -> Vec<PathBuf> {
    if path.is_dir() {
        return fs::read_dir(path).into_iter().flatten().flatten().map(|entry| entry.path()).collect();
    }
    let Ok(source) = fs::read_to_string(path) else { return Vec::new() };
    let base = path.parent().unwrap_or(Path::new("."));
    source
        .lines()
        .filter_map(|line| line.split_once(PATH_ATTRIBUTE))
        .filter_map(|(_, rest)| rest.split_once('"'))
        .map(|(named, _)| normalize(&base.join(named)))
        .collect()
}

/// `path` without its `.` and `..` components, so one file has one name.
fn normalize(path: &Path) -> PathBuf {
    path.components().fold(PathBuf::new(), |mut normal, component| {
        match component {
            std::path::Component::ParentDir if normal.file_name().is_some() => { normal.pop(); }
            std::path::Component::CurDir => {}
            other => normal.push(other),
        }
        normal
    })
}
