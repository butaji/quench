use std::path::PathBuf;

use crate::{HarnessCache, StageReport, TestMetadata, TestOutcome};

pub(crate) fn count(report: &mut StageReport, outcome: TestOutcome) {
    if let TestOutcome::Fail { .. } = outcome {
        report.failed += 1;
    } else {
        report.passed += 1;
    }
}

pub(crate) fn cached_harness<'a>(
    metadata: &TestMetadata,
    cache: &'a mut HarnessCache,
) -> Result<Vec<&'a str>, String> {
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

pub(crate) fn negative(outcome: TestOutcome, metadata: &TestMetadata) -> TestOutcome {
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

pub(crate) fn outcome(result: Result<(), String>) -> TestOutcome {
    result.map_or_else(|reason| TestOutcome::Fail { reason }, |_| TestOutcome::Pass)
}
