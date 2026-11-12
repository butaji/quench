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
    /// binds a direct object itself (`lane_receiver_this`); `None` for a strict callee, which
    /// takes any receiver as it is.
    pub(in crate::vm) sloppy: Option<Value>,
    /// The callee's dense capture slots (`EnvironmentSlots::native_dense_view`).
    pub(in crate::vm) capture_values: *const Value,
    pub(in crate::vm) capture_len: usize,
    /// For a construction site, the forwarding constructor whose activation the call skips:
    /// `callee` is that constructor, which the call guards, and `target` its forwarded
    /// initializer, which runs with the receiver the constructor's plan allocates and
    /// whose result the construction ignores (`lane_construct_keeping_activation`).
    pub(in crate::vm) forwarder: Option<super::super::dispatch_fast::SkippedForwarder>,
}

/// The callees a call site reached that a body may call by their entries, the latest first.
pub(in crate::vm) type DirectCandidates = smallvec::SmallVec<[DirectCallee; tier_policy::POLYMORPHISM]>;

/// A native frame of a direct call (`DirectCallee`), which the lane materializes as an
/// activation only if a run leaves inside it (`Vm::take_shadow_frames`): its callee, which
/// names the call record it returns to, and its receiver. Its registers are the window past
/// the frame below it, at the lane stack's end, so the frame holds neither.
#[derive(Clone, Copy)]
#[repr(C)]
pub(in crate::vm) struct ShadowFrame {
    pub(in crate::vm) callee: *const DirectCallee,
    pub(in crate::vm) this: u64, pub(in crate::vm) body: usize,
    /// The address of its window of registers.
    pub(in crate::vm) window: usize,
}

impl ShadowFrame {
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
};
