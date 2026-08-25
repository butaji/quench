//! Standalone ECMA-262 conformance runner boundary.
//!
//! This crate owns runner outcomes and dispatch.
//!
//! It is the sole owner of test262 metadata, exact harness composition,
//! staging selection, and expected-completion classification. The runtime is
//! treated as an external JavaScript engine and is never given test262 policy.
use std::path::Path;

mod harness_cache;
pub mod module_graph;
mod runner_support;
pub mod runtime_host;
mod stages;
pub use harness_cache::HarnessCache;
pub use runtime_host::{LinkedModule, LinkedModuleGraph, RuntimeHost};
pub use stages::{list_stages, resolve_stages, ConformanceStage, ResolvedStage};

/// Engine-facing execution contract for an external conformance runner.
pub trait Test262Host: Send {
    /// Execute a complete script source.
    fn run_script(&mut self, source: &str) -> Result<(), String>;

    /// Execute a complete ES module source.
    fn run_module_script(&mut self, source: &str) -> Result<(), String>;

    /// Execute harness scripts, then one test script in the same realm.
    fn run_harnessed_script(
        &mut self,
        harness: &[&str],
        source: &str,
        strict: bool,
    ) -> Result<(), String>;

    /// Execute a harnessed script with host scheduling flags from metadata.
    fn run_harnessed_script_with_flags(
        &mut self,
        harness: &[&str],
        source: &str,
        strict: bool,
        _can_block: bool,
    ) -> Result<(), String> {
        self.run_harnessed_script(harness, source, strict)
    }

    /// Execute harness scripts, then one module in the same realm.
    fn run_harnessed_module(&mut self, harness: &[&str], source: &str) -> Result<(), String>;

    /// Execute a module whose source-file location is known to the runner.
    ///
    /// The default preserves source-only hosts. A filesystem-aware host uses
    /// this location solely to resolve the module's declared specifiers.
    fn run_harnessed_module_at(
        &mut self,
        harness: &[&str],
        source: &str,
        _path: &Path,
    ) -> Result<(), String> {
        self.run_harnessed_module(harness, source)
    }
}

/// Runner metadata needed before dispatching one test.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TestMetadata {
    /// Whether the test must execute as an ES module.
    pub is_module: bool,
    /// Whether the test uses the asynchronous `$DONE` completion protocol.
    pub is_async: bool,
    /// Whether the test must run without default or declared harness sources.
    pub is_raw: bool,
    /// Whether the runner must prepend a strict directive.
    pub only_strict: bool,
    /// Whether the host agent may suspend while executing this test.
    pub can_block: bool,
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
        metadata.can_block = true;
        let mut in_negative = false;
        let normalized_frontmatter = frontmatter.replace("\r\n", "\n").replace('\r', "\n");
        for line in normalized_frontmatter.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("flags:") {
                let flags = list_after_colon(trimmed);
                metadata.is_module = flags.iter().any(|flag| flag == "module");
                metadata.is_async = flags.iter().any(|flag| flag == "async");
                metadata.is_raw = flags.iter().any(|flag| flag == "raw");
                metadata.only_strict = flags.iter().any(|flag| flag == "onlyStrict");
                metadata.can_block = !flags.iter().any(|flag| flag == "CanBlockIsFalse");
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
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    // Fixtures normally begin with copyright comments. Hashbang tests add a
    // deliberately malformed preamble before those comments; skip only that
    // narrow preamble, never arbitrary JavaScript preceding the marker.
    let mut body = skip_hashbang_preamble(source);
    loop {
        if let Some(rest) = body.strip_prefix("\r\n") {
            body = rest;
            continue;
        }
        if let Some(rest) = body.strip_prefix('\r') {
            body = rest;
            continue;
        }
        if let Some(rest) = body.strip_prefix('\n') {
            body = rest;
            continue;
        }
        let Some(rest) = body.strip_prefix("//") else {
            break;
        };
        let line_end = rest.find('\n').or_else(|| rest.find('\r'));
        let Some(line_end) = line_end else {
            return Ok("");
        };
        body = &rest[line_end + 1..];
    }
    let Some(body) = body.strip_prefix("/*---") else {
        return Ok("");
    };
    let Some(end_offset) = body.find("---*/") else {
        return Err("unterminated test262 frontmatter".into());
    };
    Ok(&body[..end_offset])
}

fn skip_hashbang_preamble(source: &str) -> &str {
    let mut body = source;

    // A preceding directive, empty statement, line comment, or block comment
    // is accepted only when it is immediately followed by a hashbang line.
    if body.starts_with("\"use strict\"") {
        if let Some(line_end) = body.find('\n') {
            let after_directive = &body[line_end + 1..];
            if after_directive.starts_with("#!") {
                body = after_directive;
            }
        }
    } else if body.starts_with(";#!") {
        body = skip_line(body);
    } else if body.starts_with("//\n#!") {
        body = &body[3..];
    } else if body.starts_with("/*\n*/#!") {
        body = &body["/*\n*/".len()..];
    }

    while is_hashbang_candidate(body) {
        if body.starts_with("#!/*") {
            if let Some(end) = body.find("*/") {
                body = &body[end + 2..];
            } else {
                return "";
            }
        } else {
            body = skip_line(body);
        }
    }
    body
}

fn is_hashbang_candidate(source: &str) -> bool {
    let Some(line_end) = source.find('\n') else {
        return source.starts_with("#!") || source.starts_with("#\\") || source.starts_with('\\');
    };
    let line = source[..line_end].trim_start();
    line.starts_with("#!")
        || line.starts_with("#\\")
        || line.starts_with("\\u")
        || line.starts_with("\\x")
        || line.starts_with("\\0")
}

fn skip_line(source: &str) -> &str {
    source
        .find('\n')
        .map_or("", |line_end| &source[line_end + 1..])
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

impl StageReport {
    /// Whether this report accounts for every discovered path exactly once.
    pub fn covers(&self, paths: &[std::path::PathBuf]) -> bool {
        self.total == paths.len()
            && self.passed + self.failed == self.total
            && self.failures.iter().all(|(path, _)| paths.contains(path))
    }

    /// Build a per-path outcome map for a completed stage report. Paths that
    /// are absent from the failure list are treated as passing; this matches
    /// the runner's convention that only failed tests are recorded with a
    /// reason. Used by determinism regression guards to compare batched
    /// outcomes against individual runs.
    pub fn outcomes(
        report: &StageReport,
        paths: &[std::path::PathBuf],
    ) -> std::collections::HashMap<std::path::PathBuf, TestOutcome> {
        let mut indexed = std::collections::HashMap::new();
        for path in paths {
            indexed.insert(path.clone(), TestOutcome::Pass);
        }
        for (path, _) in &report.failures {
            indexed.insert(
                path.clone(),
                TestOutcome::Fail {
                    reason: format!("reported failure for {}", path.display()),
                },
            );
        }
        indexed
    }
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
        runner_support::outcome(self.host.run_script(source))
    }

    /// Run one module test source.
    pub fn run_module_script(&mut self, source: &str) -> TestOutcome {
        runner_support::outcome(self.host.run_module_script(source))
    }

    /// Parse dispatch metadata and run one complete test source.
    pub fn run_test(&mut self, source: &str) -> Result<TestOutcome, String> {
        let metadata = TestMetadata::parse(source)?;
        Ok(runner_support::negative(
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
        let harness = runner_support::harness(&metadata, &mut load)?;
        let harness = harness.iter().map(String::as_str).collect::<Vec<_>>();
        Ok(runner_support::negative(
            self.dispatch_test(&harness, source, &metadata),
            &metadata,
        ))
    }

    /// Compose a test with cached harness sources without copying their text.
    pub fn run_test_with_cache(
        &mut self,
        source: &str,
        cache: &mut HarnessCache,
    ) -> Result<TestOutcome, String> {
        let metadata = TestMetadata::parse(source)?;
        let harness = runner_support::cached_harness(&metadata, cache)?;
        Ok(runner_support::negative(
            self.dispatch_test(&harness, source, &metadata),
            &metadata,
        ))
    }

    /// Compose a test with cached harness sources using pre-parsed metadata.
    pub fn run_test_with_cache_and_metadata(
        &mut self,
        source: &str,
        metadata: &TestMetadata,
        cache: &mut HarnessCache,
    ) -> Result<TestOutcome, String> {
        let harness = runner_support::cached_harness(metadata, cache)?;
        Ok(runner_support::negative(
            self.dispatch_test(&harness, source, metadata),
            metadata,
        ))
    }

    /// Compose a test with cached harness sources, pre-parsed metadata, and a
    /// source-file location so module tests can resolve their imports.
    pub fn run_test_with_cache_metadata_and_path(
        &mut self,
        source: &str,
        metadata: &TestMetadata,
        path: &Path,
        cache: &mut HarnessCache,
    ) -> Result<TestOutcome, String> {
        let harness = runner_support::cached_harness(metadata, cache)?;
        Ok(runner_support::negative(
            self.dispatch_test_at(&harness, source, metadata, Some(path)),
            metadata,
        ))
    }

    /// Read and execute one test262 source file.
    pub fn run_file<P: AsRef<Path>>(&mut self, path: P) -> Result<TestOutcome, String> {
        let source = std::fs::read_to_string(path.as_ref())
            .map_err(|error| format!("test262 read failed: {error}"))?;
        self.run_test(&source)
    }

    /// Read one test262 file and compose its requested harness files.
    pub fn run_file_with_harness<P, F>(
        &mut self,
        path: P,
        mut load: F,
    ) -> Result<TestOutcome, String>
    where
        P: AsRef<Path>,
        F: FnMut(&str) -> Result<String, String>,
    {
        let path = path.as_ref();
        let source = std::fs::read_to_string(path)
            .map_err(|error| format!("test262 read failed: {error}"))?;
        let metadata = TestMetadata::parse(&source)?;
        let harness = runner_support::harness(&metadata, &mut load)?;
        let harness = harness.iter().map(String::as_str).collect::<Vec<_>>();
        Ok(runner_support::negative(
            self.dispatch_test_at(&harness, &source, &metadata, Some(path)),
            &metadata,
        ))
    }

    /// Read one test262 file and compose it with cached harness sources.
    pub fn run_file_with_cache<P>(
        &mut self,
        path: P,
        cache: &mut HarnessCache,
    ) -> Result<TestOutcome, String>
    where
        P: AsRef<Path>,
    {
        let source = std::fs::read_to_string(path.as_ref())
            .map_err(|error| format!("test262 read failed: {error}"))?;
        self.run_test_with_cache_at(&source, path.as_ref(), cache)
    }

    fn run_test_with_cache_at(
        &mut self,
        source: &str,
        path: &Path,
        cache: &mut HarnessCache,
    ) -> Result<TestOutcome, String> {
        let metadata = TestMetadata::parse(source)?;
        let harness = runner_support::cached_harness(&metadata, cache)?;
        Ok(runner_support::negative(
            self.dispatch_test_at(&harness, source, &metadata, Some(path)),
            &metadata,
        ))
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
            runner_support::record(&mut report, path, outcome);
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
            runner_support::record(&mut report, path, outcome);
        }
        Ok(report)
    }

    /// Run files in iterator order with one shared, zero-copy harness cache.
    pub fn run_files_with_cache<I, P>(
        &mut self,
        paths: I,
        cache: &mut HarnessCache,
    ) -> Result<StageReport, String>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        self.run_files_with_cache_impl(paths, cache, None)
    }

    /// Run files with a cached harness and cap collected failure samples.
    pub fn run_files_with_cache_limited<I, P>(
        &mut self,
        paths: I,
        cache: &mut HarnessCache,
        max_failures: usize,
    ) -> Result<StageReport, String>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        self.run_files_with_cache_impl(paths, cache, Some(max_failures))
    }

    fn run_files_with_cache_impl<I, P>(
        &mut self,
        paths: I,
        cache: &mut HarnessCache,
        max_failures: Option<usize>,
    ) -> Result<StageReport, String>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let mut report = StageReport::default();
        for input in paths {
            let path = input.as_ref().to_path_buf();
            report.total += 1;
            let outcome = match self.run_file_with_cache(&path, cache) {
                Ok(outcome) => outcome,
                Err(reason) => TestOutcome::Fail { reason },
            };
            let keep_failures = match max_failures {
                Some(max) => report.failures.len() < max,
                None => true,
            };
            if keep_failures {
                runner_support::record(&mut report, path, outcome);
            } else {
                runner_support::count(&mut report, outcome);
            }
        }
        Ok(report)
    }

    fn dispatch_test(
        &mut self,
        harness: &[&str],
        source: &str,
        metadata: &TestMetadata,
    ) -> TestOutcome {
        self.dispatch_test_at(harness, source, metadata, None)
    }

    fn dispatch_test_at(
        &mut self,
        harness: &[&str],
        source: &str,
        metadata: &TestMetadata,
        path: Option<&Path>,
    ) -> TestOutcome {
        // AGENTS.md: harness fidelity is absolute; dispatch the composed sources unchanged.
        if metadata.is_module {
            if let Some(path) = path {
                return runner_support::outcome(
                    self.host.run_harnessed_module_at(harness, source, path),
                );
            }
            return runner_support::outcome(self.host.run_harnessed_module(harness, source));
        }
        runner_support::outcome(self.host.run_harnessed_script_with_flags(
            harness,
            source,
            metadata.only_strict,
            metadata.can_block,
        ))
    }
}
