use super::*;
use crate::metadata::{Negative, Test262Metadata};

pub(super) fn meta_with(phase: &str, typ: &str) -> Test262Metadata {
    Test262Metadata {
        negative: Some(Negative {
            phase: phase.into(),
            typ: typ.into(),
        }),
        ..Test262Metadata::default()
    }
}

pub(super) fn positive_meta() -> Test262Metadata {
    Test262Metadata::default()
}

pub(super) fn is_pass(outcome: &TestOutcome) -> bool {
    matches!(outcome, TestOutcome::Pass)
}

pub(super) fn is_not_infra(outcome: &TestOutcome) -> bool {
    match outcome {
        TestOutcome::Pass => true,
        TestOutcome::Fail { failure } => !failure.message.starts_with("infrastructure failure"),
        TestOutcome::Skip { .. } => true,
    }
}
