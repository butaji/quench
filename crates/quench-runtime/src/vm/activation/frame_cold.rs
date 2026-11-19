//! What few suspended frames hold, boxed off their record (`FrameRecord::cold`).

use crate::bytecode::Atom;
use crate::Value;

/// A frame record's rarely held state: the arguments legacy reflection or an `arguments`
/// object keeps, the dynamic bindings a sloppy direct eval makes, the `with` objects of its
/// scope chain and a direct eval's binding site. Inline, they took every suspended generator
/// and async function 88 bytes of its record; boxed, 8, the box made only for a frame that
/// holds any of them.
#[derive(Clone, Debug)]
pub(in crate::vm) struct FrameCold {
    pub(in crate::vm) reflected_parameters: u16,
    pub(in crate::vm) original_arguments: Vec<Value>,
    pub(in crate::vm) dynamic_bindings: Vec<(Atom, Value)>,
    /// The scope stack the frame detached when it suspended (`FrameRecord::with_objects`).
    pub(in crate::vm) with_objects: Vec<Value>,
    /// The resume pc of the binding site the direct eval the activation is running was called
    /// at (`CallerView::binding_site`).
    pub(in crate::vm) binding_site_pc: Option<u32>,
}

/// The state of a record with no box.
pub(super) static NO_COLD: FrameCold = FrameCold::EMPTY;

impl FrameCold {
    pub(super) const EMPTY: Self = Self {
        reflected_parameters: 0,
        original_arguments: Vec::new(),
        dynamic_bindings: Vec::new(),
        with_objects: Vec::new(),
        binding_site_pc: None,
    };

    /// Whether it holds nothing a record with no box would not.
    pub(super) fn is_empty(&self) -> bool {
        self.reflected_parameters == 0
            && self.original_arguments.is_empty()
            && self.dynamic_bindings.is_empty()
            && self.with_objects.is_empty()
            && self.binding_site_pc.is_none()
    }

    /// The box of a record holding this: none where it holds nothing, its buffers freed.
    pub(super) fn boxed(self) -> Option<Box<Self>> {
        (!self.is_empty()).then(|| Box::new(self))
    }

    /// The parts of `cold`, a record's box or none.
    pub(super) fn unbox(cold: Option<Box<Self>>) -> Self {
        cold.map_or(Self::EMPTY, |cold| *cold)
    }
}
