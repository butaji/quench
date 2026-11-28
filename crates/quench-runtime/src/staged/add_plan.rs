//! The add row's and the naming row's planning inputs from the VM's recorded transitions, and the
//! interpreter's reading of them. Runtime only: it names the heap, which the stencil build does not compile.

use super::add::Edge;
use super::naming::{name_form, set_function_name};
use super::object::Form;
use super::Concrete;
use crate::heap::{Heap, NATIVE_OBJECT_LAYOUT as OBJECT, layouts};
use super::field_cache_layout::{CLASS_KEY, is_class_key};
use crate::value_vec::range_has_room;

#[quench_macros::test_visible]
/// One resolved shape transition as an add site records it: objects of `before` take the value at
/// `slot`, their storage length, and move to `after`. The staged rows run it on records of exactly
/// the layout `layouts[0]` (`heap::layouts`: prototype link, flags, shape and storage form), which
/// the add moves to `layouts[1]` (`Object::layout_with_shape`): one id compare proves the shape,
/// the link and the integrity bits, and one id store moves the record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Transition {
    pub before: u32,
    pub(crate) after: u32,
    pub slot: u16,
    pub layouts: [u32; 2],
}

/// An add site as compiled code holds it: the transition the site recorded and, for an add the
/// prototype chain could intercept (a `SetField`, which finds setters and read-only inherited names),
/// the prototype link its chain proof holds for and the `depth` prototypes from it the proof read.
/// The proof is a Stable fact: the code needs only the receiver's link, and retires when a prototype
/// of that chain changes its layout (`Heap::subscribe_prototype_chain`). A define consults no chain,
/// so it holds no link.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AddSite {
    pub transition: Transition,
    pub link: Option<u32>,
    pub depth: u8,
}

impl Transition {
    /// Whether the transition is keyed by a live cell's class (`before` is `class | CLASS_KEY`,
    /// `Heap::live_class_key`): its add runs in the cell in place, not on a record.
    #[inline(always)]
    pub fn is_class_keyed(self) -> bool {
        is_class_key(self.before)
    }

    /// The class id a class-keyed transition's cell holds before the add (`before` without its key bit).
    #[inline(always)]
    pub(crate) fn class_before(self) -> u32 {
        self.before & !CLASS_KEY
    }

    /// The transition as a record's add takes it: a class-keyed `before` becomes its class's shape,
    /// so `set_class` and the record path see shapes, and the record layouts (`layouts`) are unchanged.
    #[inline(always)]
    pub(crate) fn record_form(self) -> Self {
        if self.is_class_keyed() { Self { before: layouts::class_shape(self.class_before()), ..self } } else { self }
    }

    /// The staged rows' edge: the layouts, and the slot. A class-keyed transition's `before` is its
    /// key (`class | CLASS_KEY`), which the class add (`add::class_add`) compares with the cell's class; a record
    /// transition's is its before-layout.
    #[inline(always)]
    pub fn edge(self) -> Edge<u64> {
        let [layout_before, after] = self.layouts.map(u64::from);
        let before = if self.is_class_keyed() { u64::from(self.before) } else { layout_before };
        Edge { before, after, slot: u64::from(self.slot) }
    }

    /// The storage form that can take the add without growing, a fact of the slot alone: a packed
    /// object has a free half below the inline count, and an arena range of `slot` values has
    /// room for one more where its capacity class says (`range_has_room`). `None` where neither
    /// form fits, so the VM's add, which allocates, is the only one.
    #[inline(always)]
    pub fn form(self) -> Option<Form> {
        let slot = usize::from(self.slot);
        match (slot < OBJECT.inline_property_count, range_has_room(slot)) {
            (true, true) => Some(Form::Either),
            (true, false) => Some(Form::Packed),
            (false, true) => Some(Form::Arena),
            (false, false) => None,
        }
    }
}

/// The interpreter's reading of `naming::set_function_name`: the function record `record_id` of
/// `heap` named `name` (the word of the name's string) along `transition`, whose slot is its `name`'s
/// and whose before and after shapes the naming made; `empty` is the word of the empty string the
/// function holds as its name. False changes nothing and leaves the naming to the VM.
///
/// # Safety
/// `record_id` names a live function record of `heap`, whose exact shape the row proves before any write.
#[inline(always)]
pub unsafe fn concrete_set_function_name(heap: &mut Heap, record_id: u32, transition: Transition, empty: u64, name: crate::Value) -> bool {
    let transition = transition.record_form();
    // SAFETY: the caller's contract; the gate reads the heap before any write.
    let record = unsafe { heap.unlogged_function_object(record_id) } as u64;
    let mut stage = Concrete::over(heap);
    set_function_name(&mut stage, record, u64::from(record_id), (transition.edge(), empty), name_form(transition.slot), name.to_bits(), ()).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_form_is_a_fact_of_the_slot() {
        let form = |slot| Transition { before: 0, after: 0, slot, layouts: [0; 2] }.form();
        assert_eq!((form(0), form(1), form(2), form(3)), (Some(Form::Packed), Some(Form::Either), Some(Form::Arena), Some(Form::Arena)));
        assert_eq!((form(4), form(5), form(6), form(7)), (None, Some(Form::Arena), None, Some(Form::Arena)));
    }
}
