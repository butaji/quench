//! Standalone ECMA-262 conformance runner boundary.
//!
//! This crate owns runner outcomes and dispatch. The engine is accessed only
//! through the [`Test262Host`] boundary. Engine implementation details remain
//! outside this crate.

use std::{collections::HashMap, path::Path};

pub mod runtime_host;
pub use runtime_host::RuntimeHost;

/// Exact harness sources cached by filename to avoid repeated filesystem I/O.
pub struct HarnessCache {
    root: std::path::PathBuf,
    sources: HashMap<String, String>,
}

impl HarnessCache {
    pub fn new(root: std::path::PathBuf) -> Self {
        Self {
            root,
            sources: HashMap::new(),
        }
    }

    pub fn load(&mut self, name: &str) -> Result<String, String> {
        if let Some(source) = self.sources.get(name) {
            return Ok(source.clone());
        }
        let source = std::fs::read_to_string(self.root.join(name))
            .map_err(|error| format!("harness {name}: {error}"))?;
        self.sources.insert(name.to_string(), source.clone());
        Ok(source)
    }
}

/// Engine-facing execution contract for an external conformance runner.
pub trait Test262Host: Send {
    /// Execute a complete script source.
    fn run_script(&mut self, source: &str) -> Result<(), String>;

    /// Execute a complete ES module source.
    fn run_module_script(&mut self, source: &str) -> Result<(), String>;

    /// Execute harness scripts, then one test script in the same realm.
    fn run_harnessed_script(
        &mut self,
        harness: &[String],
        source: &str,
        strict: bool,
    ) -> Result<(), String>;

    /// Execute harness scripts, then one module in the same realm.
    fn run_harnessed_module(&mut self, harness: &[String], source: &str) -> Result<(), String>;
}

/// Runner metadata needed before dispatching one test.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TestMetadata {
    /// Whether the test must execute as an ES module.
    pub is_module: bool,
    /// Whether the test uses the asynchronous `$DONE` completion protocol.
    pub is_async: bool,
    /// Whether the test must run without default or declared harness sources.
    pub is_raw: bool,
    /// Whether the runner must prepend a strict directive.
    pub only_strict: bool,
    /// Harness files requested by the test, in declaration order.
    pub includes: Vec<String>,
    /// Expected early/runtime error phase, when the test is negative.
    pub negative_phase: Option<String>,
    /// Expected error constructor name, when the test is negative.
    pub negative_type: Option<String>,
}

impl TestMetadata {
    /// Parse the small YAML subset used for dispatch decisions.
    pub fn parse(source: &str) -> Result<Self, String> {
        let frontmatter = extract_frontmatter(source)?;
        let mut metadata = Self::default();
        let mut in_negative = false;
        for line in frontmatter.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("flags:") {
                let flags = list_after_colon(trimmed);
                metadata.is_module = flags.iter().any(|flag| flag == "module");
                metadata.is_async = flags.iter().any(|flag| flag == "async");
                metadata.is_raw = flags.iter().any(|flag| flag == "raw");
                metadata.only_strict = flags.iter().any(|flag| flag == "onlyStrict");
                in_negative = false;
            } else if trimmed.starts_with("includes:") {
                metadata.includes = list_after_colon(trimmed);
                in_negative = false;
            } else if trimmed == "negative:" {
                in_negative = true;
            } else if in_negative && trimmed.starts_with("phase:") {
                metadata.negative_phase = value_after_colon(trimmed);
            } else if in_negative && trimmed.starts_with("type:") {
                metadata.negative_type = value_after_colon(trimmed);
            } else if !trimmed.is_empty() && !line.starts_with(' ') {
                in_negative = false;
            }
        }
        Ok(metadata)
    }
}

fn extract_frontmatter(source: &str) -> Result<&str, String> {
    let Some(start) = source.find("/*---") else {
        return Ok("");
    };
    let body_start = start + "/*---".len();
    let Some(end_offset) = source[body_start..].find("---*/") else {
        return Err("unterminated test262 frontmatter".into());
    };
    Ok(&source[body_start..body_start + end_offset])
}

fn value_after_colon(line: &str) -> Option<String> {
    line.split_once(':')
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn list_after_colon(line: &str) -> Vec<String> {
    let Some((_, raw)) = line.split_once(':') else {
        return Vec::new();
    };
    raw.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(str::trim)
        .map(|item| item.trim_matches(['\'', '"']))
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

/// Result of dispatching one test source to the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestOutcome {
    /// The engine completed without an abrupt completion.
    Pass,
    /// The engine rejected or threw while running the test.
    Fail { reason: String },
}

/// Aggregate result for one runner-selected batch or stage.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct StageReport {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub failures: Vec<(std::path::PathBuf, String)>,
}

/// Discover JavaScript test files recursively in deterministic path order.
pub fn discover_js_files<P: AsRef<Path>>(root: P) -> Result<Vec<std::path::PathBuf>, String> {
    let mut files = Vec::new();
    collect_js_files(root.as_ref(), &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_js_files(directory: &Path, files: &mut Vec<std::path::PathBuf>) -> Result<(), String> {
    for entry in std::fs::read_dir(directory)
        .map_err(|error| format!("test262 directory read failed: {error}"))?
    {
        let path = entry
            .map_err(|error| format!("test262 directory entry failed: {error}"))?
            .path();
        if path.is_dir() {
            collect_js_files(&path, files)?;
        } else if path.extension().is_some_and(|extension| extension == "js")
            && !path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with("_FIXTURE.js"))
        {
            // `*_FIXTURE.js` files are harness fixtures, not runnable tests.
            files.push(path);
        }
    }
    Ok(())
}

/// Runner parameterized over the engine host implementation.
pub struct Test262Runner<H: Test262Host> {
    host: H,
}

impl<H: Test262Host> Test262Runner<H> {
    /// Create a runner around an engine host.
    pub fn new(host: H) -> Self {
        Self { host }
    }

    /// Run one script test source.
    pub fn run_script(&mut self, source: &str) -> TestOutcome {
        map_result(self.host.run_script(source))
    }

    /// Run one module test source.
    pub fn run_module_script(&mut self, source: &str) -> TestOutcome {
        map_result(self.host.run_module_script(source))
    }

    /// Parse dispatch metadata and run one complete test source.
    pub fn run_test(&mut self, source: &str) -> Result<TestOutcome, String> {
        let metadata = TestMetadata::parse(source)?;
        Ok(apply_negative_expectation(
            self.dispatch_test(&[], source, &metadata),
            &metadata,
        ))
    }

    /// Load requested harness files, compose them with the test, and dispatch.
    pub fn run_test_with_harness<F>(
        &mut self,
        source: &str,
        mut load: F,
    ) -> Result<TestOutcome, String>
    where
        F: FnMut(&str) -> Result<String, String>,
    {
        let metadata = TestMetadata::parse(source)?;
        let harness = load_harness(&metadata, &mut load)?;
        Ok(apply_negative_expectation(
            self.dispatch_test(&harness, source, &metadata),
            &metadata,
        ))
    }

    /// Read and execute one test262 source file.
    pub fn run_file<P: AsRef<Path>>(&mut self, path: P) -> Result<TestOutcome, String> {
        let source = std::fs::read_to_string(path.as_ref())
            .map_err(|error| format!("test262 read failed: {error}"))?;
        self.run_test(&source)
    }

    /// Read one test262 file and compose its requested harness files.
    pub fn run_file_with_harness<P, F>(&mut self, path: P, load: F) -> Result<TestOutcome, String>
    where
        P: AsRef<Path>,
        F: FnMut(&str) -> Result<String, String>,
    {
        let source = std::fs::read_to_string(path.as_ref())
            .map_err(|error| format!("test262 read failed: {error}"))?;
        self.run_test_with_harness(&source, load)
    }

    /// Run files in iterator order and collect all outcomes.
    pub fn run_files<I, P>(&mut self, paths: I) -> Result<StageReport, String>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let mut report = StageReport::default();
        for input in paths {
            let path = input.as_ref().to_path_buf();
            report.total += 1;
            let outcome = match self.run_file(&path) {
                Ok(outcome) => outcome,
                Err(reason) => TestOutcome::Fail { reason },
            };
            match outcome {
                TestOutcome::Pass => report.passed += 1,
                TestOutcome::Fail { reason } => {
                    report.failed += 1;
                    report.failures.push((path, reason));
                }
            }
        }
        Ok(report)
    }

    /// Run files in iterator order with the same harness loader for each file.
    pub fn run_files_with_harness<I, P, F>(
        &mut self,
        paths: I,
        mut load: F,
    ) -> Result<StageReport, String>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
        F: FnMut(&str) -> Result<String, String>,
    {
        let mut report = StageReport::default();
        for input in paths {
            let path = input.as_ref().to_path_buf();
            report.total += 1;
            let outcome = match self.run_file_with_harness(&path, &mut load) {
                Ok(outcome) => outcome,
                Err(reason) => TestOutcome::Fail { reason },
            };
            match outcome {
                TestOutcome::Pass => report.passed += 1,
                TestOutcome::Fail { reason } => {
                    report.failed += 1;
                    report.failures.push((path, reason));
                }
            }
        }
        Ok(report)
    }

    fn dispatch_test(
        &mut self,
        harness: &[String],
        source: &str,
        metadata: &TestMetadata,
    ) -> TestOutcome {
        if metadata.is_module {
            return map_result(self.host.run_harnessed_module(harness, source));
        }
        map_result(
            self.host
                .run_harnessed_script(harness, source, metadata.only_strict),
        )
    }
}

fn load_harness<F>(metadata: &TestMetadata, load: &mut F) -> Result<Vec<String>, String>
where
    F: FnMut(&str) -> Result<String, String>,
{
    let mut harness = Vec::new();
    if !metadata.is_raw {
        load_harness_file(&mut harness, load, "assert.js")?;
        load_harness_file(&mut harness, load, "sta.js")?;
        if metadata.is_async {
            load_harness_file(&mut harness, load, "doneprintHandle.js")?;
        }
        for include in &metadata.includes {
            if !is_default_harness_binding(include) {
                load_harness_file(&mut harness, load, include)?;
            }
        }
    }
    Ok(harness)
}

fn is_default_harness_binding(include: &str) -> bool {
    matches!(include, "assert.js" | "sta.js")
}

fn load_harness_file<F>(harness: &mut Vec<String>, load: &mut F, name: &str) -> Result<(), String>
where
    F: FnMut(&str) -> Result<String, String>,
{
    harness.push(load(name)?);
    Ok(())
}

fn apply_negative_expectation(outcome: TestOutcome, metadata: &TestMetadata) -> TestOutcome {
    let Some(expected_type) = metadata.negative_type.as_deref() else {
        return outcome;
    };
    match outcome {
        TestOutcome::Pass => TestOutcome::Fail {
            reason: format!("expected {expected_type} but execution completed"),
        },
        TestOutcome::Fail { reason } if reason.contains(expected_type) => TestOutcome::Pass,
        TestOutcome::Fail { reason } => TestOutcome::Fail {
            reason: format!("expected {expected_type}, got {reason}"),
        },
    }
}

fn map_result(result: Result<(), String>) -> TestOutcome {
    match result {
        Ok(()) => TestOutcome::Pass,
        Err(reason) => TestOutcome::Fail { reason },
    }
}
