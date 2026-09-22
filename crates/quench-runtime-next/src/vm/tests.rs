use super::{CallTarget, JsError, MethodCache, Vm};
use crate::{Engine, Host, Value};
use std::cell::RefCell;
use std::rc::Rc;

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
fn js_error_is_pointer_sized() {
    assert_eq!(size_of::<JsError>(), size_of::<usize>());
}

#[test]
fn dynamic_primitive_strings_are_canonicalized() {
    let mut vm = Vm::new(SilentHost);
    let first = vm.intern_dynamic_string("same text".into());
    let second = vm.intern_dynamic_string("same text".into());
    assert_eq!(first, second);
}

#[test]
fn repeated_string_concatenations_use_the_bounded_cache() {
    let mut vm = Vm::new(SilentHost);
    let left = vm.intern_dynamic_string("left".into());
    let right = vm.intern_dynamic_string("right".into());
    let first = vm.intern_dynamic_concat(left, right).unwrap();
    let second = vm.intern_dynamic_concat(left, right).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        vm.string_concats.as_ref().unwrap().len(),
        super::STRING_CONCAT_CACHE_SIZE
    );
}

#[test]
fn accessor_descriptors_share_get_and_set_property_semantics() {
    let source = r#"
      var object = {};
      Object.defineProperty(object, "value", {
        get: function() { return 3; },
        set: function(next) { print(next); }
      });
      print(object.value);
      object.value = 9;
      var descriptor = Object.getOwnPropertyDescriptor(object, "value");
      print(typeof descriptor.get);
      print(typeof descriptor.set);
      var prototype = {};
      var receiver = Object.create(prototype);
      Object.defineProperty(prototype, "inherited", {
        get: function() { return this === receiver ? 7 : 0; },
        set: function(next) { print(this === receiver); }
      });
      print(receiver.inherited);
      receiver.inherited = 11;
      var lockedPrototype = {};
      Object.defineProperty(lockedPrototype, "locked", { value: 1, writable: false });
      var lockedReceiver = Object.create(lockedPrototype);
      try { lockedReceiver.locked = 2; print("not-blocked"); }
      catch (error) { print("blocked"); }
    "#;
    let program = Engine::specialize(source, "accessor.js").unwrap();
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(RecordingHost(output.clone()));
    vm.execute(&program).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        ["3", "9", "function", "function", "7", "true", "blocked"]
    );
}

#[test]
fn untaken_closure_branch_does_not_allocate_environments() {
    let source = r#"
      function maybe(make) {
        var value = 1;
        if (make) return function() { return value; };
        return value;
      }
      var i = 0;
      while (i < 1000) { maybe(false); i = i + 1; }
    "#;
    let program = Engine::specialize(source, "lazy-env.js").unwrap();
    let mut vm = Vm::new(SilentHost);
    vm.initialize(&program).unwrap();
    let baseline = vm.heap.stats().0;
    let root = vm.closure(&program, 0, Value::NULL).unwrap();
    let globals = vm.globals;
    vm.call_value(&program, root, globals, &[]).unwrap();
    let execution_allocations = vm.heap.stats().0 - baseline;
    assert!(
        execution_allocations < 128,
        "unexpected per-call allocation: {}",
        execution_allocations
    );
}

#[test]
fn third_method_receiver_promotes_site_to_megamorphic() {
    let mut vm = Vm::new(SilentHost);
    vm.method_caches.push([super::EMPTY_METHOD_CACHE; 2]);
    for shape in 1..=4 {
        vm.record_method_cache(
            0,
            MethodCache {
                shape,
                proto: crate::Value::NULL,
                target: Some(CallTarget::User(shape, crate::Value::NULL)),
            },
        );
    }
    assert_eq!(vm.megamorphic_methods[0].len, 4);
}

#[test]
fn method_cache_gc_retains_live_and_rejects_reused_handles() {
    let mut vm = Vm::new(SilentHost);
    let live = vm.heap.alloc(crate::heap::Cell::Environment {
        parent: crate::Value::NULL,
        slots: Box::new([]),
    });
    let dead = vm.heap.alloc(crate::heap::Cell::Environment {
        parent: crate::Value::NULL,
        slots: Box::new([]),
    });
    vm.method_caches.push([
        MethodCache {
            shape: 1,
            proto: crate::Value::NULL,
            target: Some(CallTarget::User(1, live)),
        },
        MethodCache {
            shape: 2,
            proto: crate::Value::NULL,
            target: Some(CallTarget::User(2, dead)),
        },
    ]);
    vm.heap.collect([live]);
    vm.retain_live_method_caches();
    assert!(matches!(
        vm.method_caches[0][0].target,
        Some(CallTarget::User(1, env)) if env == live
    ));
    assert!(vm.method_caches[0][1].target.is_none());

    let reused = vm.heap.alloc(crate::heap::Cell::Environment {
        parent: crate::Value::NULL,
        slots: Box::new([]),
    });
    assert_eq!(reused, dead);
    assert!(vm.method_caches[0][1].target.is_none());
}
