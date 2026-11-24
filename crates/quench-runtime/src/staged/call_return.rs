//! The order of a script activation's return as staged rules over the tier's primitives: the
//! running activation's (`step_activation_return_of`, `step_return_one_op_of`), a run of
//! activations each returning its callee's completion (`step_returning_chain_of`), and another
//! program's activation's (`step_cross_return_of`). The words each completion reads and writes
//! are `call_complete`'s rows; these rules decide which completion runs.

use super::call_complete::concrete_returns_callee_completion;
use crate::bytecode::Register;

/// The primitives of a script activation's return (`step_return_one_op_of`,
/// `step_activation_return_of`): the running leaf's, the plain header's, a returning run's and
/// the side record's completions. Every method is one completion the tier makes or declines;
/// the rules decide their order.
pub trait ScriptReturn {
    /// The returned value.
    type Value: Copy;
    /// Where the caller continues: its registers and next record.
    type Returned;
    /// Whether the returning record is a leaf's (its form fixed where the record was made).
    fn leaf(&self) -> bool;
    /// The running leaf's completion where it is of the caller's own program, as one op.
    fn leaf_header(&mut self, value: Self::Value) -> Option<Self::Returned>;
    /// The running leaf's completion, of any program.
    fn complete_leaf(&mut self, value: Self::Value) -> Option<Self::Returned>;
    /// The plain return of the running window's header (`step_plain_return_of`).
    fn plain_header(&mut self, value: Self::Value) -> Option<Self::Returned>;
    /// How many plain frames, innermost first, return their callee's completion before one whose
    /// call keeps it; `None` where the run ends any other way. Reads headers only.
    fn returning_run(&mut self) -> Option<usize>;
    /// Pops `above` returning plain frames (`step_returning_pop_of` each), which the run proved.
    fn pop_returning(&mut self, above: usize);
    /// The completion past the plain ones: the side record's, or the general return's chain.
    fn side_return(&mut self, value: Self::Value) -> Option<Self::Returned>;
}

/// The return with `value` of the running activation as one op, making no call: a running leaf
/// of the caller's program completes, a plain frame pops its header. `None`, which changes
/// nothing, for any other, which `step_activation_return_of` decides.
#[inline(always)]
pub fn step_return_one_op_of<S: ScriptReturn>(stage: &mut S, value: S::Value) -> Option<S::Returned> {
    if stage.leaf() { stage.leaf_header(value) } else { stage.plain_header(value) }
}

/// The return with `value` of the running script activation: a leaf's record completes the
/// running leaf first; then the header's plain return; then a run of plain frames that return
/// their callee's completion (`return f()`), which pops only where it ends in a call that keeps
/// the value, whose header then returns; else the side record's completion.
#[inline(always)]
pub fn step_activation_return_of<S: ScriptReturn>(stage: &mut S, value: S::Value) -> Option<S::Returned> {
    if stage.leaf() && let Some(returned) = stage.complete_leaf(value) {
        return Some(returned);
    }
    if let Some(returned) = stage.plain_header(value) {
        return Some(returned);
    }
    if let Some(above) = stage.returning_run() {
        stage.pop_returning(above);
        return stage.plain_header(value);
    }
    stage.side_return(value)
}

/// What a script frame's completion is to a returning chain (`step_returning_chain_of`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainCompletion {
    /// The returned value, into a caller of the same program.
    Plain,
    /// The returned value, from another program's activation, which enters its caller's program.
    Cross,
    /// A lane body's callee, which ends the run its call started.
    ReturnsToBody,
    /// A completion that needs its side record (a construction, a settle, a native's callback...).
    Flagged,
}

/// The primitives of a run of script activations each returning the completion of the call
/// above it (`step_returning_chain_of`), and of another program's activation's return
/// (`step_cross_return_of`). Every method is one read or one completion the tier makes.
pub trait ReturningChain {
    /// The returned value.
    type Value: Copy;
    /// Where the caller continues.
    type Resume;
    /// A frame, read where it is.
    type Frame: Copy;
    /// The frames, innermost first; `None` for one that is not a script activation.
    fn frames(&mut self) -> impl Iterator<Item = Option<Self::Frame>>;
    /// The frame's completion.
    fn completion(frame: &Self::Frame) -> ChainCompletion;
    /// The result word of the frame's call record.
    fn call_result(frame: &Self::Frame) -> u64;
    /// Drops the `above` activations past `outermost`, which runs next.
    fn discard(&mut self, above: usize, outermost: Self::Frame);
    /// The return into the lane body that called `outermost`'s activation.
    fn end_body(&mut self, value: Self::Value) -> Option<Self::Resume>;
    /// Enters `outermost`'s caller's program, which a crossing activation above it left.
    fn reenter_caller(&mut self, outermost: Self::Frame);
    /// The running activation's return, the run's outermost (`step_activation_return_of`).
    fn complete_plain(&mut self, value: Self::Value) -> Option<Self::Resume>;
    /// Whether the running plain frame's header crosses programs; `None` where no plain frame runs.
    fn plain_header_crosses(&mut self) -> Option<bool>;
    /// The running activation's return into another program's caller, which it enters.
    fn return_crossing(&mut self, value: Self::Value) -> Option<Self::Resume>;
}

/// The outermost frame of the run the innermost one completes, each returning the completion of
/// the call above it: how many lie above it, the frame, and whether the run crosses programs.
/// A body's callee ends the run (unless a crossing frame lies above it); a flagged completion,
/// or a frame that is not a script one, declines it.
#[inline(always)]
pub fn returning_chain<S: ReturningChain>(frames: impl Iterator<Item = Option<S::Frame>>) -> Option<(usize, S::Frame, bool)> {
    let (mut above, mut crossed) = (0, false);
    for frame in frames {
        let frame = frame?;
        match S::completion(&frame) {
            ChainCompletion::ReturnsToBody => return (!crossed).then_some((above, frame, false)),
            ChainCompletion::Flagged => return None,
            completion => crossed |= completion == ChainCompletion::Cross,
        }
        if !concrete_returns_callee_completion(S::call_result(&frame) as Register) {
            return Some((above, frame, crossed));
        }
        above += 1;
    }
    None
}

/// The return with `value` of a run of script activations, of one program or several, each
/// returning the completion of the call above it (`return f(...)`), down to one whose call takes
/// the value: the inner ones drop, and the outermost returns as its completion says (into a lane
/// body, across programs, or plainly, back in its caller's program where the run crossed).
/// `None`, which changes nothing, leaves any other completion to the general returns.
#[inline(always)]
pub fn step_returning_chain_of<S: ReturningChain>(stage: &mut S, value: S::Value) -> Option<S::Resume> {
    let frames = stage.frames();
    let (above, outermost, crossed) = returning_chain::<S>(frames)?;
    stage.discard(above, outermost);
    match S::completion(&outermost) {
        ChainCompletion::ReturnsToBody => return stage.end_body(value),
        ChainCompletion::Cross => return step_cross_return_of(stage, value),
        _ => {}
    }
    if crossed {
        stage.reenter_caller(outermost);
    }
    stage.complete_plain(value)
}

/// The return with `value` of another program's activation, which enters its caller's program:
/// a plain frame crosses by its header's flag, so one without it declines before its side record
/// is read. `None`, which changes nothing, for any other.
#[inline(always)]
pub fn step_cross_return_of<S: ReturningChain>(stage: &mut S, value: S::Value) -> Option<S::Resume> {
    if stage.plain_header_crosses() == Some(false) {
        return None;
    }
    stage.return_crossing(value)
}
