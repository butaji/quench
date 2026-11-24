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

/// The primitives of a Wasm call's native entry (`enter_native`), which both interpreters make: the
/// lane (`vm/dispatch_fast/wasm_calls.rs`) and the general loop (`vm/dispatch/call_forms.rs`).
pub trait WasmNativeEntry {
    /// Where a call continues (the lane's `Resumed`, the general loop's step result).
    type Resume;
    /// Whether the call may run natively: the execution mode is compiled and the call keeps its result.
    fn native_mode(&mut self) -> bool;
    /// The tiers' answer to this entry of `function` (`offer_entry` over the interpreter's
    /// tiers); `Held` keeps the code for `enter_compiled`.
    fn offer(&mut self, function: u32) -> EntryOffer<()>;
    /// Runs `function` in native code, with the call's window as its arguments.
    fn enter_compiled(&mut self, function: u32) -> NativeCall;
    /// Leaves for the general loop, at the call (the general loop: raises what the native run raised).
    fn left(&mut self) -> Self::Resume;
    /// Completes the call with the word of its result, in the caller.
    fn complete_call(&mut self, word: u64) -> Self::Resume;
}

/// The primitives a Wasm call's rules run on: the lane's state, its activations and its native
/// tiers. Every method is one check or one step the lane takes; the rules decide their order.
pub trait WasmActivation: WasmStage + WasmNativeEntry {
    /// The activation shape a callee's frame takes, copied out of its view.
    type Shape: Copy;
    /// The callee of the reference the call names.
    fn reference(&mut self) -> CallTarget;
    /// Whether the lane may make calls inline (the frame is not leaving for native code).
    fn inline_calls(&mut self) -> bool;
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
/// code where a tier holds it, out of the lane where a compile came due (the general loop makes
/// it), else as an activation. The guard order is the lane's: depth, the frames and activations
/// together, storage, then the value stack's room.
#[inline(always)]
pub(crate) fn enter_function<S: WasmActivation>(stage: &mut S, function: u32) -> CallOutcome<S::Resume> {
    if !stage.inline_calls() {
        return CallOutcome::Exit;
    }
    match enter_native(stage, function) {
        NativeEntry::Ran(resumed) => CallOutcome::Resume(resumed),
        NativeEntry::Leave => CallOutcome::Exit,
        NativeEntry::Interpret => enter_activation(stage, function),
    }
}

/// Where a Wasm call's native entry (`enter_native`) leaves the call.
pub enum NativeEntry<R> {
    /// Native code ran it: the caller resumes, or leaves, as `R` says.
    Ran(R),
    /// A compile came due that this interpreter does not make: the general loop takes the call.
    Leave,
    /// The interpreter runs it.
    Interpret,
}

/// A known call's native entry, the rule both interpreters make first: where the call may run
/// natively, the tiers answer the entry (`offer_entry`), native code runs what they hold, and the
/// caller resumes with its result or leaves. A tier that declines the run leaves the call to the
/// interpreter: the lane activates it, the general loop pushes its frame.
#[inline(always)]
pub fn enter_native<S: WasmNativeEntry>(stage: &mut S, function: u32) -> NativeEntry<S::Resume> {
    if !stage.native_mode() {
        return NativeEntry::Interpret;
    }
    match stage.offer(function) {
        EntryOffer::Held(()) => call_compiled(stage, function).map_or(NativeEntry::Interpret, NativeEntry::Ran),
        EntryOffer::Leave => NativeEntry::Leave,
        EntryOffer::Interpret => NativeEntry::Interpret,
    }
}

/// What the tiers answer one Wasm entry (`offer_entry`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryOffer<C> {
    /// A tier holds the function: native code runs it.
    Held(C),
    /// A compile came due that this interpreter hands to the general loop.
    Leave,
    /// No tier runs it: the interpreter does.
    Interpret,
}

impl<C> EntryOffer<C> {
    /// The offer with its code kept in `slot`, for the run that follows (`enter_compiled`).
    #[inline(always)]
    pub fn keep(self, slot: &mut Option<C>) -> EntryOffer<()> {
        match self {
            Self::Held(code) => {
                *slot = Some(code);
                EntryOffer::Held(())
            }
            Self::Leave => EntryOffer::Leave,
            Self::Interpret => EntryOffer::Interpret,
        }
    }
}

/// What an interpreter did with a compile an entry made due.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Compile {
    /// It compiled (the artifact may hold the function or not).
    Made,
    /// It made no compile: the attempt was claimed elsewhere, or the budget refused it.
    Refused,
    /// It does not compile here: the general loop does, at the same call.
    Leave,
}

/// The tiers an interpreter offers a Wasm entry to: the module's whole compile, its root tiers and
/// their counters. Each method is one lookup, one count or one compile; `offer_entry` orders them.
pub trait WasmEntryTiers {
    /// What runs a held function (the artifact the tier keeps).
    type Code;
    /// The module's whole compile, where it holds `function`.
    fn whole(&mut self, function: u32) -> Option<Self::Code>;
    /// Counts one entry's work toward the module's whole compile; whether that compile is due.
    fn note_module_work(&mut self) -> bool;
    /// Makes the module's whole compile, which `note_module_work` found due.
    fn compile_whole(&mut self) -> Compile;
    /// A root tier holding `function`.
    fn root(&mut self, function: u32) -> Option<Self::Code>;
    /// Whether a root tier may take `function` (`ProgramStore::wasm_root_admissible`).
    fn root_admissible(&mut self, function: u32) -> bool;
    /// Counts one entry of `function` toward its root compile; whether the compile is due.
    fn note_root_entry(&mut self, function: u32) -> bool;
    /// Makes the root compile of `function`, which `note_root_entry` found due.
    fn compile_root(&mut self, function: u32) -> Compile;
    /// Notes that the interpreter runs `function` though the module has a whole compile.
    fn note_fallback(&mut self, function: u32);
}

/// The tiers' answer to one entry of `function`, the compile trigger of both interpreters: the
/// whole compile's code; else one entry's work counted toward the whole compile, made where it
/// comes due; else a root tier's code; else one entry counted toward the function's own root
/// compile, made where it comes due; else the interpreter. An interpreter that does not compile
/// (the lane) leaves at a due compile, and the general loop, offered the same call, makes it.
#[inline(always)]
pub fn offer_entry<T: WasmEntryTiers>(tiers: &mut T, function: u32) -> EntryOffer<T::Code> {
    if let Some(code) = tiers.whole(function) {
        return EntryOffer::Held(code);
    }
    if tiers.note_module_work() {
        match tiers.compile_whole() {
            Compile::Leave => return EntryOffer::Leave,
            Compile::Made => if let Some(code) = tiers.whole(function) {
                return EntryOffer::Held(code);
            },
            Compile::Refused => {}
        }
    }
    if let Some(code) = tiers.root(function) {
        return EntryOffer::Held(code);
    }
    if tiers.root_admissible(function) && tiers.note_root_entry(function) {
        match tiers.compile_root(function) {
            Compile::Leave => return EntryOffer::Leave,
            Compile::Made => if let Some(code) = tiers.root(function) {
                return EntryOffer::Held(code);
            },
            Compile::Refused => {}
        }
    }
    tiers.note_fallback(function);
    EntryOffer::Interpret
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
pub(crate) fn call_compiled<S: WasmNativeEntry>(stage: &mut S, function: u32) -> Option<S::Resume> {
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
fn native_outcome<S: WasmNativeEntry>(stage: &mut S, native: NativeCall) -> Option<S::Resume> {
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

/// What a JavaScript import returns to its Wasm caller for the Number `number`, for the scalar
/// result type `ty` (ToWebAssemblyValue of a Number): an i32 is its ToInt32, an f32 its rounding,
/// an f64 itself. `None` for any other type, whose conversion is not of a Number alone. One body
/// for the lane's import return and the general loop's (`Vm::js_to_wasm_value`).
#[inline(always)]
pub fn import_result(ty: WasmType, number: f64) -> Option<crate::WasmValue> {
    match ty {
        WasmType::I32 => Some(crate::WasmValue::I32(crate::value::number_to_u32(number) as i32)),
        WasmType::F32 => Some(crate::WasmValue::F32((number as f32).to_bits())),
        WasmType::F64 => Some(crate::WasmValue::F64(number.to_bits())),
        _ => None,
    }
}
