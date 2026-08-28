//! Runner for the upstream WebAssembly specification testsuite.
//!
//! The testsuite itself lives in the `testsuite/` git submodule. This crate
//! owns filesystem discovery and reporting; execution is delegated to
//! [`quench_wasm::Engine`].

use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestFailure {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct TestReport {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub failures: Vec<TestFailure>,
}

pub struct TestSuite {
    root: PathBuf,
    engine: quench_wasm::Engine,
}

impl TestSuite {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            engine: quench_wasm::Engine::default(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn files(&self) -> impl Iterator<Item = PathBuf> {
        let mut files = walkdir::WalkDir::new(&self.root)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "wast"))
            .map(|entry| entry.into_path())
            .collect::<Vec<_>>();
        files.sort();
        files.into_iter()
    }

    pub fn run_file(&self, path: impl AsRef<Path>) -> Result<(), String> {
        let path = path.as_ref();
        let source = fs::read_to_string(path).map_err(|error| error.to_string())?;
        let filename = path.to_string_lossy();
        self.engine
            .run_wast(&filename, &source)
            .map_err(|error| error.to_string())
    }

    pub fn run_all(&self) -> TestReport {
        let mut report = TestReport::default();
        for path in self.files() {
            report.total += 1;
            match self.run_file(&path) {
                Ok(()) => report.passed += 1,
                Err(reason) => {
                    report.failed += 1;
                    report.failures.push(TestFailure { path, reason });
                }
            }
        }
        report
    }
}

pub fn testsuite_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testsuite")
}

#[cfg(test)]
mod tests {
    use super::TestSuite;

    #[test]
    fn runs_a_small_wast_fixture() {
        let root = tempfile::tempdir().expect("tempdir");
        let path = root.path().join("smoke.wast");
        std::fs::write(
            &path,
            "(module (func (export \"answer\") (result i32) i32.const 42))\n(invoke \"answer\")\n",
        )
        .expect("write");
        let suite = TestSuite::new(root.path());
        assert_eq!(suite.run_all().passed, 1);
    }
}
