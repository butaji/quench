//! The add row's and the naming row's planning inputs from the VM's recorded transitions, and the
//! interpreter's reading of them. Runtime only: it names the heap, which the stencil build does not compile.

use super::add::{Edge, add_property};
use super::naming::{name_form, set_function_name};
use super::object::Form;
use super::Concrete;
use crate::heap::{Heap, NATIVE_OBJECT_LAYOUT as OBJECT};
use crate::value_vec::range_has_room;

#[quench_macros::test_visible]
/// One resolved shape transition as an add site records it: objects of `before` take the value at
/// `slot`, their storage length, and move to `after`. The staged rows run it on records of exactly
/// the layout `layouts[0]` (`heap::layouts`: prototype link, flags, shape and storage form), which
/// the add moves to `layouts[1]` (`Object::layout_with_shape`): one id compare proves the shape,
/// the link and the integrity bits, and one id store moves the record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Transition {
    pub(crate) before: u32,
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
    /// The staged rows' edge: the layouts, and the slot.
    #[inline(always)]
    pub fn edge(self) -> Edge<u64> {
        let [before, after] = self.layouts.map(u64::from);
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

/// The interpreter's reading of `add_property`: `value` added to object record `record_id` of
/// `heap` along `transition`; false changes nothing and leaves the add to the VM.
///
/// # Safety
/// `record_id` names a live object record of `heap`, whose exact shape's storage length is the
/// transition's slot where the shape matches (the row proves the shape before any write).
#[inline(always)]
pub unsafe fn concrete_add_property(heap: &mut Heap, record_id: u32, transition: Transition, value: crate::Value) -> bool {
    let Some(form) = transition.form() else {
        return false;
    };
    // SAFETY: the caller's contract; the gate reads the heap before any write.
    let record = unsafe { heap.unlogged_object_record(record_id) } as u64;
    let mut stage = Concrete::over(heap);
    let added = add_property(&mut stage, record, u64::from(record_id), transition.edge(), form, value.to_bits(), ()).is_some();
    #[cfg(feature = "profile-memory")]
    if added {
        let value = heap.object_value(record_id);
        heap.note_object_slots(value, usize::from(transition.slot) + 1);
    }
    added
}

/// `concrete_add_property` of a value the after-shape's slot observation already holds, into the
/// inline (packed) storage of the record: the add slot observation would see nothing of
/// (`Heap::add_observation_holds`), whose narrow word needs no slot encoding.
///
/// # Safety
/// As `concrete_add_property`.
#[inline(always)]
pub unsafe fn concrete_add_property_held(heap: &mut Heap, record_id: u32, transition: Transition, value: crate::Value) -> bool {
    if usize::from(transition.slot) >= OBJECT.inline_property_count { return false; }
    // SAFETY: the caller's contract; the gate reads the heap before any write.
    let record = unsafe { heap.unlogged_object_record(record_id) } as u64;
    let mut stage = Concrete::over(heap);
    super::add::add_property_held(&mut stage, record, u64::from(record_id), transition.edge(), Form::Packed, value.to_bits(), ()).is_some()
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
    // SAFETY: the caller's contract; the gate reads the heap before any write.
    let record = unsafe { heap.unlogged_function_object(record_id) } as u64;
    let mut stage = Concrete::over(heap);
    set_function_name(&mut stage, record, u64::from(record_id), (transition.edge(), empty), name_form(transition.slot), name.to_bits(), ()).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::Object;
    use crate::Value;

    /// A heap that knows the shapes 1 to `shapes`, each of as many slots as its number, and an
    /// object of none (shape 0).
    fn heap_of(shapes: u32) -> (Heap, Value) {
        let mut heap = Heap::new();
        for shape in 1..=shapes {
            heap.register_property_shape(shape, shape as usize, None);
        }
        let object = heap.alloc_object(Object::new(Value::NULL));
        (heap, object)
    }

    /// The edge `len` to `len + 1` as a site records it from `object`, whose layout, renumbered to
    /// shape `len`, is the one the site guards.
    fn transition_of(heap: &Heap, object: Value, len: u32) -> Transition {
        let before = Object::layout_with_shape(heap.object_ref(object).expect("an object").layout_id(), len);
        Transition { before: len, after: len + 1, slot: len as u16, layouts: [before, Object::layout_with_shape(before, len + 1)] }
    }

    /// The row's add of `value` to `object` by `transition`.
    fn add_by(heap: &mut Heap, object: Value, transition: Transition, value: Value) -> bool {
        let id = heap.record_id(object.direct_object_index().expect("a direct object"));
        // SAFETY: a live record of the heap; the row proves its layout.
        unsafe { concrete_add_property(heap, id, transition, value) }
    }

    /// The row's add of `value` as the `len`th property of `object`, by the edge `len` to `len + 1`
    /// recorded from `object` itself.
    fn add(heap: &mut Heap, object: Value, len: u32, value: Value) -> bool {
        let transition = transition_of(heap, object, len);
        add_by(heap, object, transition, value)
    }

    fn shape_of(heap: &Heap, object: Value) -> u32 {
        heap.object_ref(object).expect("an object").exact_shape()
    }

    fn read(heap: &Heap, object: Value, slot: u16) -> Option<Value> {
        let shape = shape_of(heap, object);
        // SAFETY: a live record of its exact shape, whose slot it holds.
        unsafe { crate::staged::concrete_own_field_hit(heap.object_ref(object).expect("an object").record(), heap.arena_values_base(), shape, slot) }
    }

    /// The compiled site's reading (`site_add`): the object word must name a direct object, and a chained
    /// add holds only for receivers whose prototype link is the proved one.
    #[test]
    fn a_site_add_tests_the_object_word_and_the_prototype_link() {
        use super::super::add::site_add;
        let (mut heap, object) = heap_of(2);
        let records = heap.object_record_base().expect("a record table") as u64;
        let link = u64::from(heap.object_ref(object).expect("an object").proto_link().bits());
        let before = heap.object_ref(object).expect("an object").layout_id();
        let edge = Edge { before: u64::from(before), after: u64::from(Object::layout_with_shape(before, 1)), slot: 0 };
        let run = |heap: &mut Heap, word: Value, link: Option<u64>| {
            let mut stage = Concrete::over(heap);
            site_add(&mut stage, records, word.to_bits(), (edge, link), Form::Packed, Value::integer(4).to_bits(), ()).is_some()
        };
        assert!(!run(&mut heap, Value::integer(3), None), "an int word names no object");
        assert!(!run(&mut heap, object, Some(link ^ 1)), "another prototype");
        assert_eq!(shape_of(&heap, object), 0, "a leave changes nothing");
        assert!(run(&mut heap, object, Some(link)));
        assert_eq!((shape_of(&heap, object), read(&heap, object, 0)), (1, Some(Value::integer(4))));
    }

    #[test]
    fn the_form_is_a_fact_of_the_slot() {
        let form = |slot| Transition { before: 0, after: 0, slot, layouts: [0; 2] }.form();
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
        let (first, other) = (transition_of(&heap, object, 0), transition_of(&heap, object, 1));
        assert!(!add_by(&mut heap, object, other, Value::integer(1)), "the object is of another shape");
        heap.object_mut_ref(object).expect("an object").set_extensible(false);
        assert!(!add_by(&mut heap, object, first, Value::integer(1)), "a non-extensible object takes no property");
        heap.object_mut_ref(object).expect("an object").set_extensible(true);
        heap.object_mut_ref(object).expect("an object").mark_as_prototype();
        assert!(!add_by(&mut heap, object, first, Value::integer(1)), "a prototype keeps extras and shape readers");
        assert_eq!(shape_of(&heap, object), 0);
    }
}
