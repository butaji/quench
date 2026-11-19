//! The callees a lane body calls by their entries, and the native frames of those calls.

use super::*;

/// A callee a lane body calls by its compiled entry (`lane_region`'s direct call): the
/// target the lane would activate, fixed by the callee's identity, which the call guards,
/// and its specialized body. The caller's body owns it.
#[derive(Clone)]
pub(in crate::vm) struct DirectCallee {
    pub(in crate::vm) target: super::dispatch_fast::ScriptCallTarget,
    /// The callee's published body (`LaneBody::Specialized`): the caller's code calls its entry,
    /// so the caller retires with it, at a shape compaction (`LaneMethod::names_shapes`) or at
    /// a lifecycle edge (`ProgramStore::retire_dead_native_calls`).
    /// `None` where the call runs what the callee's `cell` holds then, as a call of the function itself.
    pub(in crate::vm) method: Option<Rc<PublishedLaneMethod>>,
    /// Pins the exact entry embedded by the caller, including recovery after retirement.
    pub(in crate::vm) body: Option<Rc<LaneMethod>>,
    /// The caller's call record, which the callee's activation returns to.
    pub(in crate::vm) call: usize,
    pub(in crate::vm) cell: usize,
    pub(in crate::vm) callee: Value,
    pub(in crate::vm) registers: u16,
    /// The global object a sloppy callee binds for an `undefined` or `null` receiver, which
    /// binds a direct object itself (`receiver_this_in_lane`); `None` for a strict callee, which
    /// takes any receiver as it is.
    pub(in crate::vm) sloppy: Option<Value>,
    /// An arrow, which binds no receiver: its body reads its environment's `this`, so a call
    /// passes the deleted marker (`lane_body_call`). Only BaseJIT's direct rows call one.
    pub(in crate::vm) lexical: bool,
    /// The callee's dense capture slots (`EnvironmentSlots::native_dense_view`).
    pub(in crate::vm) capture_values: *const Value,
    pub(in crate::vm) capture_len: usize,
    /// Whether those slots admit every direct store (`dispatch_fast::capture_stores`).
    pub(in crate::vm) capture_stores: bool,
    /// For a construction site, the forwarding constructor whose activation the call skips:
    /// `callee` is that constructor, which the call guards, and `target` its forwarded
    /// initializer, which runs with the receiver the constructor's plan allocates and
    /// whose result the construction ignores (`lane_construct_keeping_activation`).
    pub(in crate::vm) forwarder: Option<super::super::dispatch_fast::SkippedForwarder>,
}

impl DirectCallee {
    /// The callee's dense capture slots, in the one word a body's entry takes them in.
    pub(in crate::vm) fn capture_word(&self) -> u64 {
        super::entry_cell::capture_word(self.capture_values, self.capture_len, self.capture_stores)
    }
}

/// The callees a call site reached that a body may call by their entries, the latest first.
pub(in crate::vm) type DirectCandidates = smallvec::SmallVec<[DirectCallee; tier_policy::POLYMORPHISM]>;

/// Whether code that runs `inlined` as their callees' rows and calls `direct` by their entries
/// names a callee that `live` says died, or a callee body that retired: a collection that frees one
/// retires the code (`ProgramStore::retire_dead_native_calls`).
pub(in crate::vm) fn names_dead_callee(inlined: &[InlinedCall], direct: &[DirectCallee], live: impl Fn(Value) -> bool) -> bool {
    inlined.iter().any(|call| call.target.identity().is_some_and(|callee| !live(callee)))
        || direct.iter().any(|callee| !live(callee.callee) || callee.method.as_ref().is_some_and(|method| !method.is_published()))
}

/// The registers past the caller's `caller_registers` that `inlined` occupy: the scratch window a
/// leave inside one reads (`Vm::scratch_in_lane`), and the room the frames past the caller's registers
/// need.
pub(in crate::vm) fn window_width(inlined: &[InlinedCall], caller_registers: u16) -> usize {
    inlined.iter().map(|frame| usize::from(frame.base - caller_registers) + usize::from(frame.registers)).max().unwrap_or(0)
}

/// The prefix of a native frame of a direct call (`DirectCallee`), which every activation
/// starts with: its callee, which names the call record it returns to, its receiver, the body
/// it runs and its window of registers. Emitted code writes it, with `ActivationKind::Native`,
/// into the lane stack's activations; the lane links the frame (`native_frames`) only where a
/// helper observes it.
#[derive(Clone, Copy)]
#[repr(C)]
pub(in crate::vm) struct ShadowFrame {
    pub(in crate::vm) callee: *const DirectCallee,
    pub(in crate::vm) this: u64, pub(in crate::vm) body: usize,
    /// The address of its window of registers.
    pub(in crate::vm) window: usize,
}

impl ShadowFrame {
    /// The prefix of an activation the lane made: no callee, which tells it from a native
    /// frame's (`LaneActivations::native_depth`), and its window, written at its push.
    pub(in crate::vm) const EMPTY: Self = Self { callee: std::ptr::null(), this: 0, body: 0, window: 0 };
}


const _: () = {
    use crate::staged::frame;
    assert!(std::mem::offset_of!(ShadowFrame, callee) == frame::CALLEE);
    assert!(std::mem::offset_of!(ShadowFrame, this) == frame::THIS);
    assert!(std::mem::offset_of!(ShadowFrame, body) == frame::BODY);
    assert!(std::mem::offset_of!(ShadowFrame, window) == frame::WINDOW);
    assert!(std::mem::size_of::<ShadowFrame>() == frame::PREFIX_BYTES);
    assert!(std::mem::offset_of!(crate::vm::dispatch_fast::Activation, native) == 0);
    assert!(std::mem::size_of::<crate::vm::dispatch_fast::Activation>().is_power_of_two());
};
