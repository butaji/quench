//! Which files hold only tests and benchmarks of Quench itself, which the measures that
//! police production code leave out: a file in a `tests` directory or a directory named
//! `*_tests`, a `tests.rs` or `*_tests.rs` file, a benchmark module, a test-only crate, and a
//! module a `#[cfg(test)] mod name;` declaration names with everything below it. A path that
//! merely contains the word (a `test_runner` module of the product) is production code.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

const TESTS: &str = "tests";
const TESTS_SUFFIX: &str = "_tests";
const BENCHMARK: &str = "primitive_bench";
/// Crates that exist only to test or measure Quench.
const TEST_CRATES: &[&str] = &["quench-test262", "quench-node-test", "quench-wasm-test"];
const RUST_SUFFIX: &str = ".rs";
const RUST_EXTENSION: &str = "rs";
const MODULE_FILE: &str = "mod.rs";
/// File stems whose modules live beside the file, not in a directory named for it.
const MODULE_ROOTS: &[&str] = &["mod", "lib", "main"];
/// The attributes that gate the item after them to test builds.
const TEST_GATES: &[&str] = &["#[cfg(test)]", "#[cfg(all(test"];
const ATTRIBUTE: &str = "#[";
const COMMENT: &str = "//";

/// The files the `#[cfg(test)] mod name;` declarations of the scanned files name.
static DECLARED: OnceLock<BTreeSet<PathBuf>> = OnceLock::new();

/// Reads the test-gated module declarations of `files`; call once, before any
/// [`in_test_path`] question that should know them.
pub fn declare(files: &[PathBuf]) {
    let declared: BTreeSet<PathBuf> = files
        .iter()
        .filter_map(|path| Some((path, fs::read_to_string(path).ok()?)))
        .flat_map(|(path, source)| {
            gated_modules(&source).into_iter().flat_map(|name| module_files(path, name)).collect::<Vec<PathBuf>>()
        })
        .collect();
    let _ = DECLARED.set(declared);
}

/// Whether a function's `file::path` subject lives in tests or benchmarks.
pub fn in_test_path(subject: &str) -> bool {
    let file = Path::new(subject.split("::").next().unwrap_or(subject));
    named_as_test(file) || DECLARED.get().is_some_and(|declared| declared_below(declared, file))
}

/// Whether a file's own path says it holds only tests or benchmarks.
pub fn named_as_test(file: &Path) -> bool {
    file.components()
        .filter_map(|part| part.as_os_str().to_str())
        .map(|part| part.strip_suffix(RUST_SUFFIX).unwrap_or(part))
        .any(|part| part == TESTS || part == BENCHMARK || part.ends_with(TESTS_SUFFIX) || TEST_CRATES.contains(&part))
}

/// Whether `file` is a declared test module or lies below one.
fn declared_below(declared: &BTreeSet<PathBuf>, file: &Path) -> bool {
    file.ancestors().any(|ancestor| {
        declared.contains(ancestor)
            || declared.contains(&ancestor.with_extension(RUST_EXTENSION))
            || declared.contains(&ancestor.join(MODULE_FILE))
    })
}

/// The names of the out-of-line modules `source` gates to test builds.
fn gated_modules(source: &str) -> Vec<&str> {
    let mut gated = false;
    let mut found = Vec::new();
    for line in source.lines().map(str::trim) {
        let gate = TEST_GATES.iter().any(|gate| line.starts_with(*gate));
        gated |= gate;
        let item = if gate { line.split_once(']').map_or("", |(_, rest)| rest.trim()) } else { line };
        if !gated || item.is_empty() || item.starts_with(COMMENT) || item.starts_with(ATTRIBUTE) {
            continue;
        }
        found.extend(module_name(item));
        gated = false;
    }
    found
}

/// The name of the out-of-line module `item` declares (`mod name;`, with any visibility).
fn module_name(item: &str) -> Option<&str> {
    let item = item.trim_start_matches("pub").trim_start();
    let item = item.strip_prefix('(').and_then(|rest| rest.split_once(')')).map_or(item, |(_, rest)| rest.trim_start());
    let (name, _) = item.strip_prefix("mod ")?.split_once(';')?;
    let name = name.trim();
    name.chars().all(|letter| letter.is_alphanumeric() || letter == '_').then_some(name)
}

/// The files module `name` of `declaring` may be: `name.rs` or `name/mod.rs` beside it or in its
/// directory.
fn module_files(declaring: &Path, name: &str) -> [PathBuf; 2] {
    let root = declaring.file_stem().and_then(|stem| stem.to_str()).is_some_and(|stem| MODULE_ROOTS.contains(&stem));
    let directory = if root { declaring.parent().map(Path::to_path_buf).unwrap_or_default() } else { declaring.with_extension("") };
    [directory.join(format!("{name}{RUST_SUFFIX}")), directory.join(name).join(MODULE_FILE)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_a_test_path_by_a_test_directory_or_file_name_not_a_substring() {
        for test in ["crates/a/src/tests/x.rs", "crates/a/src/x_tests.rs", "crates/a/src/subset_tests/p.rs", "crates/quench-test262/src/m.rs", "a/tests.rs::f"] {
            assert!(in_test_path(test), "{test}");
        }
        for production in ["crates/quench/src/modules/test_runner/run.rs::drive", "crates/a/src/latest.rs", "crates/a/src/test_host.rs"] {
            assert!(!in_test_path(production), "{production}");
        }
    }

    #[test]
    fn gated_module_declarations_are_found_with_their_visibility_and_attributes() {
        let source = "#[cfg(test)]\nmod a;\n#[cfg(test)] pub(crate) mod b;\n#[cfg(all(test, x))]\n// why\nmod c;\n#[cfg(test)]\nmod d {}\nmod e;\n";
        assert_eq!(gated_modules(source), ["a", "b", "c"]);
    }

    #[test]
    fn a_declared_module_and_what_lies_below_it_are_test_files() {
        let declared: BTreeSet<PathBuf> = module_files(Path::new("crates/a/src/lib.rs"), "test_host")
            .into_iter()
            .chain(module_files(Path::new("crates/a/src/staged.rs"), "words"))
            .collect();
        assert!(declared_below(&declared, Path::new("crates/a/src/test_host.rs")));
        assert!(declared_below(&declared, Path::new("crates/a/src/test_host/inner.rs")));
        assert!(declared_below(&declared, Path::new("crates/a/src/staged/words.rs")));
        assert!(!declared_below(&declared, Path::new("crates/a/src/staged.rs")));
    }
}
