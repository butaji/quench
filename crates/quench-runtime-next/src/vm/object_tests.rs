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

#[test]
fn optional_method_call_field_cache_observes_callable_replacement() {
    let source = r#"
      var receiver = { method: function() { return 1; } };
      function callMethod(value) { return value.method?.(); }
      print(callMethod(receiver));
      receiver.method = function() { return 2; };
      print(callMethod(receiver));
    "#;
    for (mode, compile) in [
        ("specialized", Engine::specialize as fn(&str, &str) -> _),
        ("unspecialized", Engine::specialize_unspecialized),
    ] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(RecordingHost(output.clone()));
        let program = compile(source, "optional-method-cache-write.js").unwrap();
        vm.execute(&program).unwrap();
        assert_eq!(output.borrow().as_slice(), ["1", "2"], "{mode}");

        if vm.specialized {
            let method = vm.intern_atom("method");
            assert!(
                vm.field_caches
                    .iter()
                    .any(|entry| entry.atom == method && entry.receiver != u32::MAX),
                "optional method lookup should populate its field cache"
            );
        }
    }
}

#[test]
fn redefining_a_deleted_sparse_array_index_uses_new_property_defaults() {
    let source = r#"
      var values = [];
      values[1000] = 1;
      delete values[1000];
      Object.defineProperty(values, "1000", { value: 2 });
      var descriptor = Object.getOwnPropertyDescriptor(values, "1000");
      print(descriptor.value);
      print(descriptor.writable);
      print(descriptor.enumerable);
      print(descriptor.configurable);
      var accessors = [];
      accessors[1000] = 1;
      delete accessors[1000];
      Object.defineProperty(accessors, "1000", { get: function() { return 3; } });
      var accessorDescriptor = Object.getOwnPropertyDescriptor(accessors, "1000");
      print(typeof accessorDescriptor.get);
      print(accessorDescriptor.enumerable);
      print(accessorDescriptor.configurable);
    "#;
    for (mode, compile) in [
        ("specialized", Engine::specialize as fn(&str, &str) -> _),
        ("unspecialized", Engine::specialize_unspecialized),
    ] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(RecordingHost(output.clone()));
        let program = compile(source, "deleted-sparse-array-descriptor.js").unwrap();
        vm.execute(&program).unwrap();
        assert_eq!(
            output.borrow().as_slice(),
            ["2", "false", "false", "false", "function", "false", "false"],
            "{mode}"
        );
    }
}

#[test]
fn field_cache_fallback_preserves_accessor_reentry() {
    let source = r#"
      var count = 0;
      var receiver = {
        get value() { count += 1; return count; }
      };
      function readValue(value) { return value.value; }
      print(readValue(receiver));
      print(readValue(receiver));
      print(count);
    "#;
    for (mode, compile) in [
        ("specialized", Engine::specialize as fn(&str, &str) -> _),
        ("unspecialized", Engine::specialize_unspecialized),
    ] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(RecordingHost(output.clone()));
        let program = compile(source, "field-cache-accessor-reentry.js").unwrap();
        vm.execute(&program).unwrap();
        assert_eq!(output.borrow().as_slice(), ["1", "2", "2"], "{mode}");

        if vm.specialized {
            let value = vm.intern_atom("value");
            assert!(
                vm.field_caches
                    .iter()
                    .all(|entry| entry.atom != value || entry.receiver == u32::MAX),
                "accessor lookup must use the generic re-entrant path"
            );
        }
    }
}

#[test]
fn optional_method_lookup_reenters_accessor_once_and_preserves_receiver() {
    let source = r#"
      var reads = 0;
      var receiver = {
        get method() {
          reads += 1;
          return function() { return this === receiver; };
        }
      };
      function callMethod(value) { return value.method?.(); }
      print(callMethod(receiver));
      print(reads);
      print(callMethod(receiver));
      print(reads);
    "#;
    for (mode, compile) in [
        ("specialized", Engine::specialize as fn(&str, &str) -> _),
        ("unspecialized", Engine::specialize_unspecialized),
    ] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(RecordingHost(output.clone()));
        let program = compile(source, "optional-method-accessor.js").unwrap();
        vm.execute(&program).unwrap();
        assert_eq!(
            output.borrow().as_slice(),
            ["true", "1", "true", "2"],
            "{mode}"
        );

        if vm.specialized {
            let method = vm.intern_atom("method");
            assert!(
                vm.field_caches
                    .iter()
                    .all(|entry| entry.atom != method || entry.receiver == u32::MAX),
                "optional accessor lookup must retain the generic getter path"
            );
        }
    }
}
