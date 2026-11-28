//! How a lane call enters its target (`ScriptCallTarget::entry`): the call protocol's two lane
//! forms, and the shape each form's entries take.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

pub(crate) use quench_runtime::vm::engine_types::{CallEntry, Framed, Frameless};
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

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

    /// The form of a leaf entry, `NOT_LEAF` for any other (`LEAF_FORMS`).
    #[inline(always)]
    pub(crate) fn leaf_form(self) -> LeafForm {
        LEAF_FORMS[self.table_index()]
    }
}

/// The leaf form of every entry byte (`CallEntry::leaf_form`): the four leaf shapes' forms, and `NOT_LEAF`
/// for every other byte, framed entries among them. One lookup answers it, since the table is indexed by
/// the whole entry byte, its form bit included.
const LEAF_FORMS: [LeafForm; 256] = {
    let mut forms = [NOT_LEAF; 256];
    forms[CallEntry::frameless(Frameless::Leaf).table_index()] = PLAIN_LEAF;
    forms[CallEntry::frameless(Frameless::CaptureLeaf).table_index()] = CAPTURE_LEAF;
    forms[CallEntry::frameless(Frameless::ContextLeaf).table_index()] = CONTEXT_LEAF;
    forms[CallEntry::frameless(Frameless::ThisLeaf).table_index()] = THIS_LEAF;
    forms
};

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
            Self::frameless(Frameless::ReadsCapture),
            Self::frameless(Frameless::ReadsThis),
        ])
    }

    /// A body whose call a read answers (`call_read_in_lane`): its own read.
    #[inline(always)]
    pub(crate) fn answered_by_read(self) -> bool {
        self.reads()
    }
}
