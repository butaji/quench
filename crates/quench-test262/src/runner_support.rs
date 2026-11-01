use std::path::PathBuf;

use crate::{HarnessCache, StageReport, TestMetadata, TestOutcome};

pub(crate) fn count(report: &mut StageReport, outcome: TestOutcome) {
    if let TestOutcome::Fail { .. } = outcome {
        report.failed += 1;
    } else {
        report.passed += 1;
    }
}

pub fn cached_harness<'a>(
    metadata: &TestMetadata,
    cache: &'a mut HarnessCache,
) -> Result<Vec<&'a str>, String> {
    // AGENTS.md: harness fidelity is absolute; load only exact declared sources.
    cache.sources(&names(metadata))
}

pub(crate) fn record(report: &mut StageReport, path: PathBuf, outcome: TestOutcome) {
    match outcome {
        TestOutcome::Pass => report.passed += 1,
        TestOutcome::Fail { reason } => {
            report.failed += 1;
            report.failures.push((path, reason));
        }
    }
}

pub(crate) fn harness<F>(metadata: &TestMetadata, load: &mut F) -> Result<Vec<String>, String>
where
    F: FnMut(&str) -> Result<String, String>,
{
    // AGENTS.md: harness fidelity is absolute; compose only exact declared sources.
    names(metadata).into_iter().map(load).collect()
}

pub(crate) fn names(metadata: &TestMetadata) -> Vec<&str> {
    if metadata.is_raw {
        return Vec::new();
    }
    let mut names = vec!["assert.js", "sta.js"];
    if metadata.is_async {
        names.push("doneprintHandle.js");
    }
    names.extend(
        metadata
            .includes
            .iter()
            .map(String::as_str)
            .filter(|include| !matches!(*include, "assert.js" | "sta.js")),
    );
    names
}

pub fn negative(outcome: TestOutcome, metadata: &TestMetadata) -> TestOutcome {
    let Some(expected_type) = metadata.negative_type.as_deref() else {
        return outcome;
    };
    match outcome {
        TestOutcome::Pass => TestOutcome::Fail {
            reason: format!("expected {expected_type} but execution completed"),
        },
        TestOutcome::Fail { reason } if expected_failure(&reason, metadata, expected_type) => TestOutcome::Pass,
        TestOutcome::Fail { reason } => TestOutcome::Fail {
            reason: format!("expected {expected_type}, got {reason}"),
        },
    }
}

fn expected_failure(reason: &str, metadata: &TestMetadata, expected_type: &str) -> bool {
    if reason.starts_with("quench resolution SyntaxError:") {
        return expected_type == "SyntaxError" && matches!(metadata.negative_phase.as_deref(), None | Some("resolution"));
    }
    let early = reason.strip_prefix("quench runtime SyntaxError:");
    let runtime = ["quench runtime: ", "quench runtime jobs: ", "quench runtime async: "]
        .iter().find_map(|prefix| reason.strip_prefix(prefix));
    if early.is_some() {
        return expected_type == "SyntaxError" && matches!(metadata.negative_phase.as_deref(), None | Some("parse" | "early"));
    }
    if runtime.is_some() && matches!(metadata.negative_phase.as_deref(), Some("parse" | "early")) { return false; }
    let error = runtime.unwrap_or(reason).trim_start();
    error == expected_type || error.strip_prefix(expected_type).is_some_and(|rest| rest.starts_with(':'))
}

pub fn outcome(result: Result<(), String>) -> TestOutcome {
    result.map_or_else(|reason| TestOutcome::Fail { reason }, |_| TestOutcome::Pass)
}

/// The exact strict-variant transformation, shared by source and linked execution.
pub fn script_source(source: &str, strict: bool) -> std::borrow::Cow<'_, str> {
    if strict {
        let prefix = if source.starts_with("#!") {
            source.char_indices().find(|(_, ch)| oxc_syntax::line_terminator::is_line_terminator(*ch))
                .map_or(source.len(), |(index, ch)| index + ch.len_utf8())
        } else { 0 };
        std::borrow::Cow::Owned(format!("{}\n\"use strict\";\n{}", &source[..prefix], &source[prefix..]))
    } else { std::borrow::Cow::Borrowed(source) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_errors_require_exact_type_and_phase() {
        let metadata = TestMetadata::parse("/*---\nnegative:\n  phase: parse\n  type: SyntaxError\n---*/").unwrap();
        assert!(expected_failure("quench runtime SyntaxError: invalid token", &metadata, "SyntaxError"));
        assert!(!expected_failure("quench runtime: SyntaxError: thrown at runtime", &metadata, "SyntaxError"));
        assert!(!expected_failure("backend failed at /SyntaxError/example.js", &metadata, "SyntaxError"));
        assert!(!expected_failure("quench runtime: TypeError: mentions SyntaxError", &metadata, "SyntaxError"));
        let mut runtime = metadata;
        runtime.negative_phase = Some("runtime".into());
        assert!(expected_failure("quench runtime: SyntaxError: thrown at runtime", &runtime, "SyntaxError"));
        assert!(!expected_failure("quench runtime SyntaxError: invalid token", &runtime, "SyntaxError"));
        assert!(!expected_failure("quench resolution SyntaxError: invalid dependency", &runtime, "SyntaxError"));
        runtime.negative_phase = Some("resolution".into());
        assert!(expected_failure("quench resolution SyntaxError: invalid dependency", &runtime, "SyntaxError"));
        runtime.negative_phase = Some("parse".into());
        assert!(!expected_failure("quench resolution SyntaxError: invalid dependency", &runtime, "SyntaxError"));
    }
}
