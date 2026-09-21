use super::{CallTarget, JsError, MethodCache, Vm};
use crate::{Engine, Host};

struct SilentHost;
impl Host for SilentHost {
    fn write_line(&mut self, _: &str) {}
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
    vm.execute(&program).unwrap();
    assert!(vm.heap.stats().0 < 100, "unexpected per-call allocation");
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
