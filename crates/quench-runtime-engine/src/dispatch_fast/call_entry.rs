//! How a lane call enters its target (`ScriptCallTarget::entry`): the call protocol's two lane
//! forms, and the shape each form's entries take.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

pub(crate) use quench_runtime::vm::engine_types::{CallEntry, Framed, Frameless};

#[quench_macros::ext(CallEntryOps1)]
impl CallEntry {
    /// Whether the entry is one of `entries`.
    #[inline(always)]
    pub(crate) fn is_any<const N: usize>(self, entries: [Self; N]) -> bool {
        entries.contains(&self)
    }

    /// Whether a call that does not answer it otherwise activates the target as its registers
    /// alone (`activate_in_lane`): a leaf's every call but the plain one does.
    #[inline(always)]
    pub(crate) fn activates(self) -> bool {
        self == Self::framed(Framed::Activates) || self.leaf_form() != NOT_LEAF
    }

    /// Whether a call needs the out-of-line frame path: a target that needs a VM frame, or an
    /// environment or settling layout (`script_call_full_miss`).
    #[inline(always)]
    pub(crate) fn needs_frame_path(self) -> bool {
        self.is_any([Self::framed(Framed::Frame), Self::framed(Framed::Environment), Self::framed(Framed::Settles)])
    }

    /// The leaf entry of a body whose records read the context as `form` says.
    #[inline] pub(crate) fn leaf(form: LeafForm) -> Self {
        Self::frameless(match form {
            CONTEXT_LEAF => Frameless::ContextLeaf,
            CAPTURE_LEAF => Frameless::CaptureLeaf,
            THIS_LEAF => Frameless::ThisLeaf,
            _ => Frameless::Leaf,
        })
    }

    /// The form of a leaf entry, `NOT_LEAF` for any other.
    #[inline(always)]
    pub(crate) fn leaf_form(self) -> LeafForm {
        if self == Self::frameless(Frameless::Leaf) { PLAIN_LEAF }
        else if self == Self::frameless(Frameless::CaptureLeaf) { CAPTURE_LEAF }
        else if self == Self::frameless(Frameless::ContextLeaf) { CONTEXT_LEAF }
        else if self == Self::frameless(Frameless::ThisLeaf) { THIS_LEAF }
        else { NOT_LEAF }
    }
}

#[quench_macros::ext(CallEntryOps2)]
impl CallEntry {
    /// Whether a plain call's activation of the target holds its registers alone: the
    /// entries whose calls a read answers activate like `Framed::Activates` once the read misses,
    /// which is the activation a lane body's inlined call materializes
    /// (`body_inlined_activation_in_lane`).
    pub(crate) fn activates_registers_only(self) -> bool {
        self.activates() || self.answered_by_read()
    }
}

#[quench_macros::ext(CallEntryOps3)]
impl CallEntry {
    /// An entry another program's call activates frameless (`Vm::cross_call_entry`): a plain
    /// or keeping activation, which `activate_keeping_in_lane` enters across programs.
    #[inline(always)]
    pub(crate) fn activates_across(self) -> bool {
        self.activates() || self.is_any([Self::framed(Framed::Keeps), Self::framed(Framed::Gathers)])
    }

    /// A body a call answers by reading alone (`lane_call_leaf_read`).
    #[inline(always)]
    pub(crate) fn reads(self) -> bool {
        self.is_any([
            Self::frameless(Frameless::Reads),
            Self::frameless(Frameless::ReadsElement),
            Self::frameless(Frameless::ReadsCapture),
            Self::frameless(Frameless::ReadsThis),
        ])
    }

    /// A body whose call a read answers (`call_read_in_lane`): its own read, or a forwarded one.
    #[inline(always)]
    pub(crate) fn answered_by_read(self) -> bool {
        self.reads() || self == Self::frameless(Frameless::ForwardsRead)
    }
}
