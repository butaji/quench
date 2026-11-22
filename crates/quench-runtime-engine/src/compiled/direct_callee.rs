//! The callees a lane body calls by their entries, and the native frames of those calls.

use super::*;

/// A callee a lane body calls by its compiled entry (`lane_region`'s direct call): the
/// target the lane would activate, fixed by the callee's identity, which the call guards,
/// and its specialized body. The caller's body owns it.
#[derive(Clone)]
pub(crate) struct DirectCallee {
    pub(crate) target: super::dispatch_fast::ScriptCallTarget,
    /// The callee's published body (`LaneBody::Specialized`): the caller's code calls its entry,
    /// so the caller retires with it, at a shape compaction (`LaneMethod::names_shapes`) or at
    /// a lifecycle edge (`ProgramStore::retire_dead_native_calls`).
    /// `None` where the call runs what the callee's `cell` holds then, as a call of the function itself.
    pub(crate) method: Option<Rc<PublishedLaneMethod>>,
    /// Pins the exact entry embedded by the caller, including recovery after retirement.
    pub(crate) body: Option<Rc<LaneMethod>>,
    /// The caller's call record, which the callee's activation returns to.
    pub(crate) call: usize,
    /// The caller's base instruction pointer (its view's records), which the callee's header saves for the return.
    pub(crate) caller_base: usize,
    pub(crate) cell: usize,
    pub(crate) callee: Value,
    pub(crate) registers: u16,
    /// The global object a sloppy callee binds for an `undefined` or `null` receiver, which
    /// binds a direct object itself (`receiver_this_in_lane`); `None` for a strict callee, which
    /// takes any receiver as it is.
    pub(crate) sloppy: Option<Value>,
    /// An arrow, which binds no receiver: its body reads its environment's `this`, so a call
    /// passes the deleted marker (`body_call_helper`). Only BaseJIT's direct rows call one.
    pub(crate) lexical: bool,
    /// The callee's dense capture slots (`EnvironmentSlots::native_dense_view`).
    pub(crate) capture_values: *const Value,
    pub(crate) capture_len: usize,
    /// Whether those slots admit every direct store (`dispatch_fast::capture_stores`).
    pub(crate) capture_stores: bool,
    /// For a construction site, the forwarding constructor whose activation the call skips:
    /// `callee` is that constructor, which the call guards, and `target` its forwarded
    /// initializer, which runs with the receiver the constructor's plan allocates and
    /// whose result the construction ignores (`construct_keeping_activation_in_lane`).
    pub(crate) forwarder: Option<crate::dispatch_fast::SkippedForwarder>,
}

impl DirectCallee {
    /// The type summary of the callee's published body (`lane_region::TypeSummary`): its entry
    /// parameter types, which the call's arguments must hold for its proven entry, and its result,
    /// which types the call's result. `None` where the callee has no published body or its body has none.
    pub(crate) fn summary(&self) -> Option<lane_region::TypeSummary> {
        self.method.as_ref()?.lease()?.summary().cloned()
    }

    /// The callee's dense capture slots, in the one word a body's entry takes them in.
    pub(crate) fn capture_word(&self) -> u64 {
        super::entry_cell::capture_word(self.capture_values, self.capture_len, self.capture_stores)
    }
}

/// The callees a call site reached that a body may call by their entries, the latest first.
pub(crate) type DirectCandidates = smallvec::SmallVec<[DirectCallee; tier_policy::POLYMORPHISM]>;

/// Whether code that runs `inlined` as their callees' rows and calls `direct` by their entries
/// names a callee that `live` says died, or a callee body that retired: a collection that frees one
/// retires the code (`ProgramStore::retire_dead_native_calls`).
pub(crate) fn names_dead_callee(inlined: &[InlinedCall], direct: &[DirectCallee], live: impl Fn(Value) -> bool) -> bool {
    inlined.iter().any(|call| call.target.identity().is_some_and(|callee| !live(callee)))
        || direct.iter().any(|callee| !live(callee.callee) || callee.method.as_ref().is_some_and(|method| !method.is_published()))
}

/// The registers past the caller's `caller_registers` that `inlined` occupy: the scratch window a
/// leave inside one reads (`Vm::scratch_in_lane`), and the room the frames past the caller's registers
/// need.
pub(crate) fn window_width(inlined: &[InlinedCall], caller_registers: u16) -> usize {
    frames_window(inlined.iter().map(|frame| (frame.base, frame.registers)), caller_registers)
}

/// `window_width` of frames given by their first register and their count, for a region whose
/// frames are not `InlinedCall`s (`Inlining::window`).
pub(crate) fn frames_window(frames: impl Iterator<Item = (quench_runtime::bytecode::Register, u16)>, caller_registers: u16) -> usize {
    frames.map(|(base, registers)| usize::from(base - caller_registers) + usize::from(registers)).max().unwrap_or(0)
}

