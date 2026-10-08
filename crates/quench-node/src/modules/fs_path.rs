//! Filesystem path normalization shared by both Node execution paths.

/// Node fixtures run with `tests/node` as their cwd, while the Quench runner
/// keeps the repository root as its cwd. Resolve only the fixture-relative
/// `./test/...` spelling when its canonical target exists; application paths
/// retain normal host semantics.
pub(crate) fn resolve_fixture_path(path: String) -> String {
    let Some(suffix) = path.strip_prefix("./test/") else {
        return path;
    };
    let mapped = format!("tests/node/test/{suffix}");
    std::path::Path::new(&mapped)
        .exists()
        .then_some(mapped)
        .unwrap_or(path)
}
