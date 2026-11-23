//! What few suspended frames hold, boxed off their record (`FrameRecord::cold`).

use super::FrameRecord;
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
static NO_COLD: FrameCold = FrameCold::EMPTY;

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

/// The rarely held state of a record (`FrameRecord::cold`).
impl FrameRecord {
    /// Its rarely held state, the empty one where it holds none.
    #[inline(always)]
    pub(super) fn cold(&self) -> &FrameCold {
        self.cold.as_deref().unwrap_or(&NO_COLD)
    }

    /// Its rarely held state, boxed now where it held none.
    pub(super) fn cold_mut(&mut self) -> &mut FrameCold {
        self.cold.get_or_insert_with(|| Box::new(FrameCold::EMPTY))
    }

    /// The arguments it keeps for an `arguments` object or legacy reflection.
    #[cfg(test)]
    pub(in crate::vm) fn original_arguments(&self) -> &[Value] {
        &self.cold().original_arguments
    }

    /// The dynamic bindings it holds itself (implicit ones, a direct eval's).
    pub(in crate::vm) fn dynamic_bindings(&self) -> &[(Atom, Value)] {
        &self.cold().dynamic_bindings
    }

    /// `dynamic_bindings`, to change.
    pub(in crate::vm) fn dynamic_bindings_mut(&mut self) -> &mut Vec<(Atom, Value)> {
        &mut self.cold_mut().dynamic_bindings
    }

    /// The dynamic bindings it holds itself, taken: an environment takes them over.
    pub fn take_dynamic_bindings(&mut self) -> Vec<(Atom, Value)> {
        self.cold.as_mut().map(|cold| std::mem::take(&mut cold.dynamic_bindings)).unwrap_or_default()
    }

    /// The scope stack the frame detached when it suspended: the `with` objects of its
    /// scope chain, those its closure captured and those it entered, outermost first. While a
    /// lane activation runs the frame, the chain is the lane stack's (`LaneStack::with_objects`).
    pub fn with_objects(&self) -> &[Value] {
        &self.cold().with_objects
    }

    /// `with_objects`, to change.
    pub fn with_objects_mut(&mut self) -> &mut Vec<Value> {
        &mut self.cold_mut().with_objects
    }

    /// `with_objects`, to drain, where the frame holds a box for them.
    pub fn held_with_objects_mut(&mut self) -> Option<&mut Vec<Value>> {
        self.cold.as_mut().map(|cold| &mut cold.with_objects)
    }

    /// The resume pc of the binding site the direct eval the activation is running was called
    /// at (`CallerView::binding_site`).
    pub(in crate::vm) fn binding_site_pc(&self) -> Option<u32> {
        self.cold().binding_site_pc
    }

    /// Makes `site` the binding site, returning the one it replaces.
    pub fn replace_binding_site_pc(&mut self, site: Option<u32>) -> Option<u32> {
        if site.is_none() && self.cold.is_none() {
            return None;
        }
        std::mem::replace(&mut self.cold_mut().binding_site_pc, site)
    }

    /// A direct eval over the record ended: its binding site and the `with` scopes it entered
    /// with go, and the box with them where it holds nothing else.
    pub fn end_eval_scope(&mut self) {
        if let Some(cold) = self.cold.as_mut() {
            cold.binding_site_pc = None;
            cold.with_objects.clear();
            if cold.is_empty() {
                self.cold = None;
            }
        }
    }
}
