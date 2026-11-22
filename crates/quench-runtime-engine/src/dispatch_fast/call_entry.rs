//! How a plain lane call enters a frameless target (`ScriptCallTarget::entry`).
use super::*;

pub(crate) use quench_runtime::vm::engine_types::CallEntry;

#[quench_macros::ext(CallEntryOps1)]
impl CallEntry {
    /// Whether a call that does not answer it otherwise activates the target as its registers
    /// alone (`activate_in_lane`): a leaf's every call but the plain one does.
    #[inline(always)]
    pub(crate) fn activates(self) -> bool {
        matches!(self, Self::Activates | Self::Leaf | Self::CaptureLeaf | Self::ContextLeaf | Self::ThisLeaf)
    }

    /// The leaf entry of a body whose records read the context as `form` says.
    #[inline] pub(crate) fn leaf(form: LeafForm) -> Self {
        match form {
            CONTEXT_LEAF => Self::ContextLeaf,
            CAPTURE_LEAF => Self::CaptureLeaf,
            THIS_LEAF => Self::ThisLeaf,
            _ => Self::Leaf,
        }
    }

    /// The form of a leaf entry, `NOT_LEAF` for any other.
    #[inline(always)]
    pub(crate) fn leaf_form(self) -> LeafForm {
        match self {
            Self::Leaf => PLAIN_LEAF,
            Self::CaptureLeaf => CAPTURE_LEAF,
            Self::ContextLeaf => CONTEXT_LEAF,
            Self::ThisLeaf => THIS_LEAF,
            _ => NOT_LEAF,
        }
    }
}

#[quench_macros::ext(CallEntryOps2)]
impl CallEntry {
    /// Whether a plain call's activation of the target holds its registers alone: the
    /// entries whose calls a read answers activate like `Activates` once the read misses,
    /// which is the activation a lane body's inlined call materializes
    /// (`body_inlined_activation_in_lane`).
    pub(crate) fn activates_registers_only(self) -> bool {
        self.activates() || matches!(self, Self::Reads | Self::ReadsElement | Self::ForwardsRead | Self::ReadsCapture | Self::ReadsThis)
    }
}

#[quench_macros::ext(CallEntryOps3)]
impl CallEntry {
    /// An entry another program's call activates frameless (`Vm::cross_call_entry`): a plain
    /// or keeping activation, which `activate_keeping_in_lane` enters across programs.
    #[inline(always)]
    pub(crate) fn activates_across(self) -> bool {
        matches!(self, Self::Activates | Self::Keeps | Self::Gathers | Self::Leaf | Self::CaptureLeaf | Self::ContextLeaf | Self::ThisLeaf)
    }

    /// A body a call answers by reading alone (`lane_call_leaf_read`).
    #[inline(always)]
    pub(crate) fn reads(self) -> bool {
        matches!(self, Self::Reads | Self::ReadsElement | Self::ReadsCapture | Self::ReadsThis)
    }

    /// A body whose call a read answers (`call_read_in_lane`): its own read, or a forwarded one.
    #[inline(always)]
    pub(crate) fn answered_by_read(self) -> bool {
        self.reads() || self == Self::ForwardsRead
    }
}
