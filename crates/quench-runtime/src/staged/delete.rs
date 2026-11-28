//! `delete base[key]` as a row: the frame's own deletion answers what runs no guest code and
//! changes only a configurable property; any other base, key or property (a non-configurable
//! one, a proxy, an exotic object) is the VM's call-out (`CallOut::Delete`), which throws by
//! `strict` and the running function's own strictness.

use super::callout::calling;
use super::*;

/// `delete base[key]` into `destination`: the frame's deletion, else the VM's.
#[inline(always)]
pub fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, destination: Register,
    [base, key]: [Register; 2], strict: bool) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let (base_word, key_word) = (frame.tagged(stage, base), frame.tagged(stage, key));
    match frame.delete_property(stage, base_word, key_word, strict, miss) {
        Some((deleted, fact)) => {
            frame.write(stage, destination, deleted, fact);
            Step::Value(deleted)
        }
        None => calling(stage, frame, CallOut::Delete { base, key, strict }, Some(destination)),
    }
}
