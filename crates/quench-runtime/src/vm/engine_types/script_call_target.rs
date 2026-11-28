//! A frameless call target (`ScriptCallTarget`): what a body fixes about its calls (`FunctionTarget`) and what
//! the closure a call reaches brings (`ClosureOperands`).
use super::*;

/// A user function a lane `Call` may activate without a frame, valid as the general loop's
/// `InLoopCallTarget` is: for `closure.callee` while the collection epoch and realm it was resolved
/// under hold, which its `body.admission` names. A target serves calls from its program, the
/// callee's own, or, held apart (`LaneStack::cross_targets`), from any other program, whose
/// calls it only activates, switching the lane to its program (`enter_program_in_lane`).
/// What its function's body fixes (`body`) and what the closure called brings (`closure`) are
/// held apart: the first is a compile-time fact of the body, the second a run-time input of the call.
#[derive(Clone, Copy)]
pub struct ScriptCallTarget {
    pub body: FunctionTarget,
    pub closure: ClosureOperands,
}

/// What a body fixes about every frameless call of a closure of it (`ScriptCallTarget::body`):
/// the same for each closure of the function under one generation and program, which the
/// function's cache entry holds (`LaneStack::function_targets`).
#[derive(Clone, Copy)]
pub struct FunctionTarget {
    /// The generation it was resolved in and the program it serves (`Admission`).
    pub admission: Admission,
    pub function: u32,
    pub view: *const LaneView,
    pub registers: u16,
    /// The lane-stack room its plain activation reserves (`fill_room::<SCRIPT_FILL_BLOCK>` of
    /// `registers`), derived once.
    pub room: u16,
    pub params: u16,
    /// How a call binds its `this` (`ThisBinding`). An arrow's is `Lexical` or `CallerReceiver`
    /// as its closure's environment holds a `this` or not (`ClosureOperands::lexical_this`).
    pub this_binding: ThisBinding,
    /// `TARGET_CONSTRUCTS` where `new` may activate it too (`Vm::inline_constructible`), and
    /// `TARGET_INITIALIZES` where `new` may run its body as the stores alone
    /// (`Function::only_initializes_this_fields`).
    pub forms: u8,
    /// The registers its plain activation starts `undefined` (`ResidualProgram::entry_fill`),
    /// at most `u8::MAX`: past one `WINDOW_FILL_BLOCK` it starts every register so.
    pub fill: u8,
    /// How a plain call enters it.
    pub entry: CallEntry,
    /// Whether the target is a constructor body that forwards its construction to a method of
    /// `this` under its call plan (`CallPlan::forwards_arguments`) and shape. Which method is
    /// read from the function's record (`dispatch_fast::forwarded_method_of`), so a target
    /// keeps its cache entry's size.
    pub forwards_method: bool,
}

/// What the closure a call reaches brings to its activation (`ScriptCallTarget::closure`): its
/// identity, its environment and what is read from it, rebuilt for each closure of a function
/// (`Vm::closure_target`).
#[derive(Clone, Copy)]
pub struct ClosureOperands {
    pub callee: Value,
    pub env: Value,
    /// The `this` a plain call (an `undefined` receiver) binds: an arrow's own
    /// (`ThisBinding::Lexical`), found in its environment when it was resolved, the realm's
    /// global object for a sloppy function, `undefined` for any other binding. A plain call
    /// reads it in one load (`plain_call_activation`).
    pub lexical_this: Value,
    /// The dense storage header its depth-0 captures read (`LaneContext::capture`), or null where the walk
    /// does not end at a function environment or its storage is sparse. The storage of a live environment never
    /// moves, and the generation retires the target at the next collection.
    pub capture: *const usize,
}

/// Where the call row's one guard (`staged::call::guard_bound_target`) reads a `ScriptCallTarget`: its
/// callee word and its admission word, and the entry's size, for a tier that addresses the entries itself.
pub const SCRIPT_TARGET_CALLEE_OFFSET: usize = core::mem::offset_of!(ScriptCallTarget, closure.callee);
pub const SCRIPT_TARGET_ADMISSION_OFFSET: usize = core::mem::offset_of!(ScriptCallTarget, body.admission);
pub const SCRIPT_TARGET_BYTES: usize = core::mem::size_of::<ScriptCallTarget>();

pub const EMPTY_SCRIPT_CALL_TARGET: ScriptCallTarget = ScriptCallTarget { body: EMPTY_FUNCTION_TARGET, closure: EMPTY_CLOSURE_OPERANDS };

pub const EMPTY_FUNCTION_TARGET: FunctionTarget = FunctionTarget {
    // Never proved, so no proved call's compare can match it (`Admission::UNPROVEN`).
    admission: Admission(Admission::new(NO_SCRIPT_GENERATION, ProgramId::MAIN).0 | Admission::UNPROVEN),
    function: 0,
    view: std::ptr::null(),
    registers: 0,
    room: 0,
    params: 0,
    this_binding: ThisBinding::Sloppy,
    forms: 0,
    fill: 0,
    entry: CallEntry::framed(Framed::Activates),
    forwards_method: false,
};

pub const EMPTY_CLOSURE_OPERANDS: ClosureOperands = ClosureOperands {
    callee: Value::UNDEFINED,
    env: Value::UNDEFINED,
    lexical_this: Value::UNDEFINED,
    capture: std::ptr::null(),
};
