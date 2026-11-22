//! The calls a Wasm lane enters as activations, written once over `WasmActivation`: a known
//! call, a reference call (its callee check, then the same entry), a call a native tier runs,
//! and a JavaScript import's call. The rules hold the order of the lane's checks; the stage
//! supplies the lane's state and the primitives (`vm/dispatch_fast/wasm_calls.rs`), so a call
//! takes the path it always took. The conversion of a scalar a JavaScript import passes is
//! `import_argument`, one body for the lane and every tier.

use super::wasm::WasmStage;
use crate::WasmType;

/// Where a Wasm call continues.
pub(crate) enum CallOutcome<R> {
    /// The caller resumes as `R` says: an activation was pushed, or the call completed.
    Resume(R),
    /// The lane has no room for the callee's activation.
    NoRoom,
    /// The call leaves the lane for the general loop.
    Exit,
}

/// The callee a reference call names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CallTarget {
    /// A function of the running program and instance, by its index.
    Function(u32),
    /// A host or JavaScript import (a `WasmHost` function).
    Import,
    /// Any other callee: the general loop calls it.
    Other,
}

/// What a native tier does with a call the lane offers it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NativeCall {
    /// The tier declines, and the call enters as an activation.
    Declined,
    /// The tier's run left the lane.
    Left,
    /// The tier's run returned the word of the call's result.
    Ran(u64),
}

/// The primitives a Wasm call's rules run on: the lane's state, its activations and its native
/// tiers. Every method is one check or one step the lane takes; the rules decide their order.
pub trait WasmActivation: WasmStage {
    /// Where a call continues in the lane (`Resumed`).
    type Resume;
    /// The activation shape a callee's frame takes, copied out of its view.
    type Shape: Copy;
    /// The callee of the reference the call names.
    fn reference(&mut self) -> CallTarget;
    /// Whether the lane may make calls inline (the frame is not leaving for native code).
    fn inline_calls(&mut self) -> bool;
    /// Whether the call may run natively: the execution mode is compiled and the call keeps its result.
    fn native_mode(&mut self) -> bool;
    /// Whether a native tier holds `function`, noting a fallback where it has none.
    fn has_native(&mut self, function: u32) -> bool;
    /// Notes a lane entry of `function` in its wasm tier; true where the general loop must enter it.
    fn note_tier_entry(&mut self, function: u32) -> bool;
    /// Runs `function` in native code, with the call's window as its arguments.
    fn enter_compiled(&mut self, function: u32) -> NativeCall;
    /// The activation shape of `function`, where the lane has derived one.
    fn shape(&mut self, function: u32) -> Option<Self::Shape>;
    /// The registers the shape's frame holds.
    fn shape_registers(shape: Self::Shape) -> usize;
    /// Whether the activations are below their limit.
    fn activation_room(&mut self) -> bool;
    /// Whether the frames and activations together are below the Wasm activation limit.
    fn frame_room(&mut self) -> bool;
    /// Whether a frame of `registers` registers fits in the storage the guest calls admit.
    fn storage_room(&mut self, registers: usize) -> bool;
    /// Whether the value stack holds a frame of `registers` registers and its fill block.
    fn values_room(&mut self, registers: usize) -> bool;
    /// Pushes the activation of `function` with its arguments, and enters it.
    fn push_activation(&mut self, function: u32, shape: Self::Shape) -> Self::Resume;
    /// Activates the JavaScript import the call names, where the lane can (`None` otherwise).
    fn activate_import(&mut self) -> Option<Self::Resume>;
    /// Calls the import the call names through its native boundary.
    fn import_native(&mut self) -> NativeCall;
    /// Leaves the lane for the general loop, at the call.
    fn left(&mut self) -> Self::Resume;
    /// Completes the call with the word of its result, in the caller.
    fn complete_call(&mut self, word: u64) -> Self::Resume;
    /// Which of the lane stack's arrays lack room for a frame call's window.
    type Full;
    /// The guard of a callee's frame.
    type Guard;
    /// The room a frame call's window takes: the callee's registers, in the lane's fill blocks.
    fn frame_window_room(&mut self) -> usize;
    /// Which arrays of the lane stack lack room for a window of `room` values and one activation.
    fn stack_full(&mut self, room: usize) -> Self::Full;
    /// Makes the lane's activations frames, which the general loop runs.
    fn materialize_activations(&mut self);
    /// Grows the arrays `full` names for a window of `room`.
    fn grow_stack(&mut self, full: Self::Full, room: usize);
    /// The frame that makes the call, before its frame is pushed.
    fn caller_frame(&mut self) -> usize;
    /// Pushes the callee's frame for `caller`'s call, or `None` where the call cannot.
    fn push_callee_frame(&mut self, caller: usize) -> Option<Self::Guard>;
    /// Records the call as `caller`'s pending general call, which resumes past the call.
    fn push_pending_call(&mut self, caller: usize, guard: Self::Guard);
    /// Refreshes the activations the thread admits, after a frame is pushed.
    fn refresh_activation_limit(&mut self);
    /// Enters `frame` at its first record.
    fn enter_frame(&mut self, frame: usize) -> Self::Resume;
}

/// The general frame push of a call the lane has no room to activate: the activations become
/// frames, the stack grows so later calls fit, and the call's frame is pushed for the general loop
/// to run. The order is the lane's; a call whose frame cannot be pushed leaves the lane. Out of
/// line, so the activation path saves no registers for it.
#[cold]
#[inline(never)]
pub(crate) fn push_frame<S: WasmActivation>(stage: &mut S) -> S::Resume {
    let room = stage.frame_window_room();
    let full = stage.stack_full(room);
    stage.materialize_activations();
    stage.grow_stack(full, room);
    let caller = stage.caller_frame();
    let Some(guard) = stage.push_callee_frame(caller) else {
        return stage.left();
    };
    stage.push_pending_call(caller, guard);
    stage.refresh_activation_limit();
    stage.enter_frame(caller + 1)
}

/// Enters `function` as a Wasm call: out of the lane when inline calls are off, through native
/// code where a tier holds it, else as an activation. The guard order is the lane's: depth, the
/// frames and activations together, storage, then the value stack's room.
#[inline(always)]
pub(crate) fn enter_function<S: WasmActivation>(stage: &mut S, function: u32) -> CallOutcome<S::Resume> {
    if !stage.inline_calls() {
        return CallOutcome::Exit;
    }
    if stage.native_mode() {
        if stage.has_native(function) {
            if let Some(resumed) = call_compiled(stage, function) {
                return CallOutcome::Resume(resumed);
            }
        }
        if stage.note_tier_entry(function) {
            return CallOutcome::Exit;
        }
    }
    enter_activation(stage, function)
}

/// The activation of `function` where the lane has its shape and room for it.
#[inline(always)]
fn enter_activation<S: WasmActivation>(stage: &mut S, function: u32) -> CallOutcome<S::Resume> {
    let Some(shape) = stage.shape(function) else {
        return CallOutcome::NoRoom;
    };
    let registers = S::shape_registers(shape);
    if stage.activation_room()
        && stage.frame_room()
        && stage.storage_room(registers)
        && stage.values_room(registers)
    {
        CallOutcome::Resume(stage.push_activation(function, shape))
    } else {
        CallOutcome::NoRoom
    }
}

/// A known call: an activation of `function`, or the call's frame where the lane has no room.
/// `None` leaves the lane.
#[inline(always)]
pub fn call_known<S: WasmActivation>(stage: &mut S, function: u32) -> Option<S::Resume> {
    match enter_function(stage, function) {
        CallOutcome::Resume(resumed) => Some(resumed),
        CallOutcome::NoRoom => Some(push_frame(stage)),
        CallOutcome::Exit => None,
    }
}

/// A reference call: its callee is checked first (a function of the running program and
/// instance, or an import), then entered as a known call would be. Any other callee, or no room,
/// leaves the lane.
#[inline(always)]
pub fn call_reference<S: WasmActivation>(stage: &mut S) -> Option<S::Resume> {
    match stage.reference() {
        CallTarget::Function(function) => match enter_function(stage, function) {
            CallOutcome::Resume(resumed) => Some(resumed),
            CallOutcome::NoRoom | CallOutcome::Exit => None,
        },
        CallTarget::Import => call_import(stage),
        CallTarget::Other => None,
    }
}

/// A call native code runs: the caller resumes with its result, or leaves; `None` where the
/// native tier declines the call.
#[cold]
#[inline(never)]
pub(crate) fn call_compiled<S: WasmActivation>(stage: &mut S, function: u32) -> Option<S::Resume> {
    let native = stage.enter_compiled(function);
    native_outcome(stage, native)
}

/// A JavaScript import's call: activated as a JavaScript call where the lane can, else through
/// the import's native boundary. `None` where the import is neither (the general loop calls it).
#[cold]
#[inline(never)]
pub(crate) fn call_import<S: WasmActivation>(stage: &mut S) -> Option<S::Resume> {
    if let Some(resumed) = stage.activate_import() {
        return Some(resumed);
    }
    let native = stage.import_native();
    native_outcome(stage, native)
}

/// Where a native run's outcome continues the caller.
#[inline(always)]
fn native_outcome<S: WasmActivation>(stage: &mut S, native: NativeCall) -> Option<S::Resume> {
    match native {
        NativeCall::Declined => None,
        NativeCall::Left => Some(stage.left()),
        NativeCall::Ran(word) => Some(stage.complete_call(word)),
    }
}

/// The JavaScript number an import passes for a Wasm scalar argument of type `ty` whose word is
/// `word`: an i32 or f32 as its number, an f64 as its bits (ToJSValue, which allocates nothing for
/// these types). The lane reads the word from the register, which may be a Value's payload.
#[inline(always)]
pub fn import_argument(ty: WasmType, word: u64) -> f64 {
    match ty {
        WasmType::I32 => f64::from(word as u32 as i32),
        WasmType::F32 => f64::from(f32::from_bits(word as u32)),
        _ => f64::from_bits(word),
    }
}
