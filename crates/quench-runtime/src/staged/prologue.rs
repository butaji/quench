//! An activation's entry as staged operations: the tier prologue a frameless activation of a function the
//! JavaScript tier may claim starts at (`Operation::Prologue`), and the prologue of a specialized body
//! (`Operation::BodyEntry`). Neither is an instruction of any bytecode: a lane view makes them as its
//! synthetic records (`TierPrologue`, `BodyEntry` in the catalog), and the lane runs them through their
//! staged rows like any other opcode.
//!
//! What an entry does is the frame's to answer through `PrologueFrame`: the lane's frame enters the
//! tier or runs the body, and where the activation resumes is its control. The interpreter's frame
//! answers neither (the general path never sees these records), so both defaults leave the entry
//! `Unresolved` there, to the record's own hand body.

use super::{Frame, Operation, Stage, Step};

/// The effects an activation's entry makes on its frame.
pub trait PrologueFrame<S: Stage>: Frame<S> {
    /// The tier prologue of a frameless activation of `function`, which has `parameters` parameters: the
    /// JavaScript tier enters the function (its compiled body where it runs one), and the activation
    /// continues where that completes it. `None` where the frame does not enter it.
    #[inline(always)]
    fn tier_prologue(&mut self, _stage: &mut S, _function: u32, _parameters: u16) -> Option<Self::Control> {
        None
    }

    /// The prologue of `function`'s specialized body: the body runs on the activation's registers (binding
    /// the receiver where `binds_this`), and the activation continues where the body leaves. `None` where the
    /// frame does not run it.
    #[inline(always)]
    fn body_entry(&mut self, _stage: &mut S, _function: u32, _binds_this: bool) -> Option<Self::Control> {
        None
    }
}

/// The activation entry `operation` denotes, on any stage whose frame answers `PrologueFrame`; any other
/// operation is unresolved.
#[inline(always)]
pub(crate) fn step<S: Stage, F: PrologueFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>)
    -> Step<S::Word, F::Control> {
    let control = match operation {
        Operation::Prologue { function, parameters } => frame.tier_prologue(stage, function, parameters),
        Operation::BodyEntry { function, binds_this } => frame.body_entry(stage, function, binds_this),
        _ => None,
    };
    match control {
        Some(control) => Step::Control(control),
        None => Step::Unresolved,
    }
}

/// `step` of an entry row (`RowStep::Entry`) on the interpreter's word stage, whose frame is a lane frame.
#[inline(always)]
pub fn step_entry<F: PrologueFrame<super::Concrete>>(stage: &mut super::Concrete, frame: &mut F,
    operation: Operation<u64, ()>) -> Step<u64, F::Control> {
    step(stage, frame, operation)
}
