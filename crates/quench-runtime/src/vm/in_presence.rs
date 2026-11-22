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
pub struct InPresence {
    pub(crate) atom: Atom,
    pub shape: u32,
    pub link: u32,
    pub present: bool,
    /// The exact shape of the receiver's prototype (its own where it has none): the holder whose values a code holding
    /// the answer subscribes to (`heap::subscribe_prototype_values`).
    pub holder: u32,
    /// The prototypes from `link` to the chain's end, all the answer read: a code holding it holds their layouts
    /// (`Heap::subscribe_prototype_chain`).
    pub depth: u8,
    /// The cell of the receiver's whole chain (`Vm::chain_cell`): the walk marked every link of it a prototype, so a
    /// change to one, or its death, ends the answer, and a change to a prototype off the chain leaves it.
    cell: crate::heap::ChainCell,
}

/// The `Vm::in_presences` entry a receiver of `shape` asked about `atom` takes.
fn presence_index(shape: u32, atom: Atom) -> usize {
    (shape ^ atom) as usize % IN_PRESENCES
}

impl Vm {
    /// Remembers that `key in object` for the key `atom` was `present`, where `object` is a direct object of a
    /// layout a shape names exactly and so is each prototype on its chain (a dictionary's layout is its own).
    pub(super) fn remember_presence(&mut self, object: Value, atom: Atom, present: bool) {
        let Some(receiver) = self.heap.object_record(object) else { return };
        let (shape, link, proto) = (receiver.exact_shape(), receiver.proto_link().bits(), receiver.proto());
        if self.shape_is_dictionary(receiver.shape()) {
            return;
        }
        let index = presence_index(shape, atom);
        let current_answer = self.in_presences[index]
            .is_some_and(|entry| (entry.atom, entry.shape, entry.link, entry.present) == (atom, shape, link, present) && entry.cell.holds());
        if current_answer {
            return;
        }
        let holder = self.heap.object_record(proto).map_or(shape, Object::exact_shape);
        // Every link to the chain's end is a record a shape names exactly, so the chain's cell covers all the answer
        // read.
        let mut current = proto;
        let mut depth = 0u8;
        while !current.is_null() {
            let Some(data) = self.heap.object_record(current) else { return };
            if self.shape_is_dictionary(data.shape()) {
                return;
            }
            current = data.proto();
            let Some(deeper) = depth.checked_add(1) else { return };
            depth = deeper;
        }
        // Marking a link that was no prototype yet changes no layout: the answer holds under the cell made after it.
        let Some(cell) = self.chain_cell(proto, depth) else { return };
        self.in_presences[index] = Some(InPresence { atom, shape, link, present, holder, depth, cell });
    }

    /// What `key in object` answered for the key `atom` where exactly one receiver layout answered it and its answer
    /// still holds (no prototype has changed its layout since): the fact a code may hold under the dependency on the
    /// prototypes' values and layouts.
    pub(in crate::vm) fn presence_fact(&self, atom: Atom) -> Option<InPresence> {
        let mut answers = self.in_presences.iter().flatten().filter(|entry| entry.atom == atom);
        let only = answers.next().copied().filter(|entry| entry.cell.holds())?;
        answers.next().is_none().then_some(only)
    }
}
