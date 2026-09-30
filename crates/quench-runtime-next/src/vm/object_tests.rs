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
fn optional_method_cache_observes_callable_replacement_without_shape_change() {
    let source = r#"
      var receiver = { value: 1, method: function() { return this.value; } };
      function callMethod(value) { return value?.method?.(); }
      print(callMethod(receiver));
      print(callMethod(receiver));
      receiver.method = function() { return this.value + 1; };
      print(callMethod(receiver));
    "#;
    for (mode, compile) in [
        ("specialized", Engine::specialize as fn(&str, &str) -> _),
        ("unspecialized", Engine::specialize_unspecialized),
    ] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(RecordingHost(output.clone()));
        let program = compile(source, "optional-method-cache-callable-replacement.js").unwrap();
        assert!(
            !program.method_sites.is_empty(),
            "test must exercise CallMethod"
        );
        vm.execute(&program).unwrap();
        assert_eq!(output.borrow().as_slice(), ["1", "1", "2"], "{mode}");

        if vm.specialized {
            let method = vm.intern_atom("method");
            assert!(
                vm.method_caches
                    .iter()
                    .flatten()
                    .any(|entry| entry.atom == method && entry.target.is_some()),
                "specialized method site should populate its method cache"
            );
            #[cfg(feature = "profile-aggregate")]
            assert!(
                vm.profile.method_cache_hits > 0,
                "specialized method site should take its method-cache hit path"
            );
        }
    }
}

#[test]
fn optional_method_cache_invalidates_when_inherited_callable_changes() {
    let source = r#"
      var prototype = { method: function() { return this.value; } };
      var receiver = Object.create(prototype);
      receiver.value = 1;
      function callMethod(value) { return value?.method?.(); }
      print(callMethod(receiver));
      print(callMethod(receiver));
      prototype.method = function() { return this.value + 1; };
      print(callMethod(receiver));
    "#;
    for (mode, compile) in [
        ("specialized", Engine::specialize as fn(&str, &str) -> _),
        ("unspecialized", Engine::specialize_unspecialized),
    ] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(RecordingHost(output.clone()));
        let program = compile(source, "optional-method-cache-inherited-replacement.js").unwrap();
        assert!(
            !program.method_sites.is_empty(),
            "test must exercise CallMethod"
        );
        vm.execute(&program).unwrap();
        assert_eq!(output.borrow().as_slice(), ["1", "1", "2"], "{mode}");
    }
}

#[test]
fn optional_method_cache_executes_megamorphic_receiver_shapes() {
    let source = r#"
      var method = function() { return this.value; };
      var first = { method: method, value: 1 };
      var second = { extra: 0, method: method, value: 2 };
      var third = { value: 3, method: method, extra: 0 };
      var fourth = { extra: 0, value: 4, method: method };
      function callMethod(value) { return value?.method?.(); }
      print(callMethod(first));
      print(callMethod(second));
      print(callMethod(third));
      print(callMethod(fourth));
      print(callMethod(first));
      print(callMethod(second));
      print(callMethod(third));
      print(callMethod(fourth));
      third.method = function() { return this.value + 10; };
      print(callMethod(third));
    "#;
    for (mode, compile) in [
        ("specialized", Engine::specialize as fn(&str, &str) -> _),
        ("unspecialized", Engine::specialize_unspecialized),
    ] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(RecordingHost(output.clone()));
        let program = compile(source, "optional-method-cache-megamorphic.js").unwrap();
        assert_eq!(program.method_sites.len(), 1, "test needs one method site");
        vm.execute(&program).unwrap();
        assert_eq!(
            output.borrow().as_slice(),
            ["1", "2", "3", "4", "1", "2", "3", "4", "13"],
            "{mode}"
        );

        if vm.specialized {
            assert!(
                vm.megamorphic_methods.is_empty(),
                "mutation should clear the megamorphic method cache"
            );
            let third_atom = vm.intern_atom("third");
            let third = vm.own_property(vm.realm.globals, third_atom).unwrap();
            let third_shape = vm.object_data(third).unwrap().shape();
            let method_atom = vm.intern_atom("method");
            let target = vm
                .call_target(vm.own_property(third, method_atom).unwrap())
                .unwrap();
            assert!(
                vm.method_caches[0]
                    .iter()
                    .any(|entry| entry.shape == third_shape
                        && entry.atom == method_atom
                        && entry.target == Some(target)),
                "the replacement should refill the receiver's method cache"
            );
            #[cfg(feature = "profile-aggregate")]
            assert!(
                vm.profile.method_cache_tiers[2] > 0,
                "calls should hit the megamorphic method-cache tier"
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
fn non_configurable_accessor_redefinition_uses_one_identity_rule() {
    let source = r#"
      var getter = function() { return 7; };
      var setter = function(value) {};
      var ordinary = {};
      var indexed = [];
      var ordinarySetter = {};
      var indexedSetter = [];
      Object.defineProperty(ordinary, "value", { get: getter, configurable: false });
      Object.defineProperty(indexed, "0", { get: getter, configurable: false });
      Object.defineProperty(ordinarySetter, "value", { set: setter, configurable: false });
      Object.defineProperty(indexedSetter, "0", { set: setter, configurable: false });
      Object.defineProperty(ordinary, "value", { get: getter });
      Object.defineProperty(indexed, "0", { get: getter });
      Object.defineProperty(ordinarySetter, "value", { set: setter });
      Object.defineProperty(indexedSetter, "0", { set: setter });
      print(ordinary.value);
      print(indexed[0]);
      try { Object.defineProperty(ordinary, "value", { get: function() { return 8; } }); }
      catch (error) { print("ordinary-rejected"); }
      try { Object.defineProperty(indexed, "0", { get: function() { return 8; } }); }
      catch (error) { print("indexed-rejected"); }
      try { Object.defineProperty(ordinarySetter, "value", { set: function(value) {} }); }
      catch (error) { print("ordinary-setter-rejected"); }
      try { Object.defineProperty(indexedSetter, "0", { set: function(value) {} }); }
      catch (error) { print("indexed-setter-rejected"); }
    "#;
    for (mode, compile) in [
        ("specialized", Engine::specialize as fn(&str, &str) -> _),
        ("unspecialized", Engine::specialize_unspecialized),
    ] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(RecordingHost(output.clone()));
        let program = compile(source, "non-configurable-accessor-identity.js").unwrap();
        vm.execute(&program).unwrap();
        assert_eq!(
            output.borrow().as_slice(),
            [
                "7",
                "7",
                "ordinary-rejected",
                "indexed-rejected",
                "ordinary-setter-rejected",
                "indexed-setter-rejected",
            ],
            "{mode}"
        );
    }
}

#[test]
fn indexed_and_ordinary_descriptors_fold_partial_updates_identically() {
    let source = r#"
      var ordinary = { value: 1 };
      var indexed = [1];
      Object.defineProperty(ordinary, "value", { writable: false });
      Object.defineProperty(indexed, "0", { writable: false });
      var ordinaryData = Object.getOwnPropertyDescriptor(ordinary, "value");
      var indexedData = Object.getOwnPropertyDescriptor(indexed, "0");
      print(ordinaryData.value);
      print(ordinaryData.writable);
      print(ordinaryData.enumerable);
      print(ordinaryData.configurable);
      print(indexedData.value);
      print(indexedData.writable);
      print(indexedData.enumerable);
      print(indexedData.configurable);

      var ordinaryAccessor = {};
      var indexedAccessor = [];
      Object.defineProperty(ordinaryAccessor, "value", {
        get: function() { return 2; }, enumerable: true, configurable: true
      });
      Object.defineProperty(indexedAccessor, "0", {
        get: function() { return 2; }, enumerable: true, configurable: true
      });
      Object.defineProperty(ordinaryAccessor, "value", { value: 3, writable: true });
      Object.defineProperty(indexedAccessor, "0", { value: 3, writable: true });
      var ordinaryConverted = Object.getOwnPropertyDescriptor(ordinaryAccessor, "value");
      var indexedConverted = Object.getOwnPropertyDescriptor(indexedAccessor, "0");
      print(ordinaryConverted.value);
      print(ordinaryConverted.writable);
      print(ordinaryConverted.enumerable);
      print(ordinaryConverted.configurable);
      print(indexedConverted.value);
      print(indexedConverted.writable);
      print(indexedConverted.enumerable);
      print(indexedConverted.configurable);
    "#;
    for (mode, compile) in [
        ("specialized", Engine::specialize as fn(&str, &str) -> _),
        ("unspecialized", Engine::specialize_unspecialized),
    ] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(RecordingHost(output.clone()));
        let program = compile(source, "indexed-descriptor-folding.js").unwrap();
        vm.execute(&program).unwrap();
        assert_eq!(
            output.borrow().as_slice(),
            [
                "1", "false", "true", "true", "1", "false", "true", "true", "3", "true", "true",
                "true", "3", "true", "true", "true",
            ],
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
fn field_cache_fallback_preserves_proxy_get_trap_reentry() {
    let source = r#"
      var reads = 0;
      var target = { value: 9 };
      var proxy = new Proxy(target, {
        get: function(target, key, receiver) {
          if (key === "value") { reads += 1; return reads; }
          return Reflect.get(target, key, receiver);
        }
      });
      function readValue(value) { return value.value; }
      print(readValue(proxy));
      print(readValue(proxy));
      print(reads);
    "#;
    for (mode, compile) in [
        ("specialized", Engine::specialize as fn(&str, &str) -> _),
        ("unspecialized", Engine::specialize_unspecialized),
    ] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(RecordingHost(output.clone()));
        let program = compile(source, "field-cache-proxy-get.js").unwrap();
        vm.execute(&program).unwrap();
        assert_eq!(output.borrow().as_slice(), ["1", "2", "2"], "{mode}");

        if vm.specialized {
            let value = vm.intern_atom("value");
            assert!(
                vm.field_caches
                    .iter()
                    .all(|entry| entry.atom != value || entry.receiver == u32::MAX),
                "Proxy reads must retain the generic trap path"
            );
        }
    }
}

#[test]
fn method_cache_fallback_preserves_proxy_get_trap_and_receiver() {
    let source = r#"
      var reads = 0;
      var target = { method: function() { return this === proxy; } };
      var proxy = new Proxy(target, {
        get: function(target, key, receiver) {
          if (key === "method") reads += 1;
          return Reflect.get(target, key, receiver);
        }
      });
      function callMethod(value) { return value?.method?.(); }
      print(callMethod(proxy));
      print(reads);
      print(callMethod(proxy));
      print(reads);
    "#;
    for (mode, compile) in [
        ("specialized", Engine::specialize as fn(&str, &str) -> _),
        ("unspecialized", Engine::specialize_unspecialized),
    ] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(RecordingHost(output.clone()));
        let program = compile(source, "method-cache-proxy-get.js").unwrap();
        assert!(
            !program.method_sites.is_empty(),
            "test must exercise CallMethod"
        );
        vm.execute(&program).unwrap();
        assert_eq!(
            output.borrow().as_slice(),
            ["true", "1", "true", "2"],
            "{mode}"
        );

        if vm.specialized {
            let method = vm.intern_atom("method");
            assert!(
                vm.method_caches
                    .iter()
                    .flatten()
                    .all(|entry| entry.atom != method),
                "Proxy method reads must retain the generic trap path"
            );
            assert!(
                vm.megamorphic_methods
                    .iter()
                    .all(|cache| cache.entries[..usize::from(cache.len)]
                        .iter()
                        .all(|entry| entry.atom != method)),
                "Proxy method reads must not enter a megamorphic cache"
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
      function callMethod(value) { return value?.method?.(); }
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
        assert!(
            !program.method_sites.is_empty(),
            "test must exercise CallMethod"
        );
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
            assert!(
                vm.method_caches
                    .iter()
                    .flatten()
                    .all(|entry| entry.atom != method),
                "optional accessor lookup must not enter the method cache"
            );
        }
    }
}
