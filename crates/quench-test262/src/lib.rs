//! Standalone ECMA-262 conformance runner boundary.
//!
//! This crate owns runner outcomes and dispatch. The engine is accessed only
//! through [`quench_runtime::Test262Host`]; parser, IR, heap, and builtin
//! implementation details remain in `quench-runtime`.

use quench_runtime::Test262Host;
use std::path::Path;

/// Runner metadata needed before dispatching one test.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TestMetadata {
    /// Whether the test must execute as an ES module.
    pub is_module: bool,
    /// Whether the test uses the asynchronous `$DONE` completion protocol.
    pub is_async: bool,
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
            self.dispatch(source, metadata.is_module),
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
        let mut composed = String::new();
        for include in &metadata.includes {
            composed.push_str(&load(include)?);
            composed.push('\n');
        }
        if metadata.only_strict {
            composed.push_str("\"use strict\";\n");
        }
        composed.push_str(source);
        Ok(apply_negative_expectation(
            self.dispatch(&composed, metadata.is_module),
            &metadata,
        ))
    }

    /// Read and execute one test262 source file.
    pub fn run_file<P: AsRef<Path>>(&mut self, path: P) -> Result<TestOutcome, String> {
        let source = std::fs::read_to_string(path.as_ref())
            .map_err(|error| format!("test262 read failed: {error}"))?;
        self.run_test(&source)
    }

    fn dispatch(&mut self, source: &str, is_module: bool) -> TestOutcome {
        if is_module {
            self.run_module_script(source)
        } else {
            self.run_script(source)
        }
    }
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
