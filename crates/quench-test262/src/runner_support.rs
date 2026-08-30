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
    // AGENTS.md: harness fidelity is absolute; load only exact declared sources.
    cache.sources(names(metadata))
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
    names(metadata).map(load).collect()
}

/// Canonical harness order, replayable without allocating its names.
#[derive(Clone)]
pub(crate) struct HarnessNames<'a> {
    metadata: &'a TestMetadata,
    index: usize,
    include_index: usize,
}

pub(crate) fn names(metadata: &TestMetadata) -> HarnessNames<'_> {
    HarnessNames {
        metadata,
        index: 0,
        include_index: 0,
    }
}

impl<'a> Iterator for HarnessNames<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        let metadata = self.metadata;
        if metadata.is_raw {
            return None;
        }
        let name = match self.index {
            0 => Some("assert.js"),
            1 => Some("sta.js"),
            2 if metadata.is_async => Some("doneprintHandle.js"),
            _ => None,
        };
        self.index += 1;
        if name.is_some() {
            return name;
        }
        while let Some(include) = metadata.includes.get(self.include_index) {
            self.include_index += 1;
            if !matches!(include.as_str(), "assert.js" | "sta.js") {
                return Some(include.as_str());
            }
        }
        None
    }
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
