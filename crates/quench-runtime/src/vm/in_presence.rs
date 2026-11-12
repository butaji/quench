//! What `key in object` answered of a receiver layout, remembered where the steady case decides it
//! (`Vm::plain_in`): the presence of a named key along a chain of ordinary objects is a function of the receiver's
//! shape and prototype link and of the prototypes' layouts, so a code that guards the first and depends on the
//! second answers `in` with no lookup (`Heap::Held` of the boolean). The memo is the site feedback of a compile:
//! one entry per receiver shape and key, direct-mapped, derived from shapes, so shape compaction clears it.

use super::*;
use crate::heap::Object;

/// Entries of `Vm::in_presences`: a power of two, enough for the receiver shapes and keys a program's `in`s test.
pub(crate) const IN_PRESENCES: usize = 64;

/// What `key in object` answered for receivers of one exact shape and prototype link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InPresence {
    pub(crate) atom: Atom,
    pub(crate) shape: u32,
    pub(crate) link: u32,
    pub(crate) present: bool,
    /// The exact shape of the receiver's prototype (its own where it has none): the holder whose layout the answer
    /// depends on, which a code holding the answer subscribes to (`heap::subscribe_prototype_values`).
    pub(crate) holder: u32,
    /// The prototype-layout epoch the answer holds under: the walk marked every link of the chain a prototype, so any
    /// change to one, and every collection, ends it (`Vm::instanceof_inheriting_holders` holds its proofs alike).
    epoch: u64,
}

/// The `Vm::in_presences` entry a receiver of `shape` asked about `atom` takes.
fn presence_index(shape: u32, atom: Atom) -> usize {
    (shape ^ atom) as usize % IN_PRESENCES
}

impl<H: Host> Vm<H> {
    /// Remembers that `key in object` for the key `atom` was `present`, where `object` is a direct object of a
    /// layout a shape names exactly and so is each prototype on its chain (a dictionary's layout is its own).
    pub(super) fn remember_presence(&mut self, object: Value, atom: Atom, present: bool) {
        let Some(receiver) = self.heap.object_record(object) else { return };
        let (shape, link, proto) = (receiver.exact_shape(), receiver.proto_link().bits(), receiver.proto());
        if self.shape_is_dictionary(receiver.shape()) {
            return;
        }
        let holder = self.heap.object_record(proto).map_or(shape, Object::exact_shape);
        let mut current = proto;
        while let Some(data) = self.heap.object_record(current) {
            if self.shape_is_dictionary(data.shape()) {
                return;
            }
            let next = data.proto();
            self.heap.mark_prototype(current);
            current = next;
        }
        let epoch = crate::heap::prototype_layout_epoch();
        self.in_presences[presence_index(shape, atom)] = Some(InPresence { atom, shape, link, present, holder, epoch });
    }

    /// What `key in object` answered for the key `atom` where exactly one receiver layout answered it and its answer
    /// still holds (no prototype has changed its layout since): the fact a code may hold under the dependency on the
    /// prototypes' values and layouts.
    pub(in crate::vm) fn presence_fact(&self, atom: Atom) -> Option<InPresence> {
        let epoch = crate::heap::prototype_layout_epoch();
        let mut answers = self.in_presences.iter().flatten().filter(|entry| entry.atom == atom);
        let only = answers.next().copied().filter(|entry| entry.epoch == epoch)?;
        answers.next().is_none().then_some(only)
    }
}
