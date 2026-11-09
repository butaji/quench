//! Lane-body compile decisions: exit words and dependency subscriptions.

use super::LaneDependencies;
use crate::code_dependencies::{CodeReaders, DependentCode, PublishedCode, SlotDependency};
use crate::heap::{Heap, Object};
use crate::typed_ir::Type;
use crate::Value;
use std::rc::Rc;

/// An entry leave's word is its own, and it resumes at the activation's first record.
#[test]
fn an_entry_leave_round_trips_and_resumes_at_the_first_record() {
    use super::LaneExit;
    assert_eq!(LaneExit::decode(LaneExit::Entry.word()), LaneExit::Entry);
    assert_ne!(LaneExit::decode(LaneExit::Entry.word()), LaneExit::At(0));
    assert_eq!(LaneExit::Entry.resumed(), LaneExit::At(0));
    assert_eq!(LaneExit::At(3).resumed(), LaneExit::At(3));
}

/// A receiver leaving its shape keeps a lane body's code (its field reads guard the
/// shape at each use); a holder leaving its shape retires it (its layout is read unguarded).
#[test]
fn lane_dependencies_retire_at_holder_departures_only() {
    for (holder, retires) in [(false, false), (true, true)] {
        let mut heap = Heap::new();
        heap.track_slot_representations(&[(0, None), (1, None), (1, None)]);
        let (storage, values) = heap.initial_object_storage(1, &[Value::integer(1)], false);
        let object = heap.alloc_object(Object::with_property_storage(Value::NULL, storage, values));
        let dependencies = LaneDependencies {
            slots: vec![SlotDependency { shape: 1, slot: 0, ty: Type::INT32, target_shape: None }],
            departure_shapes: if holder { vec![1] } else { Vec::new() },
        };
        let code: Rc<dyn DependentCode> = Rc::new(PublishedCode::new(()));
        assert!(dependencies.subscribe(&Default::default(), &code, &mut heap, &mut CodeReaders::default()));
        heap.set_object_shape(object, 2);
        assert_eq!(code.is_published(), !retires, "holder={holder}");
    }
}
