use super::*;
use crate::Engine;
use std::{cell::RefCell, rc::Rc};

struct SilentHost;
impl Host for SilentHost {
    fn write_line(&mut self, _: &str) {}
    fn clock_millis(&mut self) -> f64 {
        0.0
    }
}

struct RecordingHost(Rc<RefCell<Vec<String>>>);

impl Host for RecordingHost {
    fn write_line(&mut self, line: &str) {
        self.0.borrow_mut().push(line.into());
    }

    fn clock_millis(&mut self) -> f64 {
        0.0
    }
}

#[test]
fn third_receiver_promotes_field_site_to_megamorphic() {
    let mut vm = Vm::new(SilentHost);
    vm.field_caches.push(EMPTY_CACHE);
    vm.megamorphic_field_indices.push(NO_MEGAMORPHIC_FIELD);
    for receiver in 1..=4 {
        vm.record_field_cache(
            0,
            FieldCache {
                receiver,
                atom: 0,
                owner: Value::number(f64::from(receiver)),
                owner_shape: receiver,
                slot: 0,
                depth: 0,
            },
        );
    }
    let table = &vm.megamorphic_fields[0];
    assert_eq!(table.len(), 4);
    assert!(table.get(1).is_some() && table.get(4).is_some());
    assert_eq!(vm.megamorphic_field_indices, [0]);
}

#[test]
fn shape_slot_index_is_derived_from_immutable_shape_keys() {
    let mut vm = Vm::new(SilentHost);
    let first = vm.intern_atom("first");
    let second = vm.intern_atom("second");
    let first_shape = vm.transition_shape(0, first);
    let shape = vm.transition_shape(first_shape, second);
    assert_eq!(vm.shape_slot(shape, first), Some(0));
    assert_eq!(vm.shape_slot(shape, second), Some(1));
    let missing = vm.intern_atom("missing");
    assert_eq!(vm.shape_slot(shape, missing), None);
}

#[test]
fn cached_field_reads_follow_in_place_writes() {
    let source = r#"
      var receiver = { value: 1 };
      function readValue(value) { return value.value; }
      print(readValue(receiver));
      print(readValue(receiver));
      receiver.value = 2;
      print(readValue(receiver));
    "#;
    for (mode, compile) in [
        ("specialized", Engine::specialize as fn(&str, &str) -> _),
        ("unspecialized", Engine::specialize_unspecialized),
    ] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(RecordingHost(output.clone()));
        let program = compile(source, "field-cache-write.js").unwrap();
        vm.execute(&program).unwrap();
        assert_eq!(output.borrow().as_slice(), ["1", "1", "2"], "{mode}");

        if vm.specialized {
            let value = vm.intern_atom("value");
            assert!(
                vm.field_caches
                    .iter()
                    .any(|entry| entry.atom == value && entry.receiver != u32::MAX),
                "specialized field read should populate its cache"
            );
        }
    }
}

#[test]
fn warmed_field_cache_tracks_prototype_changes_and_rejects_cycles() {
    let source = r#"
      var first = { value: 1 };
      var second = { value: 2 };
      var receiver = Object.create(first);
      function readValue(value) { return value.value; }
      print(readValue(receiver));
      print(readValue(receiver));
      Object.setPrototypeOf(receiver, second);
      print(readValue(receiver));
      try { Object.setPrototypeOf(receiver, receiver); print("cycle-accepted"); }
      catch (error) { print("cycle-rejected"); }
      print(Object.getPrototypeOf(receiver) === second);
    "#;
    for (mode, compile) in [
        ("specialized", Engine::specialize as fn(&str, &str) -> _),
        ("unspecialized", Engine::specialize_unspecialized),
    ] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(RecordingHost(output.clone()));
        let program = compile(source, "field-cache-prototype.js").unwrap();
        vm.execute(&program).unwrap();
        assert_eq!(
            output.borrow().as_slice(),
            ["1", "1", "2", "cycle-rejected", "true"],
            "{mode}"
        );
    }
}
