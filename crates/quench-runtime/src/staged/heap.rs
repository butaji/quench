//! The staged heap operations: literal allocation, the host helpers and the dense element
//! write, each over the frame's proof.

use super::*;

/// The heap operations: literal allocation, the host helpers and element access. A frame with
/// no heap for the operation leaves it `Unresolved` for its host path.
#[inline(always)]
pub(super) fn heap_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        allocate @ Operation::Allocate { .. } => allocation_step(stage, frame, allocate),
        define @ Operation::DefineElement { .. } => element_define_step(stage, frame, define),
        helper @ (Operation::IndexSetHelper { .. } | Operation::FieldStoreMiss { .. }) => helper_step(stage, frame, helper),
        site @ (Operation::AddSite { .. } | Operation::NamingSite { .. }) => site_step(stage, frame, site),
        index => index_step(stage, frame, index),
    }
}

/// The property-add and naming sites over the frame's miss edge: a hit defines the written word, a
/// decline leaves the record to the VM (`add::site_add`, `naming::site_name`).
#[inline(always)]
fn site_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    let (word, done) = match operation {
        Operation::AddSite { records, object, edge, link, form, value } =>
            (value, add::site_add(stage, records, object, (edge, link), form, value, frame.miss())),
        Operation::NamingSite { records, function, naming, form, name } =>
            (name, naming::site_name(stage, records, function, naming, form, name, frame.miss())),
        _ => return Step::Unresolved,
    };
    match done {
        Some(()) => Step::Value(word),
        None => Step::Unresolved,
    }
}
