//! The files whose change must recompile the stencils, read from the include graph: every file
//! the build script or the stencil sources name in a `#[path = "..."]` attribute, transitively,
//! with the directory a module file owns (`numeric.rs` beside `numeric/`) and the files of the stencil directory.
//! Test files are left out: cargo reruns the build on a change to a watched directory's files, so
//! directories are expanded to their files rather than watched whole.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

/// The attribute that names a module's file.
const PATH_ATTRIBUTE: &str = "#[path = \"";
/// The files the graph starts from, relative to the crate: the build script, the stencil directory and quench-staged's
/// sources (the stencil rows compile against them).
const ROOTS: [&str; 3] = ["build.rs", "stencils", "../quench-staged/src"];

/// The files to watch, relative to `crate_dir`.
pub fn watched(crate_dir: &Path) -> BTreeSet<PathBuf> {
    let mut seen = BTreeSet::new();
    let mut pending: Vec<PathBuf> = ROOTS.iter().map(|root| crate_dir.join(root)).collect();
    while let Some(path) = pending.pop() {
        if is_test(crate_dir, &path) || !seen.insert(path.clone()) {
            continue;
        }
        pending.extend(members(crate_dir, &path));
        pending.extend(owned_directory(&path));
    }
    seen.into_iter().filter(|path| !path.is_dir()).collect()
}

/// Whether a path is a test: a `tests.rs` or `*_tests.rs` file, or anything under a `tests/`
/// or `*_tests/` directory below `crate_dir`.
fn is_test(crate_dir: &Path, path: &Path) -> bool {
    path.strip_prefix(crate_dir).unwrap_or(path).components().any(|part| {
        let part = part.as_os_str().to_string_lossy();
        let stem = part.strip_suffix(".rs").unwrap_or(&part);
        stem == "tests" || stem.ends_with("_tests")
    })
}

/// The directory a module file owns: the one named as the file without its extension.
fn owned_directory(file: &Path) -> Option<PathBuf> {
    let owned = file.with_extension("");
    (file.extension().is_some() && owned.is_dir()).then_some(owned)
}

/// What a directory holds, or the files a source file names in `#[path]` attributes.
fn members(crate_dir: &Path, path: &Path) -> Vec<PathBuf> {
    if path.is_dir() {
        return fs::read_dir(path).into_iter().flatten().flatten().map(|entry| entry.path()).collect();
    }
    let Ok(source) = fs::read_to_string(path) else { return Vec::new() };
    let base = path.parent().unwrap_or(Path::new("."));
    source
        .lines()
        // A comment that quotes the attribute names no file: watching the path it names, which
        // never exists, made every build rerun the stencil compile (270 s of a 296 s rebuild).
        .filter(|line| !line.trim_start().starts_with("//"))
        .filter_map(|line| line.split_once(PATH_ATTRIBUTE))
        .filter_map(|(_, rest)| rest.split_once('"'))
        .map(|(named, _)| normalize(crate_dir, &base.join(named)))
        .collect()
}

/// `path` without its `.` and `..` components, so one file has one name. A `..` that would climb above `crate_dir`
/// stays: quench-staged's files are named `../quench-staged/src/...` as rustc names them (`staged_sources` and the
/// rows' dep-info), and a name that left the crate would make the key differ by checkout.
fn normalize(crate_dir: &Path, path: &Path) -> PathBuf {
    path.components().fold(PathBuf::new(), |mut normal, component| {
        match component {
            std::path::Component::ParentDir if normal.file_name().is_some() && normal.as_path() != crate_dir => { normal.pop(); }
            std::path::Component::CurDir => {}
            other => normal.push(other),
        }
        normal
    })
}
