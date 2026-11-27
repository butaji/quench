//! The calls a Wasm lane enters as activations, written once over `WasmActivation`: a known
//! call, a reference call (its callee check, then the same entry), a call a native tier runs,
//! and a JavaScript import's call. The rules hold the order of the lane's checks; the stage
//! supplies the lane's state and the primitives (`vm/dispatch_fast/wasm_calls.rs`), so a call
//! takes the path it always took. The conversion of a scalar a JavaScript import passes is
//! `import_argument`, one body for the lane and every tier.

use super::wasm::WasmStage;
use crate::WasmType;

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
    /// The address of the caller's registers, where the call's window lies.
    fn caller_registers(&mut self) -> u64;
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

/// The primitives of a Wasm activation's return (`step_wasm_return_of`) and of a JavaScript
/// import's return to its Wasm caller (`step_import_return_of`): the lane's innermost activation
/// and its caller's state. Every method is one read or one step the lane takes; the rules decide
/// their order.
pub trait WasmReturn {
    /// Where the caller continues (the lane's `Resumed`).
    type Resume;
    /// The activation popped off the lane's stack, with what its caller resumes in.
    type Popped;
    /// A value: the return's, and the caller's form of it.
    type Value: Copy;
    /// The result type an export's caller converts the value to.
    type Export: Copy;
    /// What an import's Wasm caller resumes in: its result type, program, instance and memory.
    type Import: Copy;
    /// The innermost activation's completion test: whether it has a script completion of its
    /// own, and its call record's result word. `None` where no activation runs.
    fn innermost(&mut self) -> Option<(bool, u64)>;
    /// Pops the innermost activation and restores its caller's registers, records and memory.
    fn pop(&mut self) -> Self::Popped;
    /// The result type of the export whose activation `popped` was, if it was one.
    fn export_result(&mut self, popped: &Self::Popped) -> Option<Self::Export>;
    /// Enters the caller's own program as the run's JavaScript (an export's caller).
    fn enter_script_caller(&mut self, popped: &Self::Popped);
    /// The JavaScript form of the Wasm result `value` of type `export`.
    fn export_value(&mut self, value: Self::Value, export: Self::Export) -> Self::Value;
    /// The innermost activation's import caller, read before it pops.
    fn import(&mut self) -> Self::Import;
    /// The import's result in its Wasm caller's form (`import_result`), or `None` where the
    /// conversion could run guest code or throw.
    fn import_value(&mut self, import: Self::Import, value: Self::Value) -> Option<Self::Value>;
    /// Enters the caller's own program, instance and memory as the run's Wasm (an import's caller);
    /// the caller resumes in that memory, which the callee may have grown.
    fn enter_wasm_caller(&mut self, import: Self::Import, popped: &mut Self::Popped);
    /// Leaves the return to the callee's frame, whose pending call converts `value`.
    fn return_from_frame(&mut self, value: Self::Value) -> Self::Resume;
    /// Resumes the caller past its call with `value` in the call's result register.
    fn resume_caller(&mut self, popped: Self::Popped, value: Self::Value) -> Self::Resume;
}

/// The return with `value` of the innermost activation, a Wasm one whose call keeps its result
/// (no script completion, and a call that does not return its callee's completion): it pops, and
/// its caller, Wasm, or JavaScript for an export's activation, resumes after the call with
/// `value`, in its JavaScript form for an export. `None`, which changes nothing, for any other.
#[inline(always)]
pub fn step_wasm_return_of<S: WasmReturn>(stage: &mut S, value: S::Value) -> Option<S::Resume> {
    let (completes, result) = stage.innermost()?;
    if completes || super::call_complete::returns_callee_completion(&mut super::Concrete::pure(), result) {
        return None;
    }
    let popped = stage.pop();
    let value = match stage.export_result(&popped) {
        Some(export) => {
            stage.enter_script_caller(&popped);
            stage.export_value(value, export)
        }
        None => value,
    };
    Some(stage.resume_caller(popped, value))
}

/// The return with `value` of the innermost activation, a JavaScript import's callee: its Wasm
/// caller resumes after the call with the value's Wasm form, in its own program, instance and
/// memory. A value whose conversion could run guest code or throw returns from the callee's
/// frame, whose pending call converts it.
#[inline(always)]
pub fn step_import_return_of<S: WasmReturn>(stage: &mut S, value: S::Value) -> S::Resume {
    let import = stage.import();
    let Some(converted) = stage.import_value(import, value) else {
        return stage.return_from_frame(value);
    };
    let mut popped = stage.pop();
    stage.enter_wasm_caller(import, &mut popped);
    stage.resume_caller(popped, converted)
}

/// The primitives of a lane's JavaScript call of an exported Wasm function (`step_export_call_of`)
/// and of its frame's return (`step_export_frame_return_of`); the rules decide their order.
pub trait WasmExportCall {
    /// Where the run continues.
    type Resume;
    /// The export the call resolved: its program, function, instance and encoded arguments.
    type Export;
    /// The export frame's pending call, popped at its return.
    type Pending;
    /// The returned value.
    type Value: Copy;
    /// Whether the lane makes the call: the run is a frame's own JavaScript run, a returning call
    /// completes a caller the lane completes, and native code may not run the callee.
    fn admits(&mut self) -> bool;
    /// The export the call names, where the lane admits its program and function.
    fn export(&mut self) -> Option<Self::Export>;
    /// The call as a frameless Wasm activation of the export, where its view and room allow.
    fn export_activation(&mut self, export: &Self::Export) -> Option<Self::Resume>;
    /// The call as the export's frame, pushed with the pending call that converts its result.
    fn export_frame(&mut self, export: Self::Export) -> Self::Resume;
    /// Retires the export's frame and pops its pending call; the caller's program runs again.
    fn retire_frame(&mut self) -> Self::Pending;
    /// The JavaScript form of the frame's encoded result.
    fn export_value(&mut self, pending: &Self::Pending, value: Self::Value) -> Self::Value;
    /// Whether the call returns its callee's completion, so the caller completes too.
    fn caller_returns(pending: &Self::Pending) -> bool;
    /// The caller's return of `value` at its call record.
    fn complete_caller(&mut self, pending: Self::Pending, value: Self::Value) -> Self::Resume;
    /// The call's register takes `value`, and the caller's frame resumes.
    fn resume_caller(&mut self, pending: Self::Pending, value: Self::Value) -> Self::Resume;
}

/// A JavaScript call of an exported Wasm function: where the lane admits it, a frameless
/// activation of the export, else the export's frame. `None` leaves the call to the native.
#[inline(always)]
pub fn step_export_call_of<S: WasmExportCall>(stage: &mut S) -> Option<S::Resume> {
    if !stage.admits() {
        return None;
    }
    let export = stage.export()?;
    if let Some(resumed) = stage.export_activation(&export) {
        return Some(resumed);
    }
    Some(stage.export_frame(export))
}

/// The return with its encoded `value` of an export's frame a JavaScript call pushed: the frame
/// retires, and the call's register takes the value's JavaScript form, or, for a call that
/// completes its caller, the caller returns it.
#[inline(always)]
pub fn step_export_frame_return_of<S: WasmExportCall>(stage: &mut S, value: S::Value) -> S::Resume {
    let pending = stage.retire_frame();
    let value = stage.export_value(&pending, value);
    if S::caller_returns(&pending) { stage.complete_caller(pending, value) } else { stage.resume_caller(pending, value) }
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
