//! How a plain lane call enters a frameless target (`ScriptCallTarget::entry`).
use super::*;

/// How a plain lane call enters a frameless target (`ScriptCallTarget::entry`), one byte so the
/// call tests it once. The kinds exclude each other: a body that is one read keeps nothing.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum CallEntry {
    /// Its activation holds its registers alone (`activate_in_lane`).
    Activates,
    /// It keeps the values it was passed for a forwarded `arguments`
    /// (`CallPlan::forwards_arguments`, `activate_keeping_in_lane`).
    Keeps,
    /// It keeps them, and its rest parameter, in the register after the others, starts as
    /// the array of those past the others (`activate_keeping_in_lane`).
    Gathers,
    /// Its body is one returning `GetField` (`returning_kind` of its first instruction),
    /// whose hit a call answers without activating it (`call_read_in_lane`).
    Reads,
    /// It needs a VM frame (`CallPlan::registers_only` fails, or its view has no
    /// activation): a call takes `frame_call_in_lane`. Only the cache and the call's own
    /// out-of-line form see such a target (`script_target_in_lane` leaves it out).
    Frame,
    /// Its body reads a field of `this` or a parameter, then an element of that by a
    /// parameter, and returns it (`element_read_body`), which a call answers likewise.
    ReadsElement,
    /// Its body returns a call of a method of a field of `this` or a parameter with its
    /// parameters in order (`forwarding_read_body`), which a call answers where that method
    /// is a `Reads` or `ReadsElement` target whose read hits.
    ForwardsRead,
    /// Its body returns one of its captures (`capture_read_body`), as a bundler's export
    /// getter `() => binding` compiles, which a call answers by reading the capture.
    ReadsCapture,
    /// Its body returns its `this` (`this_read_body`), as `get self() { return this; }`
    /// compiles, which a call answers with the receiver it binds.
    ReadsThis,
    /// It keeps frame locals and makes closures (`CallPlan::environment_activation`): its
    /// activation's window holds its environment and locals after its registers
    /// (`activate_environment_in_lane`). Only a plain call activates it.
    Environment,
    /// An async function whose body never suspends, laid out as its registers
    /// (`CallPlan::settles_registers_only`): only a plain call of its own program activates
    /// it (`script_call_settles_miss`), whose promise its return settles
    /// (`Completion::SettlesPromise`).
    Settles,
    /// Its body is a leaf (`leaf_body`): a plain call of its own program runs its records on
    /// a window of the lane stack without an activation (`script_call_leaf_miss`), which
    /// anything observing activations makes first (`materialize_leaf_in_lane`). Any other
    /// call activates it as `Activates`.
    Leaf,
    /// A leaf whose records read its captures and not its receiver (`CAPTURE_LEAF`), so its
    /// call makes the context's environment its own, which its return restores.
    CaptureLeaf,
    /// A leaf whose records read its receiver (`CONTEXT_LEAF`), so its call makes the
    /// context's receiver and environment its own, which its return restores.
    ContextLeaf,
    /// A leaf whose records read its receiver and not its captures (`THIS_LEAF`), so its
    /// call binds the receiver, which its return restores, and keeps the caller's environment.
    ThisLeaf,
    /// A proven conditional append to a captured binding, completed without activation
    /// (`capture_append::run`).
    AppendsCapture,
}

impl CallEntry {
    /// Whether a call that does not answer it otherwise activates the target as its registers
    /// alone (`activate_in_lane`): a leaf's every call but the plain one does.
    #[inline(always)]
    pub(super) fn activates(self) -> bool {
        matches!(self, Self::Activates | Self::Leaf | Self::CaptureLeaf | Self::ContextLeaf | Self::ThisLeaf)
    }

    /// The leaf entry of a body whose records read the context as `form` says.
    #[inline] pub(super) fn leaf(form: LeafForm) -> Self {
        match form {
            CONTEXT_LEAF => Self::ContextLeaf,
            CAPTURE_LEAF => Self::CaptureLeaf,
            THIS_LEAF => Self::ThisLeaf,
            _ => Self::Leaf,
        }
    }

    /// The form of a leaf entry, `NOT_LEAF` for any other.
    #[inline(always)]
    pub(super) fn leaf_form(self) -> LeafForm {
        match self {
            Self::Leaf => PLAIN_LEAF,
            Self::CaptureLeaf => CAPTURE_LEAF,
            Self::ContextLeaf => CONTEXT_LEAF,
            Self::ThisLeaf => THIS_LEAF,
            _ => NOT_LEAF,
        }
    }
}

impl CallEntry {
    /// Whether a plain call's activation of the target holds its registers alone: the
    /// entries whose calls a read answers activate like `Activates` once the read misses,
    /// which is the activation a lane body's inlined call materializes
    /// (`body_inlined_activation_in_lane`).
    pub(super) fn activates_registers_only(self) -> bool {
        self.activates() || matches!(self, Self::Reads | Self::ReadsElement | Self::ForwardsRead | Self::ReadsCapture | Self::ReadsThis)
    }
}

impl CallEntry {
    /// An entry another program's call activates frameless (`Vm::cross_call_entry`): a plain
    /// or keeping activation, which `activate_keeping_in_lane` enters across programs.
    #[inline(always)]
    pub(super) fn activates_across(self) -> bool {
        matches!(self, Self::Activates | Self::Keeps | Self::Gathers | Self::Leaf | Self::CaptureLeaf | Self::ContextLeaf | Self::ThisLeaf)
    }

    /// A body a call answers by reading alone (`lane_call_leaf_read`).
    #[inline(always)]
    pub(super) fn reads(self) -> bool {
        matches!(self, Self::Reads | Self::ReadsElement | Self::ReadsCapture | Self::ReadsThis)
    }

    /// A body whose call a read answers (`call_read_in_lane`): its own read, or a forwarded one.
    #[inline(always)]
    pub(super) fn answered_by_read(self) -> bool {
        self.reads() || self == Self::ForwardsRead
    }
}
