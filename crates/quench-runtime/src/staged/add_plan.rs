//! The add row's planning inputs from the VM's recorded transitions, and the interpreter's
//! reading of the row. Runtime only: it names the heap, which the stencil build does not compile.

use super::add::{Edge, add_property};
use super::object::Form;
use super::Concrete;
use crate::heap::{Heap, NATIVE_OBJECT_LAYOUT as OBJECT};
use crate::value_vec::range_has_room;

/// One resolved shape transition as an add site records it: objects of `before` take the value at
/// `slot`, their storage length, and move to `after`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Transition {
    pub(crate) before: u32,
    pub(crate) after: u32,
    pub(crate) slot: u16,
}

impl Transition {
    /// The storage form that can take the add without growing, a fact of the slot alone: a packed
    /// object has a free half below the inline count, and an arena range of `slot` values has
    /// room for one more where its capacity class says (`range_has_room`). `None` where neither
    /// form fits, so the VM's add, which allocates, is the only one.
    pub(crate) fn form(self) -> Option<Form> {
        let slot = usize::from(self.slot);
        match (slot < OBJECT.inline_property_count, range_has_room(slot)) {
            (true, true) => Some(Form::Either),
            (true, false) => Some(Form::Packed),
            (false, true) => Some(Form::Arena),
            (false, false) => None,
        }
    }
}

/// The interpreter's reading of `add_property`: `value` added to object record `record_id` of
/// `heap` along `transition`; false changes nothing and leaves the add to the VM.
///
/// # Safety
/// `record_id` names a live object record of `heap`, whose exact shape's storage length is the
/// transition's slot where the shape matches (the row proves the shape before any write).
#[inline(always)]
pub(crate) unsafe fn concrete_add_property(heap: &mut Heap, record_id: u32, transition: Transition, value: crate::Value) -> bool {
    let Some(form) = transition.form() else {
        return false;
    };
    // SAFETY: the caller's contract; the gate reads the heap before any write.
    let record = unsafe { heap.unlogged_object_record(record_id) } as u64;
    let edge = Edge { before: u64::from(transition.before), after: u64::from(transition.after), slot: u64::from(transition.slot) };
    let mut stage = Concrete::storing(heap);
    let added = add_property(&mut stage, record, u64::from(record_id), edge, form, value.to_bits(), ()).is_some();
    #[cfg(feature = "profile-memory")]
    if added {
        heap.note_object_slots(crate::Value::direct_object(record_id), usize::from(transition.slot) + 1);
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::{Cell, Object};
    use crate::Value;

    /// A heap that knows the shapes 1 to `shapes`, each of as many slots as its number, and an
    /// object of none (shape 0).
    fn heap_of(shapes: u32) -> (Heap, Value) {
        let mut heap = Heap::new();
        for shape in 1..=shapes {
            heap.register_property_shape(shape, shape as usize, None);
        }
        let object = heap.alloc(Cell::Object(Object::new(Value::NULL)));
        (heap, object)
    }

    /// The row's add of `value` as the `len`th property of `object`, by the edge `len` to `len + 1`.
    fn add(heap: &mut Heap, object: Value, len: u32, value: Value) -> bool {
        let id = object.direct_object_index().expect("a direct object");
        let transition = Transition { before: len, after: len + 1, slot: len as u16 };
        // SAFETY: a live record of the heap; the row proves its shape.
        unsafe { concrete_add_property(heap, id, transition, value) }
    }

    fn shape_of(heap: &Heap, object: Value) -> u32 {
        heap.object(object).expect("an object").exact_shape()
    }

    fn read(heap: &Heap, object: Value, slot: u16) -> Option<Value> {
        let shape = shape_of(heap, object);
        // SAFETY: a live record of its exact shape, whose slot it holds.
        unsafe { crate::staged::concrete_own_field_hit(heap.object(object).expect("an object"), heap.arena_values_base(), shape, slot) }
    }

    #[test]
    fn the_form_is_a_fact_of_the_slot() {
        let form = |slot| Transition { before: 0, after: 0, slot }.form();
        assert_eq!((form(0), form(1), form(2), form(3)), (Some(Form::Packed), Some(Form::Either), Some(Form::Arena), Some(Form::Arena)));
        assert_eq!((form(4), form(5), form(6), form(7)), (None, Some(Form::Arena), None, Some(Form::Arena)));
    }

    #[test]
    fn a_packed_add_takes_narrow_values_and_moves_to_the_after_shape() {
        let (mut heap, object) = heap_of(3);
        assert!(add(&mut heap, object, 0, Value::integer(7)));
        assert!(add(&mut heap, object, 1, Value::NULL));
        assert_eq!(shape_of(&heap, object), 2);
        assert_eq!((read(&heap, object, 0), read(&heap, object, 1)), (Some(Value::integer(7)), Some(Value::NULL)));
        assert!(!add(&mut heap, object, 2, Value::integer(1)), "the inline halves are full and the storage is no arena range");
        assert_eq!(shape_of(&heap, object), 2, "a leave changes nothing");
    }

    #[test]
    fn a_value_with_no_narrow_form_leaves_a_packed_add() {
        let (mut heap, object) = heap_of(1);
        assert!(!add(&mut heap, object, 0, Value::number(1.5)));
        assert_eq!(shape_of(&heap, object), 0);
    }

    #[test]
    fn an_arena_add_takes_any_value_while_its_range_has_room() {
        let (mut heap, object) = heap_of(5);
        for shape in 1..=3 {
            heap.property_add(object, Value::integer(shape as i32), shape);
        }
        assert!(add(&mut heap, object, 3, Value::number(1.5)));
        assert_eq!((shape_of(&heap, object), read(&heap, object, 3)), (4, Some(Value::number(1.5))));
        assert_eq!(read(&heap, object, 2), Some(Value::integer(3)), "the values before it stay");
        assert!(!add(&mut heap, object, 4, Value::integer(1)), "the range of four is full: the VM grows it");
        assert_eq!(shape_of(&heap, object), 4);
    }

    #[test]
    fn an_add_leaves_for_another_shape_integrity_or_extras() {
        let (mut heap, object) = heap_of(2);
        assert!(!add(&mut heap, object, 1, Value::integer(1)), "the object is of another shape");
        heap.object_mut(object).expect("an object").set_extensible(false);
        assert!(!add(&mut heap, object, 0, Value::integer(1)), "a non-extensible object takes no property");
        heap.object_mut(object).expect("an object").set_extensible(true);
        heap.object_mut(object).expect("an object").mark_as_prototype();
        assert!(!add(&mut heap, object, 0, Value::integer(1)), "a prototype keeps extras and shape readers");
        assert_eq!(shape_of(&heap, object), 0);
    }
}
