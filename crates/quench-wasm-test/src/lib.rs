//! Runner for the upstream WebAssembly specification testsuite.
//!
//! The testsuite lives in the `testsuite/` git submodule. This crate owns
//! filesystem discovery and reporting. Every `.wast` under the tree is walked,
//! including `proposals/`. Execution and scoring use the shared VM.

use std::{
    fs,
    path::{Path, PathBuf},
};

pub use quench_wasm::{DirectiveResult, WastReport};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestFailure {
    pub path: PathBuf,
    pub line: usize,
    pub directive: String,
    pub expected: String,
    pub got: String,
}

impl TestFailure {
    pub fn format_line(&self) -> String {
        format!(
            "{}:{} {}: expected {}; got {}",
            self.path.display(),
            self.line,
            self.directive,
            self.expected,
            self.got
        )
    }
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
            engine: quench_wasm::Engine::new(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn files(&self) -> impl Iterator<Item = PathBuf> {
        let mut files = walkdir::WalkDir::new(&self.root)
            .into_iter()
            .map(|entry| entry.expect("Wasm suite discovery failed"))
            .filter(|entry| entry.file_type().is_file())
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "wast"))
            .map(|entry| entry.into_path())
            .collect::<Vec<_>>();
        files.sort();
        files.into_iter()
    }

    pub fn run_file(&self, path: impl AsRef<Path>) -> WastReport {
        let path = path.as_ref();
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(error) => {
                return WastReport {
                    results: vec![DirectiveResult {
                        line: 1,
                        kind: "wast".to_string(),
                        passed: false,
                        expected: "read".to_string(),
                        got: error.to_string(),
                    }],
                };
            }
        };
        let filename = path.to_string_lossy();
        self.engine.run_wast(&filename, &source)
    }

    pub fn run_all(&self) -> TestReport {
        let mut report = TestReport::default();
        for path in self.files() {
            let file_report = self.run_file(&path);
            for result in file_report.results {
                report.total += 1;
                if result.passed {
                    report.passed += 1;
                } else {
                    report.failed += 1;
                    report.failures.push(TestFailure {
                        path: path.clone(),
                        line: result.line,
                        directive: result.kind,
                        expected: result.expected,
                        got: result.got,
                    });
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
    use super::{testsuite_root, TestSuite};

    #[test]
    fn scores_fixture_directives_not_files() {
        let root = tempfile::tempdir().expect("tempdir");
        let path = root.path().join("smoke.wast");
        std::fs::write(
            &path,
            r#"
(assert_malformed (module binary "") "unexpected end")
(assert_invalid
  (module (func (unreachable) (drop (local.get 0))))
  "unknown local")
(module (func (export "answer") (result i32) i32.const 42))
(assert_return (invoke "answer") (i32.const 42))
(invoke "answer")
"#,
        )
        .expect("write");
        let suite = TestSuite::new(root.path());
        let report = suite.run_all();
        assert_eq!(report.total, 5, "{report:?}");
        assert_eq!(report.passed, 5, "{report:?}");
        assert_eq!(report.failed, 0, "{report:?}");
    }

    #[test]
    fn names_and_legacy_are_scored_as_directives() {
        let root = testsuite_root();
        let suite = TestSuite::new(&root);
        for rel in [
            "names.wast",
            "legacy/try_catch.wast",
            "legacy/throw.wast",
            "legacy/rethrow.wast",
            "legacy/try_delegate.wast",
        ] {
            let report = suite.run_file(root.join(rel));
            assert!(
                report.results.len() > 1,
                "{rel} should parse into directives, got {:?}",
                report.results
            );
            assert!(
                report.results.iter().all(|r| r.kind != "wast"),
                "{rel} was a single wast-parse failure: {:?}",
                report.results
            );
        }
    }

    #[test]
    fn proposals_are_walked_not_omitted() {
        let root = testsuite_root();
        let suite = TestSuite::new(&root);
        let proposals: Vec<_> = suite
            .files()
            .filter(|p| p.starts_with(root.join("proposals")))
            .collect();
        assert!(
            !proposals.is_empty(),
            "no proposals/ wast files discovered under {}",
            root.display()
        );
        let sample = root.join("proposals/wide-arithmetic/wide-arithmetic.wast");
        assert!(
            proposals.contains(&sample),
            "missing {sample:?} in {proposals:?}"
        );
    }
}
