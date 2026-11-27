//! A call whose callee is a native function (`CalleeKind::Native`), written once for every
//! projection. A native has no window of its own: its entry takes the arguments as a slice, so a
//! window of the caller's file is passed where it lies, and any other form (a register list, a bound
//! function's folded prefix) is gathered into the callee's room first. A native that observes the
//! frame stack needs the lane's activations to be frames, which the call makes first
//! (`CallFrame::call_link`); one that runs beside them (`Native::runs_beside_activations`) does not.
//! A native that reaches guest code which observes the frames ends with the status the lane
//! left with, which `complete_call` takes to the frame's `unfinished` edge.

use super::invoke::{callee_this, fill_arguments, CallFrame};
use super::plan::{DirectPlan, NativeId};
use super::protocol::complete_call;
use super::{word_bytes, Arguments, Invocation, PropertyFrame, Stage};

/// The arguments the native is passed: a word and a count, the caller's own window where the call
/// passes one, else the call's arguments gathered into a window of the callee's room.
#[inline(always)]
fn native_arguments<S: Stage, F: CallFrame<S>>(stage: &mut S, frame: &mut F, file: S::Word, miss: S::Miss,
    (site, plan): (&Invocation, &DirectPlan)) -> Option<(S::Word, u16)> {
    if let (Arguments::Window { first, count }, None) = (site.arguments, plan.bound) {
        return Some((stage.add_imm(file, word_bytes(first)), count));
    }
    let count = site.arguments.count() + plan.bound.map_or(0, |bound| bound.arguments.count);
    let room = frame.call_room(stage, miss)?;
    fill_arguments(stage, frame, file, room.top, (site, plan, count))?;
    Some((room.top, count))
}

/// The direct call of `site` to `native` through `plan`: identity, `this`, arguments, link, entry and
/// completion, each a row. A construction is the VM's.
#[inline(always)]
pub fn enter_native<S: Stage, F: CallFrame<S> + PropertyFrame<S>>(stage: &mut S, frame: &mut F, miss: S::Miss,
    site: &Invocation, (plan, native): (DirectPlan, NativeId)) -> Option<S::Word> {
    if site.constructs() { return None; }
    let file = frame.call_registers(stage)?;
    let this = callee_this(stage, frame, file, miss, (site, &plan))?;
    let arguments = native_arguments(stage, frame, file, miss, (site, &plan))?;
    if !native.beside {
        frame.call_link(stage, miss)?;
    }
    let entered = frame.call_native(stage, native, this, arguments, miss)?;
    let value = complete_call(stage, entered.status, entered.word, None, plan.returned, entered.unfinished)?;
    frame.call_result(stage, site, value)?;
    Some(value)
}
