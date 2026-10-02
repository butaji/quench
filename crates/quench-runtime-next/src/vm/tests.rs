use super::wtf16::JsString;
use super::{
    CallTarget, IteratorRealmPrototypes, JsError, MethodCache, Native, TypedArrayKind, Vm,
    activation::Completion, activation::Continuation, regexp::RegExpIntrinsics,
};
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
fn realm_lexical_state_follows_the_active_global_and_stays_rooted() {
    let mut vm = Vm::new(SilentHost);
    let first_global = vm.object();
    let second_global = vm.object();
    vm.realm.globals = first_global;
    let first_binding = vm.intern_atom("firstRealmBinding");
    vm.realm.global_lexical_declarations.insert(first_binding);
    vm.realm
        .global_lexical_bindings
        .insert(first_binding, Value::number(1.0));

    assert_eq!(vm.switch_realm_global(second_global), first_global);
    assert!(
        !vm.realm
            .global_lexical_declarations
            .contains(&first_binding)
    );
    let second_binding = vm.intern_atom("secondRealmBinding");
    vm.realm.global_lexical_declarations.insert(second_binding);
    vm.realm
        .global_lexical_bindings
        .insert(second_binding, Value::number(2.0));

    let program = Engine::specialize("print(0);", "realm-switch.js").unwrap();
    vm.collect_now(&program);
    assert!(vm.heap.get(first_global).is_some());
    assert!(vm.heap.get(second_global).is_some());

    assert_eq!(vm.switch_realm_global(first_global), second_global);
    assert!(
        vm.realm
            .global_lexical_declarations
            .contains(&first_binding)
    );
    assert_eq!(
        vm.realm.global_lexical_bindings.get(&first_binding),
        Some(&Value::number(1.0))
    );
    assert!(
        !vm.realm
            .global_lexical_declarations
            .contains(&second_binding)
    );

    vm.switch_realm_global(second_global);
    assert!(
        vm.realm
            .global_lexical_declarations
            .contains(&second_binding)
    );
    assert_eq!(
        vm.realm.global_lexical_bindings.get(&second_binding),
        Some(&Value::number(2.0))
    );
}

#[test]
fn realm_intrinsic_registries_keep_each_realm_rooted() {
    let mut vm = Vm::new(SilentHost);
    let first_global = vm.object();
    let second_global = vm.object();
    let first_error_prototype = vm.object();
    let second_error_prototype = vm.object();
    let first_iterator_prototype = vm.object();
    let second_iterator_prototype = vm.object();
    let first_regexp_constructor = vm.object();
    let first_regexp_prototype = vm.object();
    let second_regexp_constructor = vm.object();
    let second_regexp_prototype = vm.object();
    let first_segmenter_prototype = vm.object();
    let second_segmenter_prototype = vm.object();
    vm.realm.intrinsics.builtin_prototypes.insert(
        (first_global, Native::TypeError),
        first_error_prototype,
    );
    vm.realm.intrinsics.builtin_prototypes.insert(
        (second_global, Native::TypeError),
        second_error_prototype,
    );
    vm.realm.intrinsics.iterator_prototypes.insert(
        first_global,
        IteratorRealmPrototypes {
            helper: first_iterator_prototype,
            wrapper: first_iterator_prototype,
            generator: first_iterator_prototype,
            async_generator: first_iterator_prototype,
        },
    );
    vm.realm.intrinsics.regexp_intrinsics.insert(
        first_global,
        RegExpIntrinsics {
            constructor: first_regexp_constructor,
            prototype: first_regexp_prototype,
        },
    );
    vm.realm.intrinsics.regexp_intrinsics.insert(
        second_global,
        RegExpIntrinsics {
            constructor: second_regexp_constructor,
            prototype: second_regexp_prototype,
        },
    );
    vm.realm
        .intrinsics
        .intl_segmenter_prototypes
        .insert(first_global, first_segmenter_prototype);
    vm.realm
        .intrinsics
        .intl_segmenter_prototypes
        .insert(second_global, second_segmenter_prototype);
    vm.realm.intrinsics.iterator_prototypes.insert(
        second_global,
        IteratorRealmPrototypes {
            helper: second_iterator_prototype,
            wrapper: second_iterator_prototype,
            generator: second_iterator_prototype,
            async_generator: second_iterator_prototype,
        },
    );

    let program = Engine::specialize("print(0);", "realm-intrinsics.js").unwrap();
    vm.collect_now(&program);

    for object in [
        first_global,
        second_global,
        first_error_prototype,
        second_error_prototype,
        first_iterator_prototype,
        second_iterator_prototype,
        first_regexp_constructor,
        first_regexp_prototype,
        second_regexp_constructor,
        second_regexp_prototype,
        first_segmenter_prototype,
        second_segmenter_prototype,
    ] {
        assert!(vm.heap.get(object).is_some());
    }
    assert_eq!(
        vm.realm.intrinsics.builtin_prototypes.get(&(first_global, Native::TypeError)),
        Some(&first_error_prototype)
    );
    assert_eq!(
        vm.realm.intrinsics.builtin_prototypes.get(&(second_global, Native::TypeError)),
        Some(&second_error_prototype)
    );
    assert_eq!(
        vm.realm.intrinsics.iterator_prototypes
            .get(&first_global)
            .map(|prototypes| prototypes.generator),
        Some(first_iterator_prototype)
    );
    assert_eq!(
        vm.realm.intrinsics.iterator_prototypes
            .get(&second_global)
            .map(|prototypes| prototypes.generator),
        Some(second_iterator_prototype)
    );
    assert_eq!(
        vm.realm
            .intrinsics
            .regexp_intrinsics
            .get(&first_global)
            .map(|intrinsics| (intrinsics.constructor, intrinsics.prototype)),
        Some((first_regexp_constructor, first_regexp_prototype))
    );
    assert_eq!(
        vm.realm
            .intrinsics
            .regexp_intrinsics
            .get(&second_global)
            .map(|intrinsics| (intrinsics.constructor, intrinsics.prototype)),
        Some((second_regexp_constructor, second_regexp_prototype))
    );
    assert_eq!(
        vm.realm
            .intrinsics
            .intl_segmenter_prototypes
            .get(&first_global),
        Some(&first_segmenter_prototype)
    );
    assert_eq!(
        vm.realm
            .intrinsics
            .intl_segmenter_prototypes
            .get(&second_global),
        Some(&second_segmenter_prototype)
    );
}

#[test]
fn realm_promise_records_keep_values_rooted() {
    let mut vm = Vm::new(SilentHost);
    let promise = vm.promise_object();
    let result = vm.object();
    vm.realm.promise.records.get_mut(&promise).unwrap().result = result;

    let program = Engine::specialize("print(0);", "realm-promises.js").unwrap();
    vm.collect_now(&program);

    assert!(vm.heap.get(promise).is_some());
    assert!(vm.heap.get(result).is_some());
    assert_eq!(
        vm.realm.promise.records.get(&promise).map(|record| record.result),
        Some(result)
    );
}

#[test]
fn js_error_is_pointer_sized() {
    assert_eq!(size_of::<JsError>(), size_of::<usize>());
}

#[test]
fn dynamic_primitive_strings_are_canonicalized() {
    let mut vm = Vm::new(SilentHost);
    let first = vm.intern_dynamic_value("same text".into());
    let second = vm.intern_dynamic_value("same text".into());
    assert_eq!(first, second);
}

#[test]
fn repeated_string_concatenations_use_the_bounded_cache() {
    let mut vm = Vm::new(SilentHost);
    let left = vm.intern_dynamic_value("left".into());
    let right = vm.intern_dynamic_value("right".into());
    let first = vm.intern_dynamic_concat(left, right).unwrap();
    let second = vm.intern_dynamic_concat(left, right).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        vm.string_concats.as_ref().unwrap().len(),
        super::STRING_CONCAT_CACHE_SIZE
    );
}

#[test]
fn dynamic_atoms_distinguish_lone_surrogate_units() {
    let mut vm = Vm::new(SilentHost);
    let first = vm.intern_js_atom(&JsString::from_units(&[0xD800]));
    let second = vm.intern_js_atom(&JsString::from_units(&[0xD800]));
    let other = vm.intern_js_atom(&JsString::from_units(&[0xD801]));
    assert_eq!(first, second);
    assert_ne!(first, other);
    assert_eq!(vm.atom_value(first).units(), &[0xD800]);
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
      print(lockedReceiver.locked);
      print(Object.hasOwn(lockedReceiver, "locked"));
      class Accessor {
        constructor(value) { this._value = value; }
        get value() { return this._value; }
        set value(next) { this._value = next; }
        static get kind() { return "class"; }
      }
      var instance = new Accessor(4);
      print(instance.value);
      instance.value = 6;
      print(instance.value);
      print(Accessor.kind);
    "#;
    for strict in [false, true] {
        let source = if strict {
            format!("'use strict';\n{source}")
        } else {
            source.to_owned()
        };
        for compile in [Engine::specialize, Engine::specialize_unspecialized] {
            let program = compile(&source, "accessor.js").unwrap();
            let output = Rc::new(RefCell::new(Vec::new()));
            let mut vm = Vm::new(RecordingHost(output.clone()));
            vm.execute(&program).unwrap();
            assert_eq!(
                output.borrow().as_slice(),
                [
                    "3", "9", "function", "function", "7", "true",
                    if strict { "blocked" } else { "not-blocked" },
                    "1", "false", "4", "6", "class"
                ],
                "strict={strict}"
            );
        }
    }
}

#[test]
fn dictionary_shapes_fall_back_after_deletion_and_prototype_use() {
    let source = r#"
      var prototype = {
        answer: 42,
        getAnswer: function() { return this.answer; }
      };
      var inherited = {};
      Object.setPrototypeOf(inherited, prototype);
      print(inherited.answer);
      print(inherited.getAnswer());
      var deleted = { answer: 5 };
      delete deleted.answer;
      deleted.answer = 7;
      print(deleted.answer);
    "#;
    for (mode, compile) in [
        ("specialized", Engine::specialize as fn(&str, &str) -> _),
        ("unspecialized", Engine::specialize_unspecialized),
    ] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(RecordingHost(output.clone()));
        let program = compile(source, "dictionary-shapes.js").unwrap();
        vm.execute(&program).unwrap();

        let global = vm.realm.globals;
        for name in ["inherited", "deleted"] {
            let atom = vm.intern_atom(name);
            let object = vm
                .own_property(global, atom)
                .expect("global binding exists");
            let shape = vm.object_data(object).expect("object value").shape();
            assert!(
                vm.field_caches.iter().all(|cache| cache.receiver != shape),
                "{mode}: dictionary shape entered a monomorphic field cache for {name}"
            );
            assert!(
                vm.megamorphic_fields
                    .iter()
                    .all(|cache| cache.get(shape).is_none()),
                "{mode}: dictionary shape entered a megamorphic field cache for {name}"
            );
            assert!(
                vm.method_caches
                    .iter()
                    .flatten()
                    .all(|cache| cache.shape != shape),
                "{mode}: dictionary shape entered a monomorphic method cache for {name}"
            );
            assert!(
                vm.megamorphic_methods.iter().all(|cache| {
                    cache.entries[..usize::from(cache.len)]
                        .iter()
                        .all(|entry| entry.shape != shape)
                }),
                "{mode}: dictionary shape entered a megamorphic method cache for {name}"
            );
        }

        for (name, trigger) in [
            ("inherited", super::DictionaryTrigger::PrototypeUse),
            ("deleted", super::DictionaryTrigger::DeletionPattern),
        ] {
            let atom = vm.intern_atom(name);
            let object = vm
                .own_property(global, atom)
                .expect("global binding exists");
            let shape = vm.object_data(object).expect("object value").shape();
            assert!(vm.shape_is_dictionary(shape), "{mode}: {name}");
            assert_eq!(
                vm.shapes[shape as usize].dictionary_trigger,
                Some(trigger),
                "{mode}: {name} trigger"
            );
        }

        vm.collect_now(&program);
        let atom = vm.intern_atom("inherited");
        let inherited = vm.own_property(global, atom).unwrap();
        let shape = vm.object_data(inherited).unwrap().shape();
        assert!(
            vm.shape_is_dictionary(shape),
            "{mode}: dictionary mode survives GC"
        );
        let atom = vm.intern_atom("deleted");
        let deleted = vm
            .own_property(global, atom)
            .expect("global binding exists");
        let atom = vm.intern_atom("answer");
        assert_eq!(
            vm.own_property(deleted, atom).and_then(Value::as_number),
            Some(7.0),
            "{mode}: dictionary-backed property survives GC"
        );
        assert_eq!(output.borrow().as_slice(), ["42", "42", "7"], "{mode}");
        #[cfg(feature = "profile-aggregate")]
        for trigger in [
            super::DictionaryTrigger::DeletionPattern,
            super::DictionaryTrigger::PrototypeUse,
        ] {
            assert!(vm.profile.dictionary_transitions[trigger.index()] > 0);
        }
    }
}

#[test]
fn dictionary_shape_starts_when_property_slots_exceed_cache_encoding() {
    let mut vm = Vm::new(SilentHost);
    vm.shapes[0].storage_len = super::object::FIELD_CACHE_SLOT_CAPACITY;
    let atom = vm.intern_atom("overflow");
    let key = super::property_key::PropertyKey::string(atom);
    let shape = vm.transition_property_shape(0, key);
    assert!(vm.shape_is_dictionary(shape));
    assert_eq!(
        vm.shapes[shape as usize].dictionary_trigger,
        Some(super::DictionaryTrigger::PropertyCount)
    );
    assert_eq!(
        vm.property_shape_slot(shape, key),
        Some(super::object::FIELD_CACHE_SLOT_CAPACITY)
    );
    assert_eq!(
        vm.shapes[shape as usize].storage_len,
        super::object::FIELD_CACHE_SLOT_CAPACITY + 1
    );
    #[cfg(feature = "profile-aggregate")]
    assert_eq!(
        vm.profile.dictionary_transitions[super::DictionaryTrigger::PropertyCount.index()],
        1
    );
}

#[test]
fn object_method_home_survives_collection_with_precise_register_roots() {
    let source = r#"
      var base = { answer: 42 };
      var receiver = {
        __proto__: base,
        answer() { return super.answer; }
      };
      var i = 0;
      while (i < 5000) { var temporary = {}; i = i + 1; }
      print(receiver.answer());
    "#;
    for (mode, compile) in [
        ("specialized", Engine::specialize as fn(&str, &str) -> _),
        ("unspecialized", Engine::specialize_unspecialized),
    ] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(RecordingHost(output.clone()));
        let program = compile(source, "object-home-gc.js").unwrap();
        if let Err(error) = vm.execute(&program) {
            panic!("{mode}: {}", vm.format_error(&program, &error));
        }
        assert_eq!(output.borrow().as_slice(), ["42"], "{mode}");
    }
}

#[test]
fn unrepresentable_register_maps_keep_the_conservative_frame_roots() {
    let mut program = Engine::specialize("print(0);", "wide-roots.js").unwrap();
    program.functions[0].registers = 65;
    program.functions[0].register_root_offset = u32::MAX;

    let mut vm = Vm::new(SilentHost);
    vm.initialize(&program).unwrap();
    let live = vm
        .heap
        .alloc(crate::heap::Cell::Error("live register".into()));
    let mut registers = vec![Value::UNDEFINED; 65];
    registers[64] = live;
    vm.frames.push(super::Frame {
        program: super::program_store::ProgramId::MAIN,
        function: 0,
        pc: 0,
        env: Value::NULL,
        this: Value::UNDEFINED,
        locals: vec![],
        dynamic_bindings: vec![],
        captured: false,
        registers,
        active_iterators: vec![],
        with_base: 0,
    });

    vm.collect_now(&program);
    assert!(vm.heap.get(live).is_some());

    vm.frames.pop();
    vm.collect_now(&program);
    assert!(vm.heap.get(live).is_none());
}

#[test]
fn untaken_closure_branch_does_not_allocate_environments() {
    const CALLS: usize = 1_000;
    fn environments(
        body: &str,
        make: bool,
        compile: fn(&str, &str) -> Result<crate::ResidualProgram, Vec<crate::Diagnostic>>,
    ) -> usize {
        let source = format!(
            "function maybe(make) {{ var value = 1; {body} return value; }} \
             var i = 0; while (i < {CALLS}) {{ maybe({make}); i = i + 1; }}"
        );
        let program = compile(&source, "lazy-env.js").unwrap();
        let mut vm = Vm::new(SilentHost);
        vm.initialize(&program).unwrap();
        vm.heap.retain_allocations_for_test();
        let baseline = vm.heap.environment_count_for_test();
        let root = vm.closure(&program, 0, Value::NULL).unwrap();
        let globals = vm.realm.globals;
        vm.call_value(&program, root, globals, &[]).unwrap();
        assert_eq!(vm.heap.stats().1, 0, "census must retain every allocation");
        vm.heap.environment_count_for_test() - baseline
    }

    for compile in [Engine::specialize, Engine::specialize_unspecialized] {
        let no_closure = environments("", false, compile);
        let closure_branch = "if (make) return function() { return value; };";
        let untaken = environments(closure_branch, false, compile);
        let taken = environments(closure_branch, true, compile);
        assert_eq!(untaken, no_closure, "untaken closure adds environments");
        assert_eq!(taken - untaken, CALLS, "one environment per taken closure");
    }
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
                atom: 0,
                proto: crate::Value::NULL,
                guard: super::EMPTY_CACHE,
                target: Some(CallTarget::User(
                    super::program_store::ProgramId::MAIN,
                    shape,
                    crate::Value::NULL,
                )),
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
        program: None,
        root_eval_scope: false,
        function: u32::MAX,
        slots: Box::new([]),
        dynamic_bindings: vec![],
        with_objects: vec![],
    });
    let dead = vm.heap.alloc(crate::heap::Cell::Environment {
        parent: crate::Value::NULL,
        program: None,
        root_eval_scope: false,
        function: u32::MAX,
        slots: Box::new([]),
        dynamic_bindings: vec![],
        with_objects: vec![],
    });
    vm.method_caches.push([
        MethodCache {
            shape: 1,
            atom: 0,
            proto: crate::Value::NULL,
            guard: super::EMPTY_CACHE,
            target: Some(CallTarget::User(
                super::program_store::ProgramId::MAIN,
                1,
                live,
            )),
        },
        MethodCache {
            shape: 2,
            atom: 0,
            proto: crate::Value::NULL,
            guard: super::EMPTY_CACHE,
            target: Some(CallTarget::User(
                super::program_store::ProgramId::MAIN,
                2,
                dead,
            )),
        },
    ]);
    vm.heap.collect([live]);
    vm.retain_live_method_caches();
    assert!(matches!(
        vm.method_caches[0][0].target,
        Some(CallTarget::User(_, 1, env)) if env == live
    ));
    assert!(vm.method_caches[0][1].target.is_none());

    let reused = vm.heap.alloc(crate::heap::Cell::Environment {
        parent: crate::Value::NULL,
        program: None,
        root_eval_scope: false,
        function: u32::MAX,
        slots: Box::new([]),
        dynamic_bindings: vec![],
        with_objects: vec![],
    });
    assert_eq!(reused, dead);
    assert!(vm.method_caches[0][1].target.is_none());
}

struct Test262Host;
impl Host for Test262Host {
    fn write_line(&mut self, _: &str) {}
    fn clock_millis(&mut self) -> f64 {
        0.0
    }
    fn globals(&self) -> &'static [crate::HostGlobal] {
        &[crate::HostGlobal {
            name: "$262",
            capability: crate::CapabilityId::CreateRealm,
        }]
    }
}

#[test]
fn array_like_list_roots_release_after_callback_completion() {
    use super::operations::ArrayLikeElementKind;
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for property_keys in [false, true] {
            for phase in [
                "accept",
                "length-throw",
                "coercion-throw",
                "element-throw",
                "invalid",
                "reserve",
            ] {
                let mut vm = Vm::new(Test262Host);
                let source = format!(
                    r#"
                    var marker = {{kind:'{phase}'}}, reads = 0;
                    function makeList() {{return {{get length() {{
                        $262.gc(); if ('{phase}' === 'length-throw') throw marker;
                        return {{valueOf() {{
                            $262.gc(); if ('{phase}' === 'coercion-throw') throw marker;
                            return '{phase}' === 'reserve' ? Infinity : 2;
                        }}}};
                    }}, get 0() {{
                        $262.gc(); reads++;
                        if ('{phase}' === 'invalid') return undefined;
                        return {property_keys} ? Symbol('first') : {{rank:42}};
                    }}, get 1() {{
                        $262.gc(); reads++;
                        if ('{phase}' === 'element-throw') throw marker;
                        return {property_keys} ? 'later' : {{rank:43}};
                    }}}};}}
                "#
                );
                let program = compile(&source, "array-like-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let make = vm.intern_atom("makeList");
                let make = vm.own_property(vm.realm.globals, make).unwrap();
                let list = vm
                    .call_value(&program, make, Value::UNDEFINED, &[])
                    .unwrap();
                let weak = vm.heap.weak_handle(list).unwrap();
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let kind = if property_keys {
                    ArrayLikeElementKind::PropertyKey
                } else {
                    ArrayLikeElementKind::Any
                };
                let result = vm.create_list_from_array_like(&program, list, kind);
                if phase == "accept" || (phase == "invalid" && !property_keys) {
                    let values = result.unwrap();
                    assert_eq!(values.len(), 2);
                    if phase == "invalid" {
                        assert!(values[0].is_undefined());
                    } else if property_keys {
                        assert!(matches!(
                            vm.heap.get(values[0]),
                            Some(super::Cell::Symbol(_))
                        ));
                    } else {
                        let rank = vm.intern_atom("rank");
                        assert_eq!(vm.own_property(values[0], rank), Some(Value::number(42.0)));
                    }
                    if property_keys {
                        assert_eq!(vm.to_string(&program, values[1]).unwrap(), "later");
                    } else {
                        let rank = vm.intern_atom("rank");
                        assert_eq!(vm.own_property(values[1], rank), Some(Value::number(43.0)));
                    }
                } else if phase == "reserve" || phase == "invalid" {
                    assert!(
                        vm.format_error(&program, &result.unwrap_err())
                            .contains("TypeError")
                    );
                } else {
                    let marker = vm.intern_atom("marker");
                    assert_eq!(
                        result.unwrap_err().thrown_value(),
                        vm.own_property(vm.realm.globals, marker)
                    );
                }
                let reads = vm.intern_atom("reads");
                let expected = if phase == "invalid" && property_keys {
                    1.0
                } else if matches!(phase, "accept" | "invalid" | "element-throw") {
                    2.0
                } else {
                    0.0
                };
                assert_eq!(
                    vm.own_property(vm.realm.globals, reads),
                    Some(Value::number(expected))
                );
                assert_eq!(
                    vm.heap.root_count_for_test(),
                    roots,
                    "{phase}/{property_keys}"
                );
                assert_eq!(vm.active_call_roots.len(), calls, "{phase}/{property_keys}");
                vm.collect_now(&program);
                assert!(
                    vm.heap.weak_value(weak).is_none(),
                    "{phase}/{property_keys}"
                );
            }
        }
    }
}

#[test]
fn proxy_own_key_roots_release_after_invariant_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in [
            "accept",
            "lookup-throw",
            "trap-throw",
            "result-throw",
            "extensible-throw",
            "target-keys-throw",
            "descriptor-throw",
            "omit",
            "duplicate",
            "invalid-type",
            "extra",
            "revoked",
            "fallback",
        ] {
            let mut vm = Vm::new(Test262Host);
            let source = format!(
                r#"
                var marker = {{kind:'{phase}'}};
                function makeProxy() {{
                    var target = {{first:42, later:43}};
                    Object.defineProperty(target,'first',{{configurable:false}});
                    Object.preventExtensions(target);
                    target = new Proxy(target, {{isExtensible(object) {{
                        $262.gc(); if ('{phase}' === 'extensible-throw') throw marker;
                        return Reflect.isExtensible(object);
                    }}, ownKeys(object) {{
                        $262.gc(); if ('{phase}' === 'target-keys-throw') throw marker;
                        return Reflect.ownKeys(object);
                    }}, getOwnPropertyDescriptor(object,key) {{
                        $262.gc(); if ('{phase}' === 'descriptor-throw' && key === 'later') throw marker;
                        return Reflect.getOwnPropertyDescriptor(object,key);
                    }}}});
                    var handler = {{get ownKeys() {{
                        $262.gc(); if ('{phase}' === 'lookup-throw') throw marker;
                        if ('{phase}' === 'fallback') return null;
                        return function() {{
                            $262.gc(); if ('{phase}' === 'trap-throw') throw marker;
                            if ('{phase}' === 'omit') return ['later'];
                            if ('{phase}' === 'duplicate') return ['first','first'];
                            if ('{phase}' === 'extra') return ['first','later','extra'];
                            if ('{phase}' === 'invalid-type') return [undefined];
                            return {{length:2, get 0() {{return 'first';}}, get 1() {{
                                $262.gc(); if ('{phase}' === 'result-throw') throw marker; return 'later';
                            }}}};
                        }};
                    }}}};
                    if ('{phase}' === 'revoked') {{var revoked=Proxy.revocable(target,handler);revoked.revoke();return revoked.proxy;}}
                    return new Proxy(target,handler);
                }}
            "#
            );
            let program = compile(&source, "proxy-own-key-roots.js").unwrap();
            vm.execute(&program).unwrap();
            let make = vm.intern_atom("makeProxy");
            let make = vm.own_property(vm.realm.globals, make).unwrap();
            let proxy = vm
                .call_value(&program, make, Value::UNDEFINED, &[])
                .unwrap();
            let weak = vm.heap.weak_handle(proxy).unwrap();
            let roots = vm.heap.root_count_for_test();
            let calls = vm.active_call_roots.len();
            let result = vm.proxy_own_keys(&program, proxy);
            match phase {
                "accept" | "fallback" => {
                    let keys = result.unwrap();
                    assert_eq!(
                        keys.iter()
                            .map(|key| vm.to_string(&program, *key).unwrap())
                            .collect::<Vec<_>>(),
                        ["first", "later"]
                    );
                }
                "omit" | "duplicate" | "extra" | "invalid-type" | "revoked" => assert!(
                    vm.format_error(&program, &result.unwrap_err())
                        .contains("TypeError")
                ),
                _ => {
                    let marker = vm.intern_atom("marker");
                    assert_eq!(
                        result.unwrap_err().thrown_value(),
                        vm.own_property(vm.realm.globals, marker)
                    );
                }
            }
            assert_eq!(vm.heap.root_count_for_test(), roots, "{phase}");
            assert_eq!(vm.active_call_roots.len(), calls, "{phase}");
            vm.collect_now(&program);
            assert!(vm.heap.weak_value(weak).is_none(), "{phase}");
        }
    }
}

#[test]
fn own_enumeration_roots_release_after_callback_completion() {
    use super::object_keys::EnumerableOwnPropertyKind;
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for kind in ["keys", "values", "entries"] {
            for phase in ["accept", "own-keys-throw", "descriptor-throw", "get-throw"] {
                let mut vm = Vm::new(Test262Host);
                let source = format!(
                    r#"
                    var marker = {{kind:'{phase}'}};
                    function makeSource() {{return new Proxy({{}}, {{get ownKeys() {{
                        $262.gc(); return function() {{
                            $262.gc(); if ('{phase}' === 'own-keys-throw') throw marker;
                            return ['first','later',Symbol('ignored')];
                        }};
                    }}, getOwnPropertyDescriptor(object,key) {{
                        $262.gc(); if (key === 'later' && '{phase}' === 'descriptor-throw') throw marker;
                        return {{enumerable:true, configurable:true, writable:true, value:undefined}};
                    }}, get(object,key) {{
                        $262.gc(); if (key === 'later' && '{phase}' === 'get-throw') throw marker;
                        return {{rank:key === 'first' ? 42 : 43}};
                    }}}});}}
                "#
                );
                let program = compile(&source, "enumeration-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let make = vm.intern_atom("makeSource");
                let make = vm.own_property(vm.realm.globals, make).unwrap();
                let source = vm
                    .call_value(&program, make, Value::UNDEFINED, &[])
                    .unwrap();
                let source_weak = vm.heap.weak_handle(source).unwrap();
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let projection = match kind {
                    "keys" => EnumerableOwnPropertyKind::Key,
                    "values" => EnumerableOwnPropertyKind::Value,
                    _ => EnumerableOwnPropertyKind::KeyValue,
                };
                let result = vm.enumerable_own_properties(&program, source, projection);
                let succeeds = phase == "accept" || (phase == "get-throw" && kind == "keys");
                let result_weak = if succeeds {
                    let result = result.unwrap();
                    let values = vm.array_values(result).unwrap();
                    assert_eq!(values.len(), 2, "{kind}/{phase}");
                    for (index, value) in values.into_iter().enumerate() {
                        if kind == "keys" {
                            assert_eq!(
                                vm.to_string(&program, value).unwrap(),
                                ["first", "later"][index]
                            );
                        } else {
                            let value = if kind == "entries" {
                                let pair = vm.array_values(value).unwrap();
                                assert_eq!(
                                    vm.to_string(&program, pair[0]).unwrap(),
                                    ["first", "later"][index]
                                );
                                pair[1]
                            } else {
                                value
                            };
                            let rank = vm.intern_atom("rank");
                            assert_eq!(
                                vm.own_property(value, rank),
                                Some(Value::number(42.0 + index as f64))
                            );
                        }
                    }
                    Some(vm.heap.weak_handle(result).unwrap())
                } else {
                    let marker = vm.intern_atom("marker");
                    assert_eq!(
                        result.unwrap_err().thrown_value(),
                        vm.own_property(vm.realm.globals, marker)
                    );
                    None
                };
                assert_eq!(vm.heap.root_count_for_test(), roots, "{kind}/{phase}");
                assert_eq!(vm.active_call_roots.len(), calls, "{kind}/{phase}");
                vm.collect_now(&program);
                assert!(vm.heap.weak_value(source_weak).is_none(), "{kind}/{phase}");
                if let Some(result_weak) = result_weak {
                    assert!(vm.heap.weak_value(result_weak).is_none(), "{kind}/{phase}");
                }
            }
        }
    }
}

#[test]
fn proxy_set_roots_release_after_collecting_completion() {
    use super::property_key::PropertyKey;
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for symbol in [false, true] {
            for phase in [
                "accept",
                "reject",
                "lookup-throw",
                "trap-throw",
                "descriptor-throw",
                "frozen",
                "forward",
            ] {
                let mut vm = Vm::new(Test262Host);
                let source = format!(
                    r#"
                    var events = [], marker = {{kind:'{phase}'}};
                    function makeOperands() {{
                        var key = {symbol} ? Symbol('entry') : 'entry';
                        var target = {{tag:41}};
                        if ('{phase}' === 'frozen') Object.defineProperty(target, key, {{value:99}});
                        target = new Proxy(target, {{getOwnPropertyDescriptor(object, key) {{
                            $262.gc(); events.push('descriptor');
                            if ('{phase}' === 'descriptor-throw') throw marker;
                            return Reflect.getOwnPropertyDescriptor(object, key);
                        }}}});
                        var handler = {{get set() {{
                            $262.gc(); events.push('lookup');
                            if ('{phase}' === 'lookup-throw') throw marker;
                            if ('{phase}' === 'forward') return undefined;
                            return function(target, key, value, receiver) {{
                                $262.gc(); events.push('trap');
                                if (target.tag !== 41 || value.rank !== 42 || receiver.rank !== 43
                                    || typeof key !== ({symbol} ? 'symbol' : 'string')) throw 'lost operand';
                                if ('{phase}' === 'trap-throw') throw marker;
                                return '{phase}' !== 'reject';
                            }};
                        }}}};
                        return [target, handler, {{rank:43}}, key, {{rank:42}}];
                    }}
                "#
                );
                let program = compile(&source, "proxy-set-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let make = vm.intern_atom("makeOperands");
                let make = vm.own_property(vm.realm.globals, make).unwrap();
                let operands = vm
                    .call_value(&program, make, Value::UNDEFINED, &[])
                    .unwrap();
                let [target, handler, receiver, key, value]: [Value; 5] =
                    vm.array_values(operands).unwrap().try_into().unwrap();
                let weak = vm.heap.weak_handle(value).unwrap();
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let property = if symbol {
                    PropertyKey::symbol(key)
                } else {
                    PropertyKey::string(vm.intern_atom("entry"))
                };
                let result = vm.proxy_set(&program, target, handler, receiver, property, value);
                match phase {
                    "accept" | "forward" => assert!(result.unwrap()),
                    "reject" => assert!(!result.unwrap()),
                    "frozen" => assert!(
                        vm.format_error(&program, &result.unwrap_err())
                            .contains("TypeError")
                    ),
                    _ => {
                        let marker = vm.intern_atom("marker");
                        assert_eq!(
                            result.unwrap_err().thrown_value(),
                            vm.own_property(vm.realm.globals, marker)
                        );
                    }
                }
                assert_eq!(
                    vm.heap.root_count_for_test(),
                    roots,
                    "{phase}, symbol={symbol}"
                );
                assert_eq!(
                    vm.active_call_roots.len(),
                    calls,
                    "{phase}, symbol={symbol}"
                );
                let rank = vm.intern_atom("rank");
                assert_eq!(
                    vm.own_property(value, rank),
                    Some(Value::number(42.0)),
                    "{phase}"
                );
                let events = vm.intern_atom("events");
                let events = vm.own_property(vm.realm.globals, events).unwrap();
                let observed = vm
                    .array_values(events)
                    .unwrap()
                    .iter()
                    .map(|value| vm.to_string(&program, *value).unwrap())
                    .collect::<Vec<_>>();
                let expected: &[&str] = match phase {
                    "lookup-throw" | "forward" => &["lookup"],
                    "reject" | "trap-throw" => &["lookup", "trap"],
                    _ => &["lookup", "trap", "descriptor"],
                };
                assert_eq!(observed, expected, "{phase}, symbol={symbol}");
                vm.collect_now(&program);
                assert!(
                    vm.heap.weak_value(weak).is_none(),
                    "{phase}, symbol={symbol}"
                );
            }
        }
    }
}

#[test]
fn receiver_proxy_writes_root_operands_across_traps() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for symbol in [false, true] {
            for existing in [false, true] {
                for phase in ["accept", "reject", "descriptor-throw", "define-throw"] {
                    let mut vm = Vm::new(Test262Host);
                    let source = format!(
                        r#"
                    var marker = {{kind:'{phase}'}};
                    function makeOperands() {{
                        var key = {symbol} ? Symbol('entry') : 'entry', receiver = {{tag:41}};
                        if ({existing}) receiver[key] = 99;
                        receiver = new Proxy(receiver, {{getOwnPropertyDescriptor(object, key) {{
                            $262.gc();
                            if (({symbol} ? key.description : key) !== 'entry' || object.tag !== 41) throw 'lost operand';
                            if ('{phase}' === 'descriptor-throw') throw marker;
                            return Reflect.getOwnPropertyDescriptor(object, key);
                        }}, defineProperty(object, key, descriptor) {{
                            $262.gc();
                            if (({symbol} ? key.description : key) !== 'entry' || descriptor.value.rank !== 42
                                || Object.hasOwn(descriptor, 'writable') !== !{existing}) throw 'lost operand';
                            if ('{phase}' === 'define-throw') throw marker;
                            if ('{phase}' === 'reject') return false;
                            return Reflect.defineProperty(object, key, descriptor);
                        }}}});
                        return [{{}}, key, {{rank:42}}, receiver];
                    }}
                "#
                    );
                    let program = compile(&source, "symbol-receiver-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let make = vm.intern_atom("makeOperands");
                    let make = vm.own_property(vm.realm.globals, make).unwrap();
                    let operands = vm
                        .call_value(&program, make, Value::UNDEFINED, &[])
                        .unwrap();
                    let [target, key, value, receiver]: [Value; 4] =
                        vm.array_values(operands).unwrap().try_into().unwrap();
                    let weak = vm.heap.weak_handle(value).unwrap();
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let result = if symbol {
                        vm.set_symbol_property_with_receiver(&program, target, key, value, receiver)
                    } else {
                        let atom = vm.intern_atom("entry");
                        vm.set_property_with_receiver(&program, target, atom, value, receiver)
                    };
                    match phase {
                        "accept" => assert!(result.unwrap()),
                        "reject" => assert!(!result.unwrap()),
                        _ => {
                            let marker = vm.intern_atom("marker");
                            assert_eq!(
                                result.unwrap_err().thrown_value(),
                                vm.own_property(vm.realm.globals, marker)
                            );
                        }
                    }
                    assert_eq!(
                        vm.heap.root_count_for_test(),
                        roots,
                        "{phase}, existing={existing}"
                    );
                    assert_eq!(
                        vm.active_call_roots.len(),
                        calls,
                        "{phase}, existing={existing}"
                    );
                    let rank = vm.intern_atom("rank");
                    assert_eq!(
                        vm.own_property(value, rank),
                        Some(Value::number(42.0)),
                        "{phase}"
                    );
                    vm.collect_now(&program);
                    assert!(
                        vm.heap.weak_value(weak).is_none(),
                        "{phase}, existing={existing}"
                    );
                }
            }
        }
    }
}

#[test]
fn legacy_accessor_record_roots_release_after_each_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for native in [
            Native::ObjectPrototypeDefineGetter,
            Native::ObjectPrototypeDefineSetter,
        ] {
            for phase in [
                "success",
                "boxed",
                "key-throw",
                "invalid",
                "proxy-reject",
                "proxy-throw",
            ] {
                let mut vm = Vm::new(Test262Host);
                let source = format!(
                    r#"
                    var marker = {{kind:'{phase}'}};
                    var target = '{phase}' === 'boxed' ? 1 : {{}};
                    if ('{phase}'.startsWith('proxy-')) target = new Proxy(target, {{defineProperty() {{
                        $262.gc(); if ('{phase}' === 'proxy-throw') throw marker; return false;
                    }}}});
                    var key = {{toString() {{$262.gc(); if ('{phase}' === 'key-throw') throw marker; return 'entry';}}}};
                    var makeAccessor = function() {{return function() {{return 42;}};}};
                "#
                );
                let program = compile(&source, "legacy-accessor-record-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let values = ["target", "key", "marker", "makeAccessor"].map(|name| {
                    let atom = vm.intern_atom(name);
                    vm.own_property(vm.realm.globals, atom).unwrap()
                });
                let accessor = vm
                    .call_value(&program, values[3], Value::UNDEFINED, &[])
                    .unwrap();
                let weak = vm.heap.weak_handle(accessor).unwrap();
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let argument = if phase == "invalid" {
                    Value::UNDEFINED
                } else {
                    accessor
                };
                let result = vm.object_prototype_define_accessor(
                    &program,
                    native,
                    values[0],
                    &[values[1], argument],
                );
                match phase {
                    "success" | "boxed" => assert_eq!(result.unwrap(), Value::UNDEFINED),
                    "invalid" | "proxy-reject" => {
                        let error = result.unwrap_err();
                        assert!(vm.format_error(&program, &error).contains("TypeError"));
                    }
                    _ => assert_eq!(result.unwrap_err().thrown_value(), Some(values[2])),
                }
                assert_eq!(vm.heap.root_count_for_test(), roots, "{phase}");
                assert_eq!(vm.active_call_roots.len(), calls, "{phase}");
                vm.collect_now(&program);
                assert_eq!(
                    vm.heap.weak_value(weak).is_some(),
                    phase == "success",
                    "{phase}"
                );
            }
        }
    }
}

#[test]
fn class_field_record_roots_release_after_each_completion() {
    use super::property_key::PropertyKey;
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in [
            "ordinary",
            "symbol",
            "private",
            "reject",
            "proxy-reject",
            "proxy-throw",
        ] {
            let mut vm = Vm::new(Test262Host);
            let source = format!(
                r#"
                var target = {{}}, marker = {{kind:'{phase}'}}, symbol = Symbol('entry');
                if ('{phase}' === 'reject') Object.preventExtensions(target);
                if ('{phase}'.startsWith('proxy-')) target = new Proxy(target, {{defineProperty() {{
                    $262.gc(); if ('{phase}' === 'proxy-throw') throw marker; return false;
                }}}});
                var makeValue = function() {{return {{rank:42}};}};
            "#
            );
            let program = compile(&source, "class-field-record-roots.js").unwrap();
            vm.execute(&program).unwrap();
            let values = ["target", "marker", "symbol", "makeValue"].map(|name| {
                let atom = vm.intern_atom(name);
                vm.own_property(vm.realm.globals, atom).unwrap()
            });
            let atom = vm.intern_atom("entry");
            let key = match phase {
                "symbol" => PropertyKey::symbol(values[2]),
                "private" => PropertyKey::private(atom),
                _ => PropertyKey::string(atom),
            };
            let value = vm
                .call_value(&program, values[3], Value::UNDEFINED, &[])
                .unwrap();
            let weak = vm.heap.weak_handle(value).unwrap();
            let roots = vm.heap.root_count_for_test();
            let calls = vm.active_call_roots.len();
            let result = vm.define_class_field(&program, values[0], key, value);
            match phase {
                "ordinary" | "symbol" => result.unwrap(),
                "proxy-throw" => assert_eq!(result.unwrap_err().thrown_value(), Some(values[1])),
                "private" => assert!(
                    result
                        .unwrap_err()
                        .to_string()
                        .contains("private names are not public class-field keys")
                ),
                _ => {
                    let error = result.unwrap_err();
                    assert!(vm.format_error(&program, &error).contains("TypeError"));
                }
            }
            assert_eq!(vm.heap.root_count_for_test(), roots, "{phase}");
            assert_eq!(vm.active_call_roots.len(), calls, "{phase}");
            vm.collect_now(&program);
            assert_eq!(
                vm.heap.weak_value(weak).is_some(),
                matches!(phase, "ordinary" | "symbol"),
                "{phase}"
            );
        }
    }
}

#[test]
fn json_revival_roots_release_after_each_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in [
            "reviver-throw",
            "get-throw",
            "keys-throw",
            "descriptor-throw",
            "is-array-throw",
            "length-throw",
            "define-throw",
            "delete-throw",
            "success",
        ] {
            let mut vm = Vm::new(Test262Host);
            let source = format!(
                r#"
                var marker = {{kind:'{phase}'}};
                var revive = function(key,value) {{
                    $262.gc();
                    if (key === 'first') {{
                        if ('{phase}' === 'get-throw') this.target = {{get entry() {{$262.gc(); throw marker;}}}};
                        if ('{phase}' === 'keys-throw') this.target = new Proxy({{}}, {{ownKeys() {{$262.gc(); throw marker;}}}});
                        if ('{phase}' === 'descriptor-throw') this.target = new Proxy({{entry:1}}, {{getOwnPropertyDescriptor() {{$262.gc(); throw marker;}}}});
                        if ('{phase}' === 'is-array-throw') {{var revoked = Proxy.revocable({{}},{{}}); revoked.revoke(); this.target = revoked.proxy;}}
                        if ('{phase}' === 'length-throw') this.target = new Proxy([], {{get(object,key) {{$262.gc(); if (key === 'length') throw marker; return Reflect.get(object,key);}}}});
                        if ('{phase}' === 'define-throw') this.target = new Proxy({{entry:1}}, {{defineProperty() {{$262.gc(); throw marker;}}}});
                        if ('{phase}' === 'delete-throw') this.target = new Proxy({{entry:1}}, {{deleteProperty() {{$262.gc(); throw marker;}}}});
                    }}
                    if (key === 'entry') {{
                        if ('{phase}' === 'reviver-throw') throw marker;
                        if ('{phase}' === 'delete-throw') return undefined;
                        return {{rank:42}};
                    }}
                    return value;
                }};
            "#
            );
            let program = compile(&source, "json-revival-roots.js").unwrap();
            vm.execute(&program).unwrap();
            let values = ["revive", "marker"].map(|name| {
                let atom = vm.intern_atom(name);
                vm.own_property(vm.realm.globals, atom).unwrap()
            });
            let text = vm.heap.alloc(super::Cell::String(
                r#"{"first":0,"target":{"entry":1}}"#.into(),
            ));
            let roots = vm.heap.root_count_for_test();
            let calls = vm.active_call_roots.len();
            let result = vm.json_parse(&program, &[text, values[0]]);
            let weak = match phase {
                "success" => {
                    let result = result.unwrap();
                    let target = vm.intern_atom("target");
                    let target = vm.own_property(result, target).unwrap();
                    let entry = vm.intern_atom("entry");
                    let entry = vm.own_property(target, entry).unwrap();
                    let rank = vm.intern_atom("rank");
                    assert_eq!(vm.own_property(entry, rank), Some(Value::number(42.0)));
                    Some(vm.heap.weak_handle(result).unwrap())
                }
                "is-array-throw" => {
                    let error = result.unwrap_err();
                    assert!(vm.format_error(&program, &error).contains("TypeError"));
                    None
                }
                _ => {
                    assert_eq!(
                        result.unwrap_err().thrown_value(),
                        Some(values[1]),
                        "{phase}"
                    );
                    None
                }
            };
            assert_eq!(vm.heap.root_count_for_test(), roots, "{phase}");
            assert_eq!(vm.active_call_roots.len(), calls, "{phase}");
            vm.collect_now(&program);
            if let Some(weak) = weak {
                assert!(vm.heap.weak_value(weak).is_none());
            }
        }
    }
}

#[test]
fn definition_batch_roots_release_after_each_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in [
            "own-keys-throw",
            "own-descriptor-throw",
            "get-descriptor-throw",
            "parse-throw",
            "invalid-late",
            "define-reject",
            "define-throw",
            "success",
        ] {
            let mut vm = Vm::new(Test262Host);
            let source = format!(
                r#"
                var weak, marker = {{kind:'{phase}'}};
                var backing = {{}};
                var target = new Proxy(backing, {{defineProperty(object,key,descriptor) {{
                    $262.gc();
                    if (key === 'second' && '{phase}' === 'define-reject') return false;
                    if (key === 'second' && '{phase}' === 'define-throw') throw marker;
                    return Reflect.defineProperty(object,key,descriptor);
                }}}});
                var descriptors = new Proxy({{
                    first: {{get value() {{var value = {{rank:42}}; weak = new WeakRef(value); $262.gc(); return value;}}}},
                    second: {{get value() {{$262.gc(); if ('{phase}' === 'parse-throw') throw marker; return 43;}}}}
                }}, {{
                    ownKeys() {{$262.gc(); if ('{phase}' === 'own-keys-throw') throw marker; return ['first','second'];}},
                    getOwnPropertyDescriptor(object,key) {{$262.gc();
                        if (key === 'second' && '{phase}' === 'own-descriptor-throw') throw marker;
                        return {{enumerable:true, configurable:true}};
                    }},
                    get(object,key) {{$262.gc();
                        if (key === 'second' && '{phase}' === 'get-descriptor-throw') throw marker;
                        if (key === 'second' && '{phase}' === 'invalid-late') return {{get:1}};
                        return Reflect.get(object,key);
                    }}
                }});
            "#
            );
            let program = compile(&source, "definition-batch-roots.js").unwrap();
            vm.execute(&program).unwrap();
            let values = ["target", "descriptors", "marker"].map(|name| {
                let atom = vm.intern_atom(name);
                vm.own_property(vm.realm.globals, atom).unwrap()
            });
            let roots = vm.heap.root_count_for_test();
            let calls = vm.active_call_roots.len();
            let result = vm.object_define_properties(&program, &values[..2]);
            match phase {
                "success" => assert_eq!(result.unwrap(), values[0]),
                "invalid-late" | "define-reject" => {
                    let error = result.unwrap_err();
                    assert!(vm.format_error(&program, &error).contains("TypeError"));
                }
                _ => assert_eq!(result.unwrap_err().thrown_value(), Some(values[2])),
            }
            assert_eq!(vm.heap.root_count_for_test(), roots, "{phase}");
            assert_eq!(vm.active_call_roots.len(), calls, "{phase}");
            let weak = vm.intern_atom("weak");
            let weak = vm.own_property(vm.realm.globals, weak).unwrap();
            let weak = match vm.heap.get(weak) {
                Some(super::Cell::WeakRef { target, .. }) => *target,
                _ => None,
            };
            vm.collect_now(&program);
            if phase != "own-keys-throw" {
                let weak = weak.expect("first descriptor created a weak value tracker");
                assert_eq!(
                    vm.heap.weak_value(weak).is_some(),
                    matches!(phase, "success" | "define-reject" | "define-throw"),
                    "{phase}"
                );
            }
        }
    }
}

#[test]
fn definition_record_stack_exhaustion_preserves_root_scopes() {
    let mut vm = Vm::new(Test262Host);
    let program = Engine::specialize("var target = {};", "definition-stack-budget.js").unwrap();
    vm.execute(&program).unwrap();
    let atom = vm.intern_atom("target");
    let target = vm.own_property(vm.realm.globals, atom).unwrap();
    let key = vm.heap.alloc(super::Cell::String("entry".into()));
    let record = super::object_descriptors::PropertyDescriptorRecord::data(Value::number(42.0));
    let roots = vm.heap.root_count_for_test();
    let calls = vm.active_call_roots.len();
    let mut guards = Vec::new();
    while let Ok(guard) = quench_stack::StackGuard::enter() {
        guards.push(guard);
    }
    let error = vm
        .define_own_property_record(&program, target, key, record)
        .unwrap_err();
    drop(guards);
    assert!(
        vm.format_error(&program, &error)
            .contains(quench_stack::STACK_EXHAUSTED_MESSAGE)
    );
    assert_eq!(vm.heap.root_count_for_test(), roots);
    assert_eq!(vm.active_call_roots.len(), calls);
    assert!(
        vm.define_own_property_record(&program, target, key, record)
            .unwrap()
    );
}

#[test]
fn definition_record_roots_release_across_target_kinds() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in [
            "ordinary-ok",
            "ordinary-reject",
            "array-ok",
            "array-reject",
            "length-ok",
            "length-reject",
            "length-throw",
            "typed-ok",
            "typed-reject",
            "typed-throw",
            "proxy-ok",
            "proxy-reject",
            "proxy-throw",
            "proxy-invariant",
        ] {
            let mut vm = Vm::new(Test262Host);
            let source = format!(
                r#"
                var target={{}};
                if ('{phase}'.startsWith('array-') || '{phase}'.startsWith('length-')) target=[];
                if ('{phase}'.startsWith('typed-')) target=new Uint8Array('{phase}' === 'typed-reject' ? 0 : 1);
                if ('{phase}' === 'ordinary-reject' || '{phase}' === 'array-reject' || '{phase}' === 'length-reject') Object.freeze(target);
                if ('{phase}'.startsWith('proxy-')) target=new Proxy(target, {{get defineProperty() {{$262.gc();
                    return function(target,key,descriptor) {{$262.gc(); if ('{phase}' === 'proxy-throw') throw {{kind:'{phase}'}}; return '{phase}' !== 'proxy-reject';}};
                }}}});
                var key='{phase}'.startsWith('length-') ? 'length' : '{phase}'.startsWith('array-') || '{phase}'.startsWith('typed-') ? '0' : 'entry';
                var descriptor={{get value() {{$262.gc(); return {{rank:42, valueOf() {{$262.gc();
                    if ('{phase}' === 'length-throw' || '{phase}' === 'typed-throw') throw {{kind:'{phase}'}}; return 2;
                }}}};}}, writable:true, enumerable:!'{phase}'.startsWith('length-'), configurable:!'{phase}'.startsWith('length-') && '{phase}' !== 'proxy-invariant'}};
            "#
            );
            let program = compile(&source, "definition-record-roots.js").unwrap();
            vm.execute(&program).unwrap();
            let values = ["target", "key", "descriptor"].map(|name| {
                let atom = vm.intern_atom(name);
                vm.own_property(vm.realm.globals, atom).unwrap()
            });
            let record = vm.to_property_descriptor(&program, values[2]).unwrap();
            let weak = vm.heap.weak_handle(record.value.unwrap()).unwrap();
            let roots = vm.heap.root_count_for_test();
            let calls = vm.active_call_roots.len();
            let result = vm.define_own_property_record(&program, values[0], values[1], record);
            if phase.ends_with("-throw") || phase == "proxy-invariant" {
                let error = result.unwrap_err();
                if phase == "proxy-invariant" {
                    assert!(vm.format_error(&program, &error).contains("TypeError"));
                } else {
                    let atom = vm.intern_atom("kind");
                    let kind = vm
                        .own_property(error.thrown_value().unwrap(), atom)
                        .unwrap();
                    assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                }
            } else {
                assert_eq!(result.unwrap(), phase.ends_with("-ok"), "{phase}");
            }
            assert_eq!(vm.heap.root_count_for_test(), roots);
            assert_eq!(vm.active_call_roots.len(), calls);
            vm.collect_now(&program);
            assert_eq!(
                vm.heap.weak_value(weak).is_some(),
                matches!(phase, "ordinary-ok" | "array-ok"),
                "{phase}"
            );
        }
    }
}

#[test]
fn descriptor_record_roots_release_after_field_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in [
            "data-success",
            "accessor-success",
            "writable-throw",
            "get-throw",
            "set-throw",
            "invalid-get",
            "invalid-set",
            "mixed",
        ] {
            let mut vm = Vm::new(Test262Host);
            let source = format!(
                r#"
                var descriptor = '{phase}' === 'data-success' || '{phase}' === 'writable-throw' || '{phase}' === 'mixed'
                    ? {{get value() {{$262.gc(); return {{rank:42}};}},
                        get writable() {{$262.gc(); if ('{phase}' === 'writable-throw') throw {{kind:'{phase}'}}; return true;}}}}
                    : {{}};
                if ('{phase}' !== 'data-success' && '{phase}' !== 'writable-throw') {{
                    Object.defineProperty(descriptor, 'get', {{get() {{$262.gc();
                        if ('{phase}' === 'get-throw') throw {{kind:'{phase}'}};
                        if ('{phase}' === 'invalid-get') return 1;
                        return function() {{return 42;}};
                    }}}});
                    Object.defineProperty(descriptor, 'set', {{get() {{$262.gc();
                        if ('{phase}' === 'set-throw') throw {{kind:'{phase}'}};
                        if ('{phase}' === 'invalid-set') return 1;
                        return function(value) {{this.saved = value;}};
                    }}}});
                }}
            "#
            );
            let program = compile(&source, "descriptor-record-roots.js").unwrap();
            vm.execute(&program).unwrap();
            let atom = vm.intern_atom("descriptor");
            let descriptor = vm.own_property(vm.realm.globals, atom).unwrap();
            let roots = vm.heap.root_count_for_test();
            let calls = vm.active_call_roots.len();
            let result = vm.to_property_descriptor(&program, descriptor);
            assert_eq!(result.is_ok(), phase.ends_with("success"), "{phase}");
            assert_eq!(vm.heap.root_count_for_test(), roots);
            assert_eq!(vm.active_call_roots.len(), calls);
            if let Err(error) = &result {
                if phase.ends_with("-throw") {
                    let thrown = error.thrown_value().unwrap();
                    let atom = vm.intern_atom("kind");
                    let kind = vm.own_property(thrown, atom).unwrap();
                    assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                } else {
                    assert!(vm.format_error(&program, error).contains("TypeError"));
                }
            }
            if let Ok(record) = result {
                let value = if phase == "data-success" {
                    let value = record.value.unwrap();
                    let atom = vm.intern_atom("rank");
                    assert_eq!(vm.own_property(value, atom), Some(Value::number(42.0)));
                    value
                } else {
                    let getter = record.getter.unwrap();
                    assert_eq!(
                        vm.call_value(&program, getter, Value::UNDEFINED, &[])
                            .unwrap(),
                        Value::number(42.0)
                    );
                    getter
                };
                let weak = vm.heap.weak_handle(value).unwrap();
                let projection = vm.from_property_descriptor(record).unwrap();
                assert_eq!(vm.heap.root_count_for_test(), roots);
                let projection = vm.heap.weak_handle(projection).unwrap();
                vm.collect_now(&program);
                assert!(vm.heap.weak_value(weak).is_none());
                assert!(vm.heap.weak_value(projection).is_none());
            }
        }
    }
}

#[test]
fn error_stack_setter_roots_release_after_each_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in [
            "missing",
            "data",
            "accessor",
            "empty-accessor",
            "getter-only",
            "readonly",
            "nonextensible",
            "home",
            "primitive",
            "nonstring",
            "descriptor-lookup-throw",
            "descriptor-call-throw",
            "define-lookup-throw",
            "define-call-throw",
            "define-false",
            "set-false",
            "set-call-throw",
            "accessor-throw",
        ] {
            let mut vm = Vm::new(Test262Host);
            let source = format!(
                r#"
                var marker = {{rank:42}};
                function factory() {{
                    if ('{phase}' === 'home') return Error.prototype;
                    if ('{phase}' === 'primitive') return 1;
                    var target = {{}};
                    if ('{phase}' === 'data' || '{phase}' === 'readonly' || '{phase}'.startsWith('set-'))
                        Object.defineProperty(target, 'stack', {{value:'old', writable:'{phase}' !== 'readonly', configurable:true}});
                    if ('{phase}' === 'accessor' || '{phase}' === 'accessor-throw')
                        Object.defineProperty(target, 'stack', {{set(v) {{$262.gc(); if ('{phase}' === 'accessor-throw') throw marker; this.saved = v;}}}});
                    if ('{phase}' === 'empty-accessor') Object.defineProperty(target, 'stack', {{get:undefined, set:undefined}});
                    if ('{phase}' === 'getter-only') Object.defineProperty(target, 'stack', {{get() {{return 'old';}}}});
                    if ('{phase}' === 'nonextensible') Object.preventExtensions(target);
                    if ('{phase}'.startsWith('descriptor-') || '{phase}'.startsWith('define-') || '{phase}'.startsWith('set-'))
                        return new Proxy(target, {{
                            get getOwnPropertyDescriptor() {{$262.gc(); if ('{phase}' === 'descriptor-lookup-throw') throw marker;
                                return function(t,k) {{$262.gc(); if ('{phase}' === 'descriptor-call-throw') throw marker; return Reflect.getOwnPropertyDescriptor(t,k);}};
                            }},
                            get defineProperty() {{$262.gc(); if ('{phase}' === 'define-lookup-throw') throw marker;
                                return function(t,k,d) {{$262.gc(); if ('{phase}' === 'define-call-throw') throw marker;
                                    if ('{phase}' === 'define-false') return false; return Reflect.defineProperty(t,k,d);}};
                            }},
                            set(t,k,v) {{$262.gc(); if ('{phase}' === 'set-call-throw') throw marker;
                                if ('{phase}' === 'set-false') return false; return Reflect.set(t,k,v,t);
                            }}
                        }});
                    return target;
                }}
            "#
            );
            let program = compile(&source, "error-stack-root-lifecycle.js").unwrap();
            vm.execute(&program).unwrap();
            let atom = vm.intern_atom("factory");
            let factory = vm.own_property(vm.realm.globals, atom).unwrap();
            let target = vm
                .call_value(&program, factory, Value::UNDEFINED, &[])
                .unwrap();
            let hold = vm.heap.root(target);
            let value = if phase == "nonstring" {
                Value::number(1.0)
            } else {
                vm.heap
                    .alloc(crate::heap::Cell::String("fresh-stack".into()))
            };
            let target = vm.heap.root_value(hold).unwrap();
            let target_weak = vm.heap.weak_handle(target);
            let value_weak = vm.heap.weak_handle(value);
            vm.heap.release_root(hold);
            let roots = vm.heap.root_count_for_test();
            let calls = vm.active_call_roots.len();
            let outcome = vm.error_stack_setter(&program, target, &[value]);
            assert_eq!(
                outcome.is_ok(),
                matches!(phase, "missing" | "data" | "accessor"),
                "{phase}"
            );
            if let Err(error) = outcome {
                if phase.ends_with("-throw") {
                    let atom = vm.intern_atom("marker");
                    assert_eq!(
                        error.thrown_value(),
                        vm.own_property(vm.realm.globals, atom)
                    );
                } else {
                    assert!(
                        vm.format_error(&program, &error).starts_with("TypeError:"),
                        "{phase}"
                    );
                }
            }
            assert_eq!(vm.heap.root_count_for_test(), roots, "{phase}");
            assert_eq!(vm.active_call_roots.len(), calls);
            vm.collect_now(&program);
            if let Some(weak) = target_weak {
                assert_eq!(
                    vm.heap.weak_value(weak).is_some(),
                    phase == "home",
                    "{phase}"
                );
            }
            if let Some(weak) = value_weak {
                assert!(vm.heap.weak_value(weak).is_none(), "{phase}");
            }
        }
    }
}

#[test]
fn module_namespace_operations_share_uninitialized_export_errors() {
    for compile in [
        Engine::specialize_module as fn(&str, &str) -> _,
        Engine::specialize_module_unspecialized,
    ] {
        for operation in [
            "get",
            "descriptor",
            "define",
            "seal",
            "freeze",
            "is-sealed",
            "is-frozen",
        ] {
            let mut vm = Vm::new(Test262Host);
            let program = compile("export let answer;", "uninitialized-module-export.mjs").unwrap();
            vm.initialize(&program).unwrap();
            let namespace = vm.object();
            let atom = vm.intern_atom("answer");
            vm.set_property(namespace, atom, Value::UNDEFINED).unwrap();
            let slot = program.functions[0]
                .local_atoms
                .iter()
                .position(|candidate| *candidate == atom)
                .unwrap();
            let environment = vm.heap.alloc(crate::heap::Cell::Environment {
                parent: Value::NULL,
                program: Some(super::program_store::ProgramId::MAIN.raw()),
                root_eval_scope: false,
                function: super::ROOT_FUNCTION_ID,
                slots: vec![Value::DELETED; program.functions[0].local_atoms.len()]
                    .into_boxed_slice(),
                dynamic_bindings: vec![],
                with_objects: vec![],
            });
            vm.programs
                .set_module_environment(super::program_store::ProgramId::MAIN, environment);
            let attributes = super::PropertyAttributes {
                configurable: false,
                ..super::DEFAULT_PROPERTY_ATTRIBUTES
            };
            vm.set_property_attributes(
                namespace,
                super::property_key::PropertyKey::string(atom),
                attributes,
            );
            let object = vm.object_data_mut(namespace).unwrap();
            object.proto = Value::NULL;
            object.set_module_namespace();
            object.set_module_bindings(vec![(
                atom,
                super::program_store::ProgramId::MAIN,
                u16::try_from(slot).unwrap(),
            )]);
            object.set_extensible(false);
            let key = vm.heap.alloc(crate::heap::Cell::String("answer".into()));
            let roots = vm.heap.root_count_for_test();
            let calls = vm.active_call_roots.len();
            let outcome = match operation {
                "get" => vm.get_property(&program, namespace, atom),
                "descriptor" => vm.object_get_own_property_descriptor(&program, &[namespace, key]),
                "define" => vm
                    .define_own_property_record(
                        &program,
                        namespace,
                        key,
                        super::object_descriptors::PropertyDescriptorRecord::data(Value::number(
                            42.0,
                        )),
                    )
                    .map(Vm::<Test262Host>::integrity_bool),
                "seal" | "freeze" => {
                    vm.object_set_integrity(&program, &[namespace], operation == "freeze")
                }
                "is-sealed" | "is-frozen" => {
                    vm.object_is_integrity_level(&program, &[namespace], operation == "is-frozen")
                }
                _ => unreachable!(),
            };
            let error = outcome.unwrap_err();
            assert!(
                vm.format_error(&program, &error)
                    .starts_with("ReferenceError:"),
                "{operation}"
            );
            assert_eq!(vm.heap.root_count_for_test(), roots);
            assert_eq!(vm.active_call_roots.len(), calls);
        }
    }
}

#[test]
fn integrity_operation_roots_release_after_each_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for freeze in [false, true] {
            for test in [false, true] {
                let phases: &[&str] = if test {
                    &[
                        "success",
                        "extensible",
                        "extensible-lookup-throw",
                        "extensible-call-throw",
                        "keys-lookup-throw",
                        "keys-call-throw",
                        "descriptor-lookup-throw",
                        "descriptor-call-throw",
                        "configurable",
                        "writable",
                        "primitive",
                        "revoked",
                    ]
                } else {
                    &[
                        "success",
                        "prevent-lookup-throw",
                        "prevent-call-throw",
                        "prevent-false",
                        "keys-lookup-throw",
                        "keys-call-throw",
                        "descriptor-lookup-throw",
                        "descriptor-call-throw",
                        "define-lookup-throw",
                        "define-call-throw",
                        "define-false",
                        "primitive",
                        "revoked",
                    ]
                };
                for &phase in phases {
                    let mut vm = Vm::new(Test262Host);
                    let source = format!(
                        r#"
                        var marker = {{rank:42}};
                        function factory() {{
                            if ('{phase}' === 'primitive') return 1;
                            var target = {{alpha:42, beta:43}};
                            if ({test} && '{phase}' !== 'extensible') {{
                                Object.defineProperty(target, 'alpha', {{configurable:'{phase}' === 'configurable', writable:!{freeze} || '{phase}' === 'writable'}});
                                Object.defineProperty(target, 'beta', {{configurable:false, writable:!{freeze}}});
                                Object.preventExtensions(target);
                            }}
                            var handler = {{
                                get preventExtensions() {{$262.gc(); if ('{phase}' === 'prevent-lookup-throw') throw marker;
                                    return function(t) {{$262.gc(); if ('{phase}' === 'prevent-call-throw') throw marker;
                                        if ('{phase}' === 'prevent-false') return false;
                                        return Reflect.preventExtensions(t);
                                    }};
                                }},
                                get isExtensible() {{$262.gc(); if ('{phase}' === 'extensible-lookup-throw') throw marker;
                                    return function(t) {{$262.gc(); if ('{phase}' === 'extensible-call-throw') throw marker; return Reflect.isExtensible(t);}};
                                }},
                                get ownKeys() {{$262.gc(); if ('{phase}' === 'keys-lookup-throw') throw marker;
                                    return function(t) {{$262.gc(); if ('{phase}' === 'keys-call-throw') throw marker; return ['al' + 'pha', 'be' + 'ta'];}};
                                }},
                                get getOwnPropertyDescriptor() {{$262.gc(); if ('{phase}' === 'descriptor-lookup-throw') throw marker;
                                    return function(t,k) {{$262.gc(); if ('{phase}' === 'descriptor-call-throw') throw marker; return Reflect.getOwnPropertyDescriptor(t,k);}};
                                }},
                                get defineProperty() {{$262.gc(); if ('{phase}' === 'define-lookup-throw') throw marker;
                                    return function(t,k,d) {{$262.gc(); if ('{phase}' === 'define-call-throw') throw marker;
                                        if ('{phase}' === 'define-false') return false;
                                        return Reflect.defineProperty(t,k,d);
                                    }};
                                }}
                            }};
                            if ('{phase}' === 'revoked') {{var revocable = Proxy.revocable(target, handler); revocable.revoke(); return revocable.proxy;}}
                            return new Proxy(target, handler);
                        }}
                    "#
                    );
                    let program = compile(&source, "integrity-root-lifecycle.js").unwrap();
                    vm.execute(&program).unwrap();
                    let atom = vm.intern_atom("factory");
                    let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                    let object = vm
                        .call_value(&program, factory, Value::UNDEFINED, &[])
                        .unwrap();
                    let weak = vm.heap.weak_handle(object);
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let outcome = if test {
                        vm.object_is_integrity_level(&program, &[object], freeze)
                    } else {
                        vm.object_set_integrity(&program, &[object], freeze)
                    };
                    let descriptor_skipped = !test && !freeze && phase.starts_with("descriptor-");
                    let fails = phase.ends_with("-throw") && !descriptor_skipped
                        || phase == "revoked"
                        || phase == "prevent-false"
                        || phase == "define-false";
                    assert_eq!(
                        outcome.is_err(),
                        fails,
                        "test={test} freeze={freeze} phase={phase}"
                    );
                    if let Err(error) = outcome {
                        if phase.ends_with("-throw") {
                            let atom = vm.intern_atom("marker");
                            assert_eq!(
                                error.thrown_value(),
                                vm.own_property(vm.realm.globals, atom)
                            );
                        }
                    } else if test {
                        let expected = phase != "extensible"
                            && phase != "configurable"
                            && !(freeze && phase == "writable");
                        assert_eq!(
                            outcome.unwrap(),
                            Vm::<Test262Host>::integrity_bool(expected),
                            "{phase}"
                        );
                    } else {
                        assert_eq!(outcome.unwrap(), object);
                    }
                    assert_eq!(
                        vm.heap.root_count_for_test(),
                        roots,
                        "test={test} freeze={freeze} phase={phase}"
                    );
                    assert_eq!(vm.active_call_roots.len(), calls);
                    if let Some(weak) = weak {
                        vm.collect_now(&program);
                        assert!(
                            vm.heap.weak_value(weak).is_none(),
                            "test={test} freeze={freeze} phase={phase}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn reflect_definition_roots_release_after_callback_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in [
            "success",
            "proxy-success",
            "proxy-forward",
            "proxy-revoke-during-getter",
            "proxy-false",
            "proxy-getter-throw",
            "proxy-call-throw",
            "proxy-revoked",
            "descriptor-throw",
            "key-throw",
            "primitive-target",
            "invalid-get",
            "mixed",
        ] {
            let mut vm = Vm::new(Test262Host);
            let source = format!(
                r#"
                var target = '{phase}' === 'primitive-target' ? 1 : {{}};
                if ('{phase}' === 'proxy-revoke-during-getter') {{
                    var revocable = Proxy.revocable(target, {{get defineProperty() {{
                        revocable.revoke(); $262.gc(); return function(target, key, descriptor) {{$262.gc(); return true;}};
                    }}}}); target = revocable.proxy;
                }} else if ('{phase}'.startsWith('proxy-')) {{
                    if ('{phase}' === 'proxy-revoked') {{var revocable = Proxy.revocable(target, {{}}); target = revocable.proxy; revocable.revoke();}}
                    else target = new Proxy(target, {{get defineProperty() {{$262.gc();
                        if ('{phase}' === 'proxy-forward') return undefined;
                        if ('{phase}' === 'proxy-getter-throw') throw {{kind:'{phase}'}};
                        return function(target, key, descriptor) {{$262.gc();
                            if ('{phase}' === 'proxy-call-throw') throw {{kind:'{phase}'}};
                            return '{phase}' !== 'proxy-false';
                        }};
                    }}}});
                }}
                var key = {{[Symbol.toPrimitive]() {{$262.gc(); if ('{phase}' === 'key-throw') throw {{kind:'{phase}'}}; return Symbol('entry');}}}};
                var descriptor = {{get value() {{$262.gc(); if ('{phase}' === 'descriptor-throw') throw {{kind:'{phase}'}}; return {{rank:42}};}},
                    get writable() {{$262.gc(); return true;}}, configurable:true}};
                if ('{phase}' === 'invalid-get' || '{phase}' === 'mixed')
                    Object.defineProperty(descriptor, 'get', {{get() {{$262.gc(); return '{phase}' === 'invalid-get' ? 1 : undefined;}}}});
            "#
            );
            let program = compile(&source, "reflect-definition-roots.js").unwrap();
            vm.execute(&program).unwrap();
            let args = ["target", "key", "descriptor"].map(|name| {
                let atom = vm.intern_atom(name);
                vm.own_property(vm.realm.globals, atom).unwrap()
            });
            let roots = vm.heap.root_count_for_test();
            let calls = vm.active_call_roots.len();
            let result = vm.reflect_define_property(&program, &args);
            let succeeds = matches!(
                phase,
                "success"
                    | "proxy-success"
                    | "proxy-false"
                    | "proxy-forward"
                    | "proxy-revoke-during-getter"
            );
            assert_eq!(result.is_ok(), succeeds, "{phase}");

            if let Err(error) = &result {
                if phase.ends_with("-throw") {
                    let thrown = error.thrown_value().unwrap();
                    let atom = vm.intern_atom("kind");
                    let kind = vm.own_property(thrown, atom).unwrap();
                    assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                } else {
                    assert!(vm.format_error(&program, error).contains("TypeError"));
                }
            }
            if succeeds {
                assert_eq!(
                    result.unwrap(),
                    Vm::<Test262Host>::integrity_bool(phase != "proxy-false")
                );
            }
            assert_eq!(vm.heap.root_count_for_test(), roots);
            assert_eq!(vm.active_call_roots.len(), calls);
        }
    }
}

#[test]
fn from_entries_roots_release_after_iterator_and_entry_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in [
            "success",
            "next-getter",
            "next",
            "done",
            "value",
            "entry",
            "key",
            "entry-value",
            "convert",
        ] {
            let mut vm = Vm::new(Test262Host);
            let source = format!(
                r#"
                var source = {{[Symbol.iterator]() {{var index = 0; return {{
                    get next() {{$262.gc(); if ('{phase}' === 'next-getter') throw {{kind:'{phase}'}};
                        return function() {{$262.gc(); if ('{phase}' === 'next') throw {{kind:'{phase}'}};
                            if (index++) return {{done:true}};
                            return {{get done() {{$262.gc(); if ('{phase}' === 'done') throw {{kind:'{phase}'}}; return false;}},
                                get value() {{$262.gc(); if ('{phase}' === 'value') throw {{kind:'{phase}'}};
                                    if ('{phase}' === 'entry') return 1;
                                    return {{get 0() {{$262.gc(); if ('{phase}' === 'key') throw {{kind:'{phase}'}};
                                        return {{[Symbol.toPrimitive]() {{$262.gc(); if ('{phase}' === 'convert') throw {{kind:'{phase}'}}; return '__proto__';}}}};}},
                                        get 1() {{$262.gc(); if ('{phase}' === 'entry-value') throw {{kind:'{phase}'}}; return {{rank:42}};}}}};}}
                            }};
                        }};
                    }},
                    get return() {{$262.gc(); return function() {{$262.gc(); throw {{kind:'close'}};}};}}
                }};}}}};
            "#
            );
            let program = compile(&source, "from-entries-roots.js").unwrap();
            vm.execute(&program).unwrap();
            let atom = vm.intern_atom("source");
            let source = vm.own_property(vm.realm.globals, atom).unwrap();
            let roots = vm.heap.root_count_for_test();
            let calls = vm.active_call_roots.len();
            let result = vm.object_from_entries(&program, source);
            assert_eq!(result.is_err(), phase != "success", "{phase}");
            assert_eq!(vm.heap.root_count_for_test(), roots);
            assert_eq!(vm.active_call_roots.len(), calls);
            match result {
                Ok(result) => {
                    let weak = vm.heap.weak_handle(result).unwrap();
                    vm.collect_now(&program);
                    assert!(vm.heap.weak_value(weak).is_none());
                }
                Err(error) if phase != "entry" => {
                    let error = error.thrown_value().unwrap();
                    let atom = vm.intern_atom("kind");
                    let kind = vm.own_property(error, atom).unwrap();
                    assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                }
                Err(error) => assert!(vm.format_error(&program, &error).contains("TypeError")),
            }
        }
    }
}

#[test]
fn group_by_roots_release_after_iterator_and_callback_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for kind in [super::GroupByKind::Object, super::GroupByKind::Map] {
            for phase in [
                "success",
                "next-getter",
                "next",
                "done",
                "value",
                "callback",
                "key",
            ] {
                let mut vm = Vm::new(Test262Host);
                let source = format!(
                    r#"
                    var source = {{[Symbol.iterator]() {{var index = 0; return {{
                        get next() {{$262.gc(); if ('{phase}' === 'next-getter') throw {{kind:'next-getter'}}; return function() {{
                            $262.gc(); if ('{phase}' === 'next') throw {{kind:'next'}};
                            if (index++) return {{done:true}};
                            return {{get done() {{$262.gc(); if ('{phase}' === 'done') throw {{kind:'done'}}; return false;}},
                                get value() {{$262.gc(); if ('{phase}' === 'value') throw {{kind:'value'}}; return {{rank:42}};}}}};
                        }};}},
                        get return() {{$262.gc(); return function() {{$262.gc(); throw {{kind:'close'}};}};}}
                    }};}}}};
                    var callback = function(value) {{$262.gc(); if ('{phase}' === 'callback') throw {{kind:'callback'}};
                        return {{[Symbol.toPrimitive]() {{$262.gc(); if ('{phase}' === 'key') throw {{kind:'key'}}; return 'group';}}}};}};
                "#
                );
                let program = compile(&source, "group-by-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let source_atom = vm.intern_atom("source");
                let source = vm.own_property(vm.realm.globals, source_atom).unwrap();
                let callback_atom = vm.intern_atom("callback");
                let callback = vm.own_property(vm.realm.globals, callback_atom).unwrap();
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let result = vm.group_by(&program, &[source, callback], kind);
                let fails = phase != "success"
                    && !(phase == "key" && matches!(kind, super::GroupByKind::Map));
                assert_eq!(result.is_err(), fails, "{phase}");
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                if let Ok(result) = result {
                    let weak = vm.heap.weak_handle(result).unwrap();
                    vm.collect_now(&program);
                    assert!(vm.heap.weak_value(weak).is_none());
                } else if let Err(error) = result {
                    let error = error.thrown_value().unwrap();
                    let kind_atom = vm.intern_atom("kind");
                    let value = vm.own_property(error, kind_atom).unwrap();
                    assert_eq!(vm.to_string(&program, value).unwrap(), phase);
                }
            }
        }
    }
}

#[test]
fn dynamic_constructor_roots_release_after_prototype_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for native in [
            Native::Function,
            Native::AsyncFunction,
            Native::GeneratorFunction,
            Native::AsyncGeneratorFunction,
        ] {
            for (action, fails, intrinsic) in [
                ("return prototype", false, false),
                ("return null", false, true),
                ("throw new Error('prototype')", true, false),
            ] {
                let mut vm = Vm::new(Test262Host);
                let program = compile(&format!("var prototype = {{rank:42}}; var target = new Proxy(function Target() {{}}, {{get(object, key, receiver) {{if (key === 'prototype') {{$262.gc(); {action}}} return Reflect.get(object, key, receiver);}}}});"), "dynamic-constructor-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let target_atom = vm.intern_atom("target");
                let target = vm.own_property(vm.realm.globals, target_atom).unwrap();
                let prototype_atom = vm.intern_atom("prototype");
                let prototype = if intrinsic {
                    vm.realm.intrinsics.builtin_prototypes[&(vm.realm.globals, native)]
                } else {
                    vm.own_property(vm.realm.globals, prototype_atom).unwrap()
                };
                let argument = vm.heap.alloc(super::Cell::String("return 42".into()));
                let weak_argument = vm.heap.weak_handle(argument).unwrap();
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let result = vm.construct_value_with_new_target(
                    &program,
                    vm.native_value(native),
                    target,
                    &[argument],
                );
                assert_eq!(result.is_err(), fails);
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                let weak_result = result.ok().map(|result| {
                    assert!(vm.is_function(result));
                    assert_eq!(vm.object_data(result).unwrap().proto, prototype);
                    vm.heap.weak_handle(result).unwrap()
                });
                vm.collect_now(&program);
                assert!(vm.heap.weak_value(weak_argument).is_none());
                if let Some(result) = weak_result {
                    assert!(vm.heap.weak_value(result).is_none());
                }
            }
        }
    }
}

#[test]
fn error_constructor_and_call_scopes_release_after_coercion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for native in [
            Native::Error,
            Native::EvalError,
            Native::RangeError,
            Native::ReferenceError,
            Native::SyntaxError,
            Native::TypeError,
            Native::URIError,
            Native::SuppressedError,
        ] {
            for (setup, fails) in [
                (
                    "var message = {toString() {$262.gc(); return 'message'}}; var options = {get cause() {$262.gc(); return {rank: 7}}};",
                    false,
                ),
                (
                    "var message = {toString() {$262.gc(); throw new Error('message')}}; var options;",
                    true,
                ),
                (
                    "var message; var options = {get cause() {$262.gc(); throw new Error('cause')}};",
                    true,
                ),
            ] {
                let mut vm = Vm::new(Test262Host);
                let program = compile(setup, "error-root-scope.js").unwrap();
                vm.execute(&program).unwrap();
                let message_atom = vm.intern_atom("message");
                let message = vm.own_property(vm.realm.globals, message_atom).unwrap();
                let options_atom = vm.intern_atom("options");
                let options = vm.own_property(vm.realm.globals, options_atom).unwrap();
                let args = if native == Native::SuppressedError {
                    vec![Value::UNDEFINED, Value::UNDEFINED, message]
                } else {
                    vec![message, options]
                };
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let result = vm.construct_error_native(&program, native, &args);
                assert_eq!(
                    result.is_err(),
                    fails && (native != Native::SuppressedError || !message.is_undefined())
                );
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                if let Ok(value) = result {
                    let handle = vm.heap.weak_handle(value).unwrap();
                    vm.collect_now(&program);
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
        for fails in [false, true] {
            let mut vm = Vm::new(Test262Host);
            let action = if fails {
                "throw new Error('prototype')"
            } else {
                "return {}"
            };
            let program = compile(&format!("var constructor = new Proxy(function C(value) {{$262.gc(); this.answer = value.answer;}}, {{get(target, key, receiver) {{if (key === 'prototype') {{$262.gc(); {action}}} return Reflect.get(target, key, receiver);}}}});"), "construct-scope.js").unwrap();
            vm.execute(&program).unwrap();
            let atom = vm.intern_atom("constructor");
            let constructor = vm.own_property(vm.realm.globals, atom).unwrap();
            let argument = vm.object();
            vm.set_named(&program, argument, "answer", Value::number(42.0))
                .unwrap();
            let weak = vm.heap.weak_handle(argument).unwrap();
            let roots = vm.heap.root_count_for_test();
            let calls = vm.active_call_roots.len();
            let result = vm.construct_value(&program, constructor, &[argument]);
            assert_eq!(result.is_err(), fails);
            assert_eq!(vm.heap.root_count_for_test(), roots);
            assert_eq!(vm.active_call_roots.len(), calls);
            if let Ok(result) = result {
                let atom = vm.intern_atom("answer");
                assert_eq!(vm.own_property(result, atom), Some(Value::number(42.0)));
            }
            vm.collect_now(&program);
            assert!(vm.heap.weak_value(weak).is_none());
        }
    }
}

#[test]
fn spread_and_aggregate_roots_release_after_guest_failures() {
    let cases = [
        (
            "var source = [{rank: 1}]; var message; var options;",
            false,
            false,
        ),
        (
            "var source = {[Symbol.iterator]() {return {get next() {$262.gc(); throw new Error('next')}}}}; var message; var options;",
            true,
            true,
        ),
        (
            "var source = {[Symbol.iterator]() {return {next() {$262.gc(); throw new Error('next')}}}}; var message; var options;",
            true,
            true,
        ),
        (
            "var source = {[Symbol.iterator]() {return {next() {return {get done() {$262.gc(); throw new Error('done')}}}}}}; var message; var options;",
            true,
            true,
        ),
        (
            "var source = {[Symbol.iterator]() {return {next() {return {done: false, get value() {$262.gc(); throw new Error('value')}}}}}}; var message; var options;",
            true,
            true,
        ),
        (
            "var source = [{rank: 1}]; var message = {toString() {$262.gc(); throw new Error('message')}}; var options;",
            false,
            true,
        ),
        (
            "var source = [{rank: 1}]; var message; var options = new Proxy({}, {has() {$262.gc(); throw new Error('has')}});",
            false,
            true,
        ),
        (
            "var source = [{rank: 1}]; var message; var options = {get cause() {$262.gc(); throw new Error('cause')}};",
            false,
            true,
        ),
    ];
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (setup, spread_fails, aggregate_fails) in cases {
            for aggregate in [false, true] {
                let mut vm = Vm::new(Test262Host);
                let program = compile(setup, "spread-aggregate-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let mut args = Vec::new();
                for name in ["source", "message", "options", "AggregateError"] {
                    let atom = vm.intern_atom(name);
                    args.push(vm.own_property(vm.realm.globals, atom).unwrap());
                }
                let roots = vm.heap.root_count_for_test();
                let result = if aggregate {
                    vm.construct_aggregate_error(&program, &args[..3], args[3])
                } else {
                    vm.spread_to_array(&program, args[0])
                };
                assert_eq!(
                    result.is_err(),
                    if aggregate {
                        aggregate_fails
                    } else {
                        spread_fails
                    }
                );
                assert_eq!(vm.heap.root_count_for_test(), roots);
                if let Ok(value) = result {
                    let handle = vm.heap.weak_handle(value).unwrap();
                    vm.collect_now(&program);
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn typed_initialization_roots_release_after_iterator_and_conversion_errors() {
    let sources = [
        ("({length: Number.MAX_SAFE_INTEGER + 1})", true),
        (
            "({length: 2, 0: {valueOf() {$262.gc(); return '7'}}, 1: '8'})",
            false,
        ),
        (
            "({length: 2, 0: {valueOf() {$262.gc(); throw new Error('convert')}}, 1: '8'})",
            true,
        ),
        (
            "({[Symbol.iterator]() {let index = 0; return {get next() {$262.gc(); return function() {return index++ ? {done: true} : {value: {valueOf() {$262.gc(); return '7'}}, done: false}}}}}})",
            false,
        ),
        (
            "({[Symbol.iterator]() {return {next() {$262.gc(); throw new Error('next')}}}})",
            true,
        ),
        (
            "({[Symbol.iterator]() {return {next() {return {get done() {$262.gc(); throw new Error('done')}}}}}})",
            true,
        ),
        (
            "({[Symbol.iterator]() {return {next() {return {done: false, get value() {$262.gc(); throw new Error('value')}}}}}})",
            true,
        ),
        (
            "({[Symbol.iterator]() {let index = 0; return {next() {return index++ ? {done: true} : {value: {valueOf() {$262.gc(); throw new Error('convert')}}, done: false}}}}})",
            true,
        ),
    ];
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (name, kind) in [
            ("Uint8Array", TypedArrayKind::Uint8),
            ("BigInt64Array", TypedArrayKind::BigInt64),
            ("BigUint64Array", TypedArrayKind::BigUint64),
        ] {
            for (source, fails) in sources {
                for from in [false, true] {
                    let mut vm = Vm::new(Test262Host);
                    let program = compile(
                        &format!("var source = {source};"),
                        "typed-initialization-roots.js",
                    )
                    .unwrap();
                    vm.execute(&program).unwrap();
                    let atom = vm.intern_atom("source");
                    let source = vm.own_property(vm.realm.globals, atom).unwrap();
                    let atom = vm.intern_atom(name);
                    let constructor = vm.own_property(vm.realm.globals, atom).unwrap();
                    let roots = vm.heap.root_count_for_test();
                    let outcome = if from {
                        vm.array_modern_native(
                            &program,
                            Native::TypedArrayFrom,
                            constructor,
                            &[source],
                        )
                    } else {
                        vm.construct_typed_array_native(&program, &[source], kind, name)
                    };
                    assert_eq!(outcome.is_err(), fails, "{name}, from={from}, {source:?}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    if let Ok(result) = outcome {
                        let handle = vm.heap.weak_handle(result).unwrap();
                        vm.collect_now(&program);
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn typed_fill_and_assignment_roots_release_after_coercion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for constructor in ["Uint8Array", "BigInt64Array", "BigUint64Array"] {
            let element = if constructor == "Uint8Array" {
                "7"
            } else {
                "7n"
            };
            for (setup, fails) in [
                (format!("var value = {element}; var start = 0; var end = undefined;"), false),
                ("var value = {valueOf() {$262.gc(); throw new Error('value')}}; var start = 0; var end = undefined;".into(), true),
                (format!("var value = {element}; var start = {{valueOf() {{$262.gc(); throw new Error('start')}}}}; var end = undefined;"), true),
                (format!("var value = {element}; var start = 0; var end = {{valueOf() {{$262.gc(); throw new Error('end')}}}};"), true),
                (format!("var value = {element}; var start = {{valueOf() {{$262.detachArrayBuffer(source.buffer); $262.gc(); return 0}}}}; var end = undefined;"), true),
                (format!("var value = {element}; var start = 0; var end = {{valueOf() {{$262.detachArrayBuffer(source.buffer); $262.gc(); return 1}}}};"), true),
            ] {
                let mut vm = Vm::new(Test262Host);
                let program = compile(&format!("var source = new {constructor}(3); {setup}"), "typed-fill-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let mut values = Vec::new();
                for name in ["source", "value", "start", "end"] {
                    let atom = vm.intern_atom(name);
                    values.push(vm.own_property(vm.realm.globals, atom).unwrap());
                }
                let roots = vm.heap.root_count_for_test();
                let result = vm.typed_array_native(&program, Native::Uint8ArrayFill, values[0], &values[1..]);
                assert_eq!(result.is_err(), fails, "{constructor}: {setup}");
                assert_eq!(vm.heap.root_count_for_test(), roots);
            }
            for fails in [false, true] {
                let mut vm = Vm::new(Test262Host);
                let coercion = if fails {
                    "throw new Error('value')".to_owned()
                } else {
                    format!("return {element}")
                };
                let program = compile(&format!("var source = new {constructor}(1); var value = {{valueOf() {{$262.gc(); {coercion}}}}};"), "typed-set-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let source_atom = vm.intern_atom("source");
                let source = vm.own_property(vm.realm.globals, source_atom).unwrap();
                let value_atom = vm.intern_atom("value");
                let value = vm.own_property(vm.realm.globals, value_atom).unwrap();
                let roots = vm.heap.root_count_for_test();
                let result = vm.typed_array_set(&program, source, 0, value);
                assert_eq!(result.is_err(), fails);
                assert_eq!(vm.heap.root_count_for_test(), roots);
            }
        }
    }
}

#[test]
fn typed_sort_snapshot_roots_release_after_comparator_errors() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for constructor in ["Uint8Array", "BigInt64Array", "BigUint64Array"] {
            for (comparator, fails) in [
                ("undefined", false),
                (
                    "(left, right) => left < right ? -1 : left > right ? 1 : 0",
                    false,
                ),
                ("() => {throw new Error('compare')}", true),
                ("() => ({valueOf() {throw new Error('coercion')}})", true),
            ] {
                for native in [
                    Native::TypedArraySort,
                    Native::TypedArrayToSorted,
                    Native::TypedArrayToReversed,
                ] {
                    let mut vm = Vm::new(SilentHost);
                    let values = if constructor == "Uint8Array" {
                        "[3, 1, 2]"
                    } else {
                        "[3n, 1n, 2n]"
                    };
                    let source = format!(
                        "var source = new {constructor}({values}); var comparator = {comparator};"
                    );
                    let program = compile(&source, "typed-sort-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let source_atom = vm.intern_atom("source");
                    let source = vm.own_property(vm.realm.globals, source_atom).unwrap();
                    let comparator_atom = vm.intern_atom("comparator");
                    let comparator = vm.own_property(vm.realm.globals, comparator_atom).unwrap();
                    let roots_before = vm.heap.root_count_for_test();
                    let outcome = vm.typed_array_native(&program, native, source, &[comparator]);
                    assert_eq!(
                        outcome.is_err(),
                        fails && native != Native::TypedArrayToReversed
                    );
                    assert_eq!(vm.heap.root_count_for_test(), roots_before);
                    if native != Native::TypedArraySort
                        && let Ok(result) = outcome
                    {
                        let handle = vm.heap.weak_handle(result).unwrap();
                        vm.collect_now(&program);
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn sort_snapshot_roots_release_after_guest_failures_and_writeback() {
    let cases = [
        ("var source = [3, 1, 2]; var comparator = (left, right) => left - right;", false, false),
        ("var source = null; var comparator = (left, right) => left - right;", true, true),
        ("var source = [3, 1, 2]; var comparator = undefined; \
          Object.defineProperty(source, '1', {get() {throw new Error('getter')}});", true, true),
        ("var source = [3, 1, 2]; var comparator = () => {throw new Error('compare')};", true, true),
        ("var source = [3, 1, 2]; var comparator = () => \
          ({valueOf() {throw new Error('coercion')}});", true, true),
        ("var source = [{toString() {throw new Error('string')}}, {}]; var comparator = undefined;", true, true),
        ("var source = [3, 1, 2]; var comparator = undefined; \
          Object.defineProperty(source, '0', {get() {return 3}, set() {throw new Error('setter')}});", true, false),
        ("var source = [3, , 1]; var comparator = undefined; \
          Object.defineProperty(source, '2', {configurable: false});", true, false),
    ];
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (source, sort_fails, copy_fails) in cases {
            for (native, fails) in [(Native::ArraySort, sort_fails), (Native::ArrayToSorted, copy_fails)] {
                let mut vm = Vm::new(SilentHost);
                let program = compile(source, "sort-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let source_atom = vm.intern_atom("source");
                let source = vm.own_property(vm.realm.globals, source_atom).unwrap();
                let comparator_atom = vm.intern_atom("comparator");
                let comparator = vm.own_property(vm.realm.globals, comparator_atom).unwrap();
                let roots_before = vm.heap.root_count_for_test();
                let outcome = vm.array_modern_native(&program, native, source, &[comparator]);
                assert_eq!(outcome.is_err(), fails);
                assert_eq!(vm.heap.root_count_for_test(), roots_before);
                if native == Native::ArrayToSorted && let Ok(result) = outcome {
                    let handle = vm.heap.weak_handle(result).unwrap();
                    vm.collect_now(&program);
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn array_copy_roots_release_after_success_getters_and_coercion_errors() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (source, getters_fail, coercion_fails) in [
            ("var source = [1, 2, 3]; var index = 1;", false, false),
            ("var source = [1, 2, 3]; var index = 1; \
              for (var key of [0, 1, 2]) Object.defineProperty(source, key, \
              {get() {throw new Error('getter')}});", true, false),
            ("var source = [1, 2, 3]; var index = {valueOf() {throw new Error('coercion')}};", false, true),
        ] {
            for native in [Native::ArrayWith, Native::ArrayToReversed, Native::ArrayToSpliced] {
                let mut vm = Vm::new(SilentHost);
                let program = compile(source, "copy-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let source_atom = vm.intern_atom("source");
                let source = vm.own_property(vm.realm.globals, source_atom).unwrap();
                let index_atom = vm.intern_atom("index");
                let index = vm.own_property(vm.realm.globals, index_atom).unwrap();
                let roots_before = vm.heap.root_count_for_test();
                let outcome = vm.array_copy_native(&program, native, source, &[index, Value::number(1.0)]);
                assert_eq!(outcome.is_err(), getters_fail || (coercion_fails && native != Native::ArrayToReversed));
                assert_eq!(vm.heap.root_count_for_test(), roots_before);
                if let Ok(result) = outcome {
                    let handle = vm.heap.weak_handle(result).unwrap();
                    vm.collect_now(&program);
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn reduction_roots_release_on_success_and_abrupt_completion() {
    let cases = [
        ("var source = [1, 2]; var callback = (accumulator, value) => ({sum: 42});", false, false),
        ("var source = [1, 2]; var callback = () => {throw new Error('callback')};", true, true),
        ("var source = [1, 2]; var callback = () => ({sum: 42}); \
          for (var index of [0, 1]) Object.defineProperty(source, index, \
          {get() {throw new Error('getter')}});", true, true),
        ("var source = new Proxy([1, 2], {has() {throw new Error('has')}}); \
          var callback = () => ({sum: 42});", true, true),
        ("var source = []; var callback = () => ({sum: 42});", true, false),
    ];
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for native in [Native::ArrayReduce, Native::ArrayReduceRight] {
            for (source, fails_without_initial, fails_with_initial) in cases {
                for (initial, fails) in [(false, fails_without_initial), (true, fails_with_initial)] {
                    let mut vm = Vm::new(SilentHost);
                    let program = compile(source, "reduce-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let source_atom = vm.intern_atom("source");
                    let source = vm.own_property(vm.realm.globals, source_atom).unwrap();
                    let callback_atom = vm.intern_atom("callback");
                    let callback = vm.own_property(vm.realm.globals, callback_atom).unwrap();
                    let mut args = vec![callback];
                    if initial { args.push(Value::UNDEFINED); }
                    let roots_before = vm.heap.root_count_for_test();
                    let outcome = vm.array_reduce_native(&program, native, source, &args);
                    assert_eq!(outcome.is_err(), fails);
                    assert_eq!(vm.heap.root_count_for_test(), roots_before);
                    if let Ok(result) = outcome {
                        if let Some(handle) = vm.heap.weak_handle(result) {
                            vm.collect_now(&program);
                            assert!(vm.heap.weak_value(handle).is_none());
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn flattening_roots_release_after_success_and_abrupt_completion() {
    let cases = [
        ("var source = [1]; var mapper = value => [value];", false, false),
        ("var source = [1]; var mapper = value => [value]; \
          Object.defineProperty(source, '0', {get() {throw new Error('source')}});", true, true),
        ("var nested = [1]; Object.defineProperty(nested, '0', \
          {get() {throw new Error('nested')}}); var source = [nested]; var mapper = value => value;", true, true),
        ("var source = [1]; var mapper = value => [value]; \
          source.constructor = {[Symbol.species]: function() { \
          return new Proxy({}, {defineProperty() {return false}})}};", true, true),
        ("var source = [1]; var mapper = value => {throw new Error('mapper')};", false, true),
    ];
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (source, flat_fails, flat_map_fails) in cases {
            for (native, fails) in [
                (Native::ArrayFlat, flat_fails),
                (Native::ArrayFlatMap, flat_map_fails),
            ] {
                let mut vm = Vm::new(SilentHost);
                let program = compile(source, "flatten-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let source_atom = vm.intern_atom("source");
                let source = vm.own_property(vm.realm.globals, source_atom).unwrap();
                let mapper_atom = vm.intern_atom("mapper");
                let mapper = vm.own_property(vm.realm.globals, mapper_atom).unwrap();
                let args = if native == Native::ArrayFlatMap { vec![mapper] } else { vec![] };
                let roots_before = vm.heap.root_count_for_test();
                let outcome = vm.array_flatten_native(&program, native, source, &args);
                assert_eq!(outcome.is_err(), fails);
                assert_eq!(vm.heap.root_count_for_test(), roots_before);
                if let Ok(result) = outcome {
                    let handle = vm.heap.weak_handle(result).unwrap();
                    vm.collect_now(&program);
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn flattening_uses_rooted_frames_instead_of_native_recursion() {
    const NESTING_DEPTH: usize = 4096;
    let mut vm = Vm::new(SilentHost);
    let program = Engine::specialize("", "deep-flatten.js").unwrap();
    vm.initialize(&program).unwrap();
    let mut nested = Value::number(42.0);
    for _ in 0..NESTING_DEPTH {
        nested = vm.new_array(vec![nested]);
    }
    let roots_before = vm.heap.root_count_for_test();
    let result = vm.array_flatten_native(
        &program, Native::ArrayFlat, nested, &[Value::number(f64::INFINITY)],
    ).unwrap();
    assert_eq!(vm.array_value_at(result, 0), Value::number(42.0));
    assert_eq!(vm.heap.root_count_for_test(), roots_before);
}

#[test]
fn bound_function_metadata_roots_release_on_normal_and_abrupt_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (metadata, fails) in [(None, false), (Some("length"), true), (Some("name"), true)] {
            let mut vm = Vm::new(SilentHost);
            let source = match metadata {
                None => "function target() {}".to_owned(),
                Some(property) => format!(
                    "function target() {{}} Object.defineProperty(target, '{property}', \
                     {{get() {{throw new Error('{property}')}}}});"
                ),
            };
            let program = compile(&source, "bound-metadata-roots.js").unwrap();
            vm.execute(&program).unwrap();
            let atom = vm.intern_atom("target");
            let target = vm.own_property(vm.realm.globals, atom).unwrap();
            let roots_before = vm.heap.root_count_for_test();
            let outcome = vm.bind_function(&program, target, &[]);
            assert_eq!(outcome.is_err(), fails, "{metadata:?}");
            assert_eq!(vm.heap.root_count_for_test(), roots_before, "{metadata:?}");
            if let Ok(function) = outcome {
                let handle = vm.heap.weak_handle(function).unwrap();
                vm.collect_now(&program);
                assert!(vm.heap.weak_value(handle).is_none());
            }
        }
    }
}

#[test]
fn closure_identity_cache_prunes_collected_cells_and_keeps_rooted_cells() {
    let mut vm = Vm::new(SilentHost);
    let program = Engine::specialize("function kept() {}", "closure-cache-gc.js").unwrap();
    vm.initialize(&program).unwrap();
    let dead = vm.closure(&program, 0, Value::NULL).unwrap();
    let kept = vm.closure(&program, 1, Value::NULL).unwrap();
    let root = vm.root(kept);
    vm.collect_now(&program);
    assert!(vm.heap.get(dead).is_none());
    assert!(vm.heap.get(kept).is_some());
    assert!(
        !vm.function_values
            .contains_key(&(super::program_store::ProgramId::MAIN, 0))
    );
    assert!(
        vm.function_values
            .contains_key(&(super::program_store::ProgramId::MAIN, 1))
    );
    assert!(vm.release_root(root));
    vm.collect_now(&program);
    assert!(
        !vm.function_values
            .contains_key(&(super::program_store::ProgramId::MAIN, 1))
    );
}

#[test]
fn closure_identity_cache_rejects_reused_function_slots() {
    let mut vm = Vm::new(SilentHost);
    let program = Engine::specialize("function kept() {}", "closure-cache-reuse.js").unwrap();
    vm.initialize(&program).unwrap();
    let old = vm.closure(&program, 1, Value::NULL).unwrap();
    let key = (super::program_store::ProgramId::MAIN, 1);
    let allocation_bound = vm.heap.stats().2 + 1;
    // Bypass cache pruning to prove lookup itself checks the heap generation.
    vm.heap.collect([]);
    let mut reused = false;
    for _ in 0..allocation_bound {
        let replacement = vm.heap.alloc(crate::heap::Cell::Function {
            object: Box::new(Vm::<SilentHost>::empty_object(Value::NULL)),
            kind: crate::heap::FunctionKind::User(key.0, key.1),
            env: Value::NULL,
            realm: Value::NULL,
        });
        if replacement == old {
            reused = true;
            break;
        }
    }
    assert!(reused, "the collected function slot must be reused");
    assert!(
        vm.cached_functions_in_environment(key.0, key.1, Value::NULL)
            .next()
            .is_none()
    );
    vm.prune_function_values();
    assert!(!vm.function_values.contains_key(&key));
}

#[test]
fn pending_jobs_use_the_shared_interpreter_after_root_release() {
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(RecordingHost(output.clone()));
    let program = Engine::specialize("print(0);", "job.js").unwrap();
    vm.initialize(&program).unwrap();
    let callback = vm.native_value(crate::heap::Native::Print);
    let callback_root = vm.root(callback);
    let argument_root = vm.root(Value::number(7.0));
    let argument = vm.root_value(argument_root).unwrap();
    vm.enqueue_job(callback, vec![argument]);
    assert!(vm.release_root(callback_root));
    assert!(vm.release_root(argument_root));
    vm.drain_jobs(&program).unwrap();
    assert_eq!(output.borrow().as_slice(), ["7"]);
}

#[test]
fn suspended_continuations_are_rooted_until_generation_checked_resume() {
    let mut vm = Vm::new(SilentHost);
    let program = Engine::specialize("print(0);", "continuation.js").unwrap();
    vm.initialize(&program).unwrap();
    let live = vm.heap.alloc(crate::heap::Cell::Environment {
        parent: Value::NULL,
        program: None,
        root_eval_scope: false,
        function: u32::MAX,
        slots: Box::new([]),
        dynamic_bindings: vec![],
        with_objects: vec![],
    });
    let held = vm.heap.alloc(crate::heap::Cell::Error("suspended binding".into()));
    let held_atom = vm.intern_atom("held");
    let id = vm.suspend_continuation(Continuation {
        program: super::program_store::ProgramId::MAIN,
        active_iterators: vec![],
        function: 0,
        pc: 0,
        env: live,
        this: Value::UNDEFINED,
        locals: vec![],
        dynamic_bindings: vec![(held_atom, held)],
        registers: vec![],
        completion: Completion::Yield(Value::UNDEFINED),
        captured: false,
        resume_register: None,
        promise: Value::UNDEFINED,
    });
    vm.collect_now(&program);
    assert!(vm.heap.get(live).is_some());
    assert!(vm.heap.get(held).is_some());
    assert!(vm.resume_continuation(id).is_some());
    assert!(vm.resume_continuation(id).is_none());
    vm.collect_now(&program);
    assert!(vm.heap.get(live).is_none());
    assert!(vm.heap.get(held).is_none());
}

#[test]
fn exhausted_continuation_generations_retire_slots_without_resumer_aliasing() {
    let mut vm = Vm::new(SilentHost);
    let program = Engine::specialize("print(0);", "continuation-generation.js").unwrap();
    vm.initialize(&program).unwrap();
    let continuation = || Continuation {
        program: super::program_store::ProgramId::MAIN,
        function: 0,
        pc: 0,
        env: Value::NULL,
        this: Value::UNDEFINED,
        locals: vec![],
        dynamic_bindings: vec![],
        registers: vec![],
        active_iterators: vec![],
        completion: Completion::Yield(Value::UNDEFINED),
        captured: false,
        resume_register: None,
        promise: Value::UNDEFINED,
    };
    let initial = vm.suspend_continuation(continuation());
    vm.suspended[initial.slot as usize].generation = u32::MAX;
    let final_generation = super::activation::ContinuationId {
        slot: initial.slot,
        generation: u32::MAX,
    };
    assert!(vm.resume_continuation(final_generation).is_some());

    let replacement = vm.suspend_continuation(continuation());
    assert_ne!(replacement.slot, final_generation.slot);
    assert!(vm.resume_continuation(final_generation).is_none());
    assert!(vm.resume_continuation(replacement).is_some());
}

#[test]
fn pooled_frame_registers_are_reset_when_their_length_is_reused() {
    let mut frame = super::Frame {
        program: super::program_store::ProgramId::MAIN,
        function: 0,
        pc: 0,
        env: Value::NULL,
        this: Value::UNDEFINED,
        locals: vec![],
        dynamic_bindings: vec![],
        captured: false,
        registers: vec![Value::heap(11), Value::heap(12)],
        active_iterators: vec![],
        with_base: 0,
    };

    frame.prepare_registers(1);
    assert_eq!(frame.registers, [Value::UNDEFINED]);

    frame.registers[0] = Value::heap(13);
    frame.prepare_registers(3);
    assert_eq!(
        frame.registers,
        [Value::UNDEFINED, Value::UNDEFINED, Value::UNDEFINED]
    );
}

#[test]
fn regression_collection_preserves_unused_regexp_iterator_prototype() {
    for (mode, compile) in [
        ("specialized", Engine::specialize as fn(&str, &str) -> _),
        ("unspecialized", Engine::specialize_unspecialized),
    ] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(RecordingHost(output.clone()));
        let program = compile(
            "function later() { print((/a/g)[Symbol.matchAll]('a').next().value[0]); }",
            "regexp-after-collection.js",
        )
        .unwrap();
        vm.execute(&program).unwrap();
        let atom = vm.intern_atom("later");
        let callback = vm.own_property(vm.realm.globals, atom).unwrap();
        let root = vm.root(callback);
        vm.collect_now(&program);
        vm.enqueue_job(vm.root_value(root).unwrap(), vec![]);
        if let Err(error) = vm.drain_jobs(&program) {
            panic!("{mode}: {}", vm.format_error(&program, &error));
        }
        assert_eq!(output.borrow().as_slice(), ["a"], "{mode}");
        assert!(vm.release_root(root));
    }
}

#[test]
fn error_data_survives_collection_without_retaining_dead_errors() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for kind in [
            Native::Error,
            Native::TypeError,
            Native::AggregateError,
            Native::SuppressedError,
        ] {
            let mut vm = Vm::new(SilentHost);
            let program = compile("", "error-data-roots.js").unwrap();
            vm.initialize(&program).unwrap();
            let roots = vm.heap.root_count_for_test();
            let error = vm.construct_error_native(&program, kind, &[]).unwrap();
            assert!(vm.error_is_error(error));
            let root = vm.heap.root(error);
            let weak = vm.heap.weak_handle(error).unwrap();
            vm.collect_now(&program);
            let error = vm.heap.root_value(root).unwrap();
            assert!(vm.error_is_error(error));
            let clone = vm
                .heap
                .alloc(super::Cell::Object(vm.object_data(error).unwrap().clone()));
            assert!(vm.error_is_error(clone));
            vm.heap.release_root(root);
            vm.collect_now(&program);
            assert!(vm.heap.weak_value(weak).is_none());
            assert_eq!(vm.heap.root_count_for_test(), roots);
            let plain = vm.object();
            assert!(!vm.error_is_error(plain));
        }
        let mut vm = Vm::new(SilentHost);
        let program = compile("", "error-data-internal.js").unwrap();
        vm.initialize(&program).unwrap();
        let exhausted = vm.stack_exhaustion_error().thrown_value().unwrap();
        assert!(vm.error_is_error(exhausted));
        let aggregate = vm.aggregate_error(vec![]).unwrap();
        assert!(vm.error_is_error(aggregate));
    }
}

#[test]
fn aggregate_error_intrinsics_and_reason_roots_follow_the_active_realm() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for foreign in [false, true] {
            let mut vm = Vm::new(Test262Host);
            let program = compile(
                "var foreign = $262.createRealm().global;",
                "aggregate-intrinsics-roots.js",
            )
            .unwrap();
            vm.execute(&program).unwrap();
            if foreign {
                let atom = vm.intern_atom("foreign");
                let global = vm.own_property(vm.realm.globals, atom).unwrap();
                vm.switch_realm_global(global);
            }
            let global = vm.realm.globals;
            let error_prototype =
                vm.realm.intrinsics.builtin_prototypes[&(global, Native::AggregateError)];
            let array_prototype = vm.array_prototype_for_realm(global);
            let promise_prototype =
                vm.realm.intrinsics.builtin_prototypes[&(global, Native::Promise)];
            for name in ["AggregateError", "Array", "Promise"] {
                let atom = vm.intern_atom(name);
                vm.set_property(global, atom, Value::NULL).unwrap();
            }
            vm.collect_now(&program);
            assert!(vm.heap.get(promise_prototype).is_some());
            let roots = vm.heap.root_count_for_test();
            let reason = vm.object();
            let reason_weak = vm.heap.weak_handle(reason).unwrap();
            let error = vm.aggregate_error(vec![reason]).unwrap();
            assert!(vm.error_is_error(error));
            assert_eq!(vm.object_data(error).unwrap().proto, error_prototype);
            let errors_atom = vm.intern_atom("errors");
            let errors = vm.own_property(error, errors_atom).unwrap();
            assert_eq!(vm.object_data(errors).unwrap().proto, array_prototype);
            let error_weak = vm.heap.weak_handle(error).unwrap();
            let errors_weak = vm.heap.weak_handle(errors).unwrap();
            let root = vm.heap.root(error);
            vm.collect_now(&program);
            assert!(vm.heap.weak_value(reason_weak).is_some());
            assert!(vm.heap.weak_value(errors_weak).is_some());
            vm.heap.release_root(root);
            vm.collect_now(&program);
            assert!(vm.heap.weak_value(reason_weak).is_none());
            assert!(vm.heap.weak_value(errors_weak).is_none());
            assert!(vm.heap.weak_value(error_weak).is_none());
            assert_eq!(vm.heap.root_count_for_test(), roots);
        }
    }
}

#[test]
fn promise_finally_restores_roots_after_each_effect_completion() {
    let cases = [
        (
            "({get constructor() {$262.gc(); throw 'constructor';}})",
            true,
        ),
        ("({constructor: {[Symbol.species]: () => {}}})", true),
        (
            "({constructor: undefined, get then() {$262.gc(); throw 'then';}})",
            true,
        ),
        (
            "({constructor: undefined, get then() {$262.gc(); return function() {$262.gc(); throw 'call';};}})",
            true,
        ),
        (
            "({constructor: undefined, get then() {$262.gc(); return function(a,b) {$262.gc(); return a === b ? 42 : 0;};}})",
            false,
        ),
    ];
    let callable_case = "({constructor: undefined, get then() {$262.gc(); throw 'then';}})";
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (source, fails, callable) in cases
            .into_iter()
            .map(|(source, fails)| (source, fails, false))
            .chain(std::iter::once((callable_case, true, true)))
        {
            let mut vm = Vm::new(Test262Host);
            let program = compile(
                &format!("function create() {{return {source};}} function createHandler() {{return function() {{}};}}"),
                "finally-roots.js",
            )
            .unwrap();
            vm.execute(&program).unwrap();
            let atom = vm.intern_atom("create");
            let factory = vm.own_property(vm.realm.globals, atom).unwrap();
            let receiver = vm
                .call_value(&program, factory, Value::UNDEFINED, &[])
                .unwrap();
            let handler = if callable {
                let atom = vm.intern_atom("createHandler");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                vm.call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap()
            } else {
                vm.object()
            };
            let receiver_weak = vm.heap.weak_handle(receiver).unwrap();
            let handler_weak = vm.heap.weak_handle(handler).unwrap();
            let roots = vm.heap.root_count_for_test();
            let incoming = vm.active_call_roots.len();
            let result = vm.call_native(&program, Native::PromiseFinally, receiver, &[handler]);
            assert_eq!(result.is_err(), fails, "{source}");
            if !fails {
                assert_eq!(result.unwrap(), Value::number(42.0));
            }
            assert_eq!(vm.heap.root_count_for_test(), roots, "{source}");
            assert_eq!(vm.active_call_roots.len(), incoming, "{source}");
            vm.collect_now(&program);
            assert!(vm.heap.weak_value(receiver_weak).is_none(), "{source}");
            assert!(vm.heap.weak_value(handler_weak).is_none(), "{source}");
        }
    }
}

#[test]
fn promise_constructor_intrinsics_survive_guest_binding_and_prototype_mutation() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        let mut vm = Vm::new(Test262Host);
        let program = compile(
            "var foreign = $262.createRealm().global; var invalid = () => {};",
            "promise-intrinsic-roots.js",
        )
        .unwrap();
        vm.execute(&program).unwrap();
        let invalid_atom = vm.intern_atom("invalid");
        let invalid = vm.own_property(vm.realm.globals, invalid_atom).unwrap();
        let foreign_atom = vm.intern_atom("foreign");
        let foreign = vm.own_property(vm.realm.globals, foreign_atom).unwrap();
        vm.switch_realm_global(foreign);
        let constructor = vm.realm.intrinsics.promise_constructors[&foreign];
        let weak = vm.heap.weak_handle(constructor).unwrap();
        let prototype = vm.realm.intrinsics.builtin_prototypes[&(foreign, Native::Promise)];
        let promise_atom = vm.intern_atom("Promise");
        vm.set_property(foreign, promise_atom, Value::NULL).unwrap();
        let constructor_atom = vm.intern_atom("constructor");
        vm.set_property(prototype, constructor_atom, Value::NULL)
            .unwrap();
        vm.collect_now(&program);
        assert_eq!(vm.heap.weak_value(weak), Some(constructor));
        let source = vm.promise_object();
        vm.set_property(source, constructor_atom, Value::UNDEFINED)
            .unwrap();
        let roots = vm.heap.root_count_for_test();
        let result = vm
            .promise_then(&program, source, Value::UNDEFINED, Value::UNDEFINED)
            .unwrap();
        assert_eq!(vm.object_data(result).unwrap().proto, prototype);
        assert_eq!(vm.heap.root_count_for_test(), roots);
        let value = vm.object();
        assert!(
            vm.promise_resolve_for_constructor(&program, invalid, value)
                .is_err()
        );
        assert_eq!(vm.heap.root_count_for_test(), roots);
    }
}

#[test]
fn aggregate_native_roots_restore_after_reject_callbacks_throw() {
    use super::promise::AggregateMode;
    let cases = [
        (
            AggregateMode::All,
            "get resolve() {$262.gc(); throw 'resolve';}",
            "[]",
        ),
        (
            AggregateMode::All,
            "get resolve() {return value => value;}",
            "({get [Symbol.iterator]() {$262.gc(); throw 'iterator';}})",
        ),
        (
            AggregateMode::All,
            "get resolve() {return value => value;}",
            "({[Symbol.iterator]() {return {next() {$262.gc(); throw 'next';}};}})",
        ),
        (
            AggregateMode::All,
            "get resolve() {return value => value;}",
            "({[Symbol.iterator]() {return {next() {return {get done() {$262.gc(); throw 'done';}};}};}})",
        ),
        (
            AggregateMode::All,
            "get resolve() {return value => value;}",
            "({[Symbol.iterator]() {return {next() {return {done:false, get value() {$262.gc(); throw 'value';}};}};}})",
        ),
        (
            AggregateMode::All,
            "get resolve() {return function() {$262.gc(); throw {rank:42};};}",
            "({[Symbol.iterator]() {return {next() {return {done:false,value:1};}, return() {$262.gc(); throw 'close';}};}})",
        ),
        (
            AggregateMode::AllSettled,
            "get resolve() {return function() {return {get then() {$262.gc(); throw {rank:42};}};};}",
            "[1]",
        ),
        (
            AggregateMode::Any,
            "get resolve() {return function() {return {then() {$262.gc(); throw {rank:42};}};};}",
            "[1]",
        ),
        (
            AggregateMode::AllKeyed,
            "get resolve() {return value => value;}",
            "new Proxy({}, {ownKeys() {return ['entry'];}, getOwnPropertyDescriptor() {$262.gc(); throw 'descriptor';}})",
        ),
        (
            AggregateMode::AllSettledKeyed,
            "get resolve() {return value => value;}",
            "({get first() {return {rank:42};}, get last() {$262.gc(); throw 'read';}})",
        ),
    ];
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (mode, resolve, input) in cases {
            let mut vm = Vm::new(Test262Host);
            let setup = format!(
                "function Constructor(executor) {{executor(function() {{}}, function() {{$262.gc(); throw 'reject';}}); return {{}};}} Object.defineProperty(Constructor, 'resolve', Object.getOwnPropertyDescriptor({{{resolve}}}, 'resolve')); function create() {{return {input};}}"
            );
            let program = compile(&setup, "aggregate-root-errors.js").unwrap();
            vm.execute(&program).unwrap();
            let atom = vm.intern_atom("Constructor");
            let constructor = vm.own_property(vm.realm.globals, atom).unwrap();
            let atom = vm.intern_atom("create");
            let factory = vm.own_property(vm.realm.globals, atom).unwrap();
            let input = vm
                .call_value(&program, factory, Value::UNDEFINED, &[])
                .unwrap();
            let roots = vm.heap.root_count_for_test();
            let incoming = vm.active_call_roots.len();
            assert!(
                vm.promise_aggregate(&program, constructor, &[input], mode)
                    .is_err(),
                "{setup}"
            );
            assert_eq!(vm.heap.root_count_for_test(), roots, "{setup}");
            assert_eq!(vm.active_call_roots.len(), incoming, "{setup}");
        }
    }
}

#[test]
fn property_copy_roots_release_after_callback_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for create in [false, true] {
            for phase in [
                "success",
                "keys",
                "descriptor",
                "get",
                "write",
                "reject",
                "excluded",
            ] {
                let mut vm = Vm::new(Test262Host);
                let source = format!(
                    r#"
                    function operands() {{
                        var source=new Proxy({{first:1,later:2}}, {{
                            ownKeys(object) {{$262.gc(); if ('{phase}'==='keys') throw {{kind:'keys'}};return Reflect.ownKeys(object);}},
                            getOwnPropertyDescriptor(object,key) {{$262.gc();if (('{phase}'==='descriptor' && key==='later') || ('{phase}'==='excluded' && key==='first')) throw {{kind:'descriptor'}};return Reflect.getOwnPropertyDescriptor(object,key);}},
                            get(object,key) {{$262.gc();if ('{phase}'==='get' && key==='later') throw {{kind:'get'}};return {{rank:key==='first'?42:43}};}}
                        }});
                        var target=new Proxy({{}}, {{
                            set(object,key,value) {{$262.gc();if ('{phase}'==='write' && key==='later') throw {{kind:'write'}};if ('{phase}'==='reject' && key==='later') return false;return Reflect.set(object,key,value);}},
                            defineProperty(object,key,descriptor) {{$262.gc();if ('{phase}'==='write' && key==='later') throw {{kind:'write'}};if ('{phase}'==='reject' && key==='later') return false;return Reflect.defineProperty(object,key,descriptor);}}
                        }});
                        return [target,source];
                    }}
                "#
                );
                let program = compile(&source, "copy-root-scope.js").unwrap();
                vm.execute(&program).unwrap();
                let atom = vm.intern_atom("operands");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                let operands = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let args = match vm.heap.get(operands) {
                    Some(super::Cell::Array { elements, .. }) => elements.as_ref().clone(),
                    _ => panic!("operands"),
                };
                let handles = args
                    .iter()
                    .map(|v| vm.heap.weak_handle(*v).unwrap())
                    .collect::<Vec<_>>();
                let excluded = if phase == "excluded" {
                    vec![vm.heap.alloc(super::Cell::String("first".into()))]
                } else {
                    vec![]
                };
                let first_atom = vm.intern_atom("first");
                let later_atom = vm.intern_atom("later");
                let rank_atom = vm.intern_atom("rank");
                let excluded_handles = excluded
                    .iter()
                    .map(|value| vm.heap.weak_handle(*value).unwrap())
                    .collect::<Vec<_>>();
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let kind = if create {
                    super::object_symbols::PropertyCopyKind::CreateDataProperty
                } else {
                    super::object_symbols::PropertyCopyKind::Set
                };
                let result =
                    vm.copy_enumerable_properties(&program, args[0], args[1], &excluded, kind);
                assert_eq!(
                    result.is_ok(),
                    matches!(phase, "success" | "excluded"),
                    "create={create} {phase}"
                );
                if let Err(error) = &result {
                    if phase == "reject" {
                        assert!(vm.format_error(&program, error).contains("TypeError"));
                    } else {
                        let atom = vm.intern_atom("kind");
                        let kind = vm
                            .own_property(error.thrown_value().unwrap(), atom)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                    }
                }
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                if phase != "keys" && phase != "excluded" {
                    let value = vm.get_property(&program, args[0], first_atom).unwrap();
                    let rank = vm.get_property(&program, value, rank_atom).unwrap();
                    assert_eq!(rank, Value::number(42.0));
                }
                if phase == "success" || phase == "excluded" {
                    let value = vm.get_property(&program, args[0], later_atom).unwrap();
                    assert_eq!(
                        vm.get_property(&program, value, rank_atom).unwrap(),
                        Value::number(43.0)
                    );
                }
                vm.collect_now(&program);
                for handle in handles.into_iter().chain(excluded_handles) {
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn descriptor_snapshot_and_assign_root_fresh_native_arguments() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for descriptors in [false, true] {
            for fails in [false, true] {
                let mut vm = Vm::new(Test262Host);
                let source = format!(
                    r#"
                    function operands() {{return [1,new Proxy({{first:1,later:2}},{{
                        getOwnPropertyDescriptor(object,key) {{$262.gc();if ({fails} && key==='later') throw {{kind:'later'}};return {{value:{{rank:key==='first'?42:43}},writable:true,enumerable:true,configurable:true}};}},
                        get(object,key) {{$262.gc();if ({fails} && key==='later') throw {{kind:'later'}};return {{rank:key==='first'?42:43}};}}
                    }}),{{late:47}}];}}
                "#
                );
                let program = compile(&source, "snapshot-native-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let atom = vm.intern_atom("operands");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                let operands = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let args = match vm.heap.get(operands) {
                    Some(super::Cell::Array { elements, .. }) => elements.as_ref().clone(),
                    _ => panic!("operands"),
                };
                let source_handle = vm.heap.weak_handle(args[1]).unwrap();
                let later_handle = vm.heap.weak_handle(args[2]).unwrap();
                let first_atom = vm.intern_atom("first");
                let rank_atom = vm.intern_atom("rank");
                let value_atom = vm.intern_atom("value");
                let late_atom = vm.intern_atom("late");
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let result = if descriptors {
                    vm.object_get_own_property_descriptors(&program, &args[1..2])
                } else {
                    vm.object_assign(&program, &args)
                };
                assert_eq!(result.is_err(), fails);
                if let Err(error) = &result {
                    let atom = vm.intern_atom("kind");
                    let kind = vm
                        .own_property(error.thrown_value().unwrap(), atom)
                        .unwrap();
                    assert_eq!(vm.to_string(&program, kind).unwrap(), "later");
                }
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                let result_handle = if let Ok(result) = result {
                    let first = vm.get_property(&program, result, first_atom).unwrap();
                    let first = if descriptors {
                        vm.get_property(&program, first, value_atom).unwrap()
                    } else {
                        first
                    };
                    assert_eq!(
                        vm.get_property(&program, first, rank_atom).unwrap(),
                        Value::number(42.0)
                    );
                    if !descriptors {
                        assert_eq!(
                            vm.get_property(&program, result, late_atom).unwrap(),
                            Value::number(47.0)
                        );
                    }
                    Some(vm.heap.weak_handle(result).unwrap())
                } else {
                    None
                };
                vm.collect_now(&program);
                assert!(vm.heap.weak_value(source_handle).is_none());
                assert!(vm.heap.weak_value(later_handle).is_none());
                if let Some(handle) = result_handle {
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn for_in_roots_release_after_prototype_and_key_callbacks() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for query in [false, true] {
            for phase in [
                "success",
                "keys-getter",
                "keys",
                "descriptor-getter",
                "descriptor",
                "prototype-getter",
                "prototype",
                "cycle",
                "shadow",
                "missing",
            ] {
                if query && matches!(phase, "keys-getter" | "keys") {
                    continue;
                }
                let mut vm = Vm::new(Test262Host);
                let source = format!(
                    r#"
                    function operand() {{
                        var object={{first:1,later:2}};
                        Object.defineProperty(object,'hidden',{{value:4,enumerable:false,configurable:true}});
                        var source=new Proxy(object,{{
                            get ownKeys() {{$262.gc();if ('{phase}'==='keys-getter') throw {{kind:'keys-getter'}};
                                return function(object) {{$262.gc();if ('{phase}'==='keys') throw {{kind:'keys'}};return Reflect.ownKeys(object);}};
                            }},
                            get getOwnPropertyDescriptor() {{$262.gc();if ('{phase}'==='descriptor-getter') throw {{kind:'descriptor-getter'}};
                                return function(object,key) {{$262.gc();if ('{phase}'==='descriptor') throw {{kind:'descriptor'}};return Reflect.getOwnPropertyDescriptor(object,key);}};
                            }},
                            get getPrototypeOf() {{$262.gc();if ('{phase}'==='prototype-getter') throw {{kind:'prototype-getter'}};
                                return function() {{$262.gc();if ('{phase}'==='prototype') throw {{kind:'prototype'}};
                                    if ('{phase}'==='cycle') return source;
                                    return new Proxy({{inherited:3,hidden:5}},{{
                                        ownKeys(object) {{$262.gc();return Reflect.ownKeys(object);}},
                                        getOwnPropertyDescriptor(object,key) {{$262.gc();return Reflect.getOwnPropertyDescriptor(object,key);}},
                                        getPrototypeOf() {{$262.gc();return null;}}
                                    }});
                                }};
                            }}
                        }});
                        return source;
                    }}
                "#
                );
                let program = compile(&source, "for-in-native-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let atom = vm.intern_atom("operand");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                let source = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let source_handle = vm.heap.weak_handle(source).unwrap();
                let key = vm.heap.alloc(super::Cell::String(
                    match phase {
                        "shadow" => "hidden",
                        "missing" => "absent",
                        _ => "inherited",
                    }
                    .into(),
                ));
                let key_handle = vm.heap.weak_handle(key).unwrap();
                // Snapshot collection has no key input to keep alive.
                let key_root = (!query).then(|| vm.heap.root(key));
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let result = if query {
                    vm.object_for_in_key_is_enumerable(&program, source, key)
                        .map(Vm::<Test262Host>::integrity_bool)
                } else {
                    vm.object_for_in_keys(&program, source)
                };
                let succeeds = matches!(phase, "success" | "cycle" | "shadow" | "missing");
                assert_eq!(result.is_ok(), succeeds, "query={query} {phase}");
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                let result_handle = match result {
                    Ok(result) if query => {
                        assert_eq!(
                            result,
                            Vm::<Test262Host>::integrity_bool(phase == "success")
                        );
                        None
                    }
                    Ok(result) => {
                        let elements = match vm.heap.get(result) {
                            Some(super::Cell::Array { elements, .. }) => elements.as_ref().clone(),
                            _ => panic!("key snapshot"),
                        };
                        let names = elements
                            .into_iter()
                            .map(|key| vm.to_string(&program, key).unwrap())
                            .collect::<Vec<_>>();
                        let expected = if phase == "cycle" {
                            vec!["first", "later"]
                        } else {
                            vec!["first", "later", "inherited"]
                        };
                        assert_eq!(names, expected);
                        Some(vm.heap.weak_handle(result).unwrap())
                    }
                    Err(error) => {
                        let atom = vm.intern_atom("kind");
                        let kind = vm
                            .own_property(error.thrown_value().unwrap(), atom)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                        None
                    }
                };
                if let Some(root) = key_root {
                    vm.heap.release_root(root);
                }
                vm.collect_now(&program);
                assert!(vm.heap.weak_value(source_handle).is_none());
                assert!(vm.heap.weak_value(key_handle).is_none());
                if let Some(handle) = result_handle {
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn proxy_introspection_roots_release_after_nested_validation() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for operation in ["prototype", "descriptor", "extensible"] {
            let phases: &[&str] = match operation {
                "prototype" => &[
                    "success",
                    "getter",
                    "trap",
                    "extensible",
                    "target-prototype",
                    "fallback",
                    "revoke-fallback",
                    "invalid-trap",
                    "invalid-result",
                    "mismatch",
                    "revoked",
                    "sealed",
                ],
                "descriptor" => &[
                    "success",
                    "key",
                    "getter",
                    "trap",
                    "target-descriptor",
                    "extensible",
                    "descriptor-value",
                    "fallback",
                    "revoke-fallback",
                    "invalid-trap",
                    "invalid-result",
                    "mismatch",
                    "revoked",
                    "hide-absent",
                    "hide-present",
                    "hide-frozen",
                ],
                _ => &[
                    "success",
                    "getter",
                    "trap",
                    "extensible",
                    "fallback",
                    "revoke-fallback",
                    "invalid-trap",
                    "mismatch",
                    "revoked",
                ],
            };
            for phase in phases {
                let mut vm = Vm::new(Test262Host);
                let source = format!(
                    r#"
                    function operands() {{
                        var raw=Object.create({{rank:46}});raw.entry={{rank:42}};
                        if ('{phase}'==='hide-frozen') Object.defineProperty(raw,'entry',{{configurable:false}});
                        if ('{operation}'==='prototype' && ('{phase}'==='target-prototype' || '{phase}'==='mismatch' || '{phase}'==='sealed')) Object.preventExtensions(raw);
                        var target=new Proxy(raw,{{
                            getOwnPropertyDescriptor(object,key) {{$262.gc();if ('{phase}'==='target-descriptor') throw {{kind:'target-descriptor'}};return Reflect.getOwnPropertyDescriptor(object,key);}},
                            get isExtensible() {{$262.gc();return function(object) {{$262.gc();if ('{phase}'==='extensible') throw {{kind:'extensible'}};return Reflect.isExtensible(object);}};}},
                            getPrototypeOf(object) {{$262.gc();if ('{phase}'==='target-prototype') throw {{kind:'target-prototype'}};return Reflect.getPrototypeOf(object);}}
                        }});
                        function method() {{$262.gc();if ('{phase}'==='getter') throw {{kind:'getter'}};
                            if ('{phase}'==='invalid-trap') return 1;
                            if ('{phase}'==='revoke-fallback') {{revocable.revoke();$262.gc();return null;}}
                            if ('{phase}'==='fallback') return undefined;
                            return function() {{$262.gc();if ('{phase}'==='trap') throw {{kind:'trap'}};
                                if ('{phase}'==='invalid-result') return 1;
                                if ('{operation}'==='extensible') return '{phase}'!=='mismatch';
                                if ('{operation}'==='prototype') return '{phase}'==='sealed' ? Reflect.getPrototypeOf(target) : {{rank:44}};
                                if ('{phase}'.startsWith('hide-')) return undefined;
                                return {{get value() {{$262.gc();if ('{phase}'==='descriptor-value') throw {{kind:'descriptor-value'}};return {{rank:43}};}},
                                    writable:true,enumerable:true,configurable:'{phase}'!=='mismatch'}};
                            }};
                        }}
                        var revocable=Proxy.revocable(target,{{get getPrototypeOf() {{return method();}},get getOwnPropertyDescriptor() {{return method();}},get isExtensible() {{return method();}}}});
                        if ('{phase}'==='revoked') revocable.revoke();
                        return [revocable.proxy,{{toString() {{$262.gc();if ('{phase}'==='key') throw {{kind:'key'}};return '{phase}'==='hide-absent' ? 'missing' : 'entry';}}}}];
                    }}
                "#
                );
                let program = compile(&source, "proxy-introspection-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let atom = vm.intern_atom("operands");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                let operands = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let args = match vm.heap.get(operands) {
                    Some(super::Cell::Array { elements, .. }) => elements.as_ref().clone(),
                    _ => panic!("operands"),
                };
                let handles = args
                    .iter()
                    .map(|value| vm.heap.weak_handle(*value).unwrap())
                    .collect::<Vec<_>>();
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let result = match operation {
                    "prototype" => vm.object_get_prototype_of(&program, args[0]),
                    "descriptor" => vm.object_get_own_property_descriptor(&program, &args),
                    _ => vm.object_is_extensible(&program, &args[..1]),
                };
                let succeeds = matches!(
                    *phase,
                    "success"
                        | "fallback"
                        | "revoke-fallback"
                        | "sealed"
                        | "hide-absent"
                        | "hide-present"
                );
                assert_eq!(result.is_ok(), succeeds, "{operation} {phase}");
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                let result_handle = match result {
                    Ok(result) if operation == "extensible" => {
                        assert_eq!(result, Value::TRUE);
                        None
                    }
                    Ok(result) if phase.starts_with("hide-") => {
                        assert!(result.is_undefined());
                        None
                    }
                    Ok(result) => {
                        let rank_atom = vm.intern_atom("rank");
                        let value_atom = vm.intern_atom("value");
                        let value = if operation == "descriptor" {
                            vm.own_property(result, value_atom).unwrap()
                        } else {
                            result
                        };
                        let expected = if operation == "descriptor" {
                            if phase.ends_with("fallback") {
                                42.0
                            } else {
                                43.0
                            }
                        } else if phase.ends_with("fallback") || *phase == "sealed" {
                            46.0
                        } else {
                            44.0
                        };
                        assert_eq!(
                            vm.own_property(value, rank_atom),
                            Some(Value::number(expected))
                        );
                        Some(vm.heap.weak_handle(result).unwrap())
                    }
                    Err(error) => {
                        if matches!(
                            *phase,
                            "invalid-trap"
                                | "invalid-result"
                                | "mismatch"
                                | "revoked"
                                | "hide-frozen"
                        ) {
                            assert!(vm.format_error(&program, &error).contains("TypeError"));
                        } else {
                            let atom = vm.intern_atom("kind");
                            let value = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), *phase);
                        }
                        None
                    }
                };
                vm.collect_now(&program);
                for handle in handles {
                    assert!(vm.heap.weak_value(handle).is_none(), "{operation} {phase}");
                }
                if let Some(handle) = result_handle {
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn proxy_reads_root_operands_and_results_through_nested_callbacks() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for symbol in [false, true] {
            for phase in [
                "success",
                "getter",
                "trap",
                "descriptor",
                "field",
                "fallback",
                "invalid-trap",
                "revoked",
                "frozen-same",
                "frozen-different",
                "no-getter-undefined",
                "no-getter-value",
                "result-symbol",
            ] {
                let mut vm = Vm::new(Test262Host);
                let source = format!(
                    r#"
                    function operands() {{
                        var target=new Proxy({{}},{{getOwnPropertyDescriptor(object,key) {{$262.gc();
                            if ('{phase}'==='descriptor') throw {{kind:'descriptor'}};
                            if ('{phase}'==='field') return {{get value() {{$262.gc();throw {{kind:'field'}};}},writable:true,enumerable:true,configurable:true}};
                            return Reflect.getOwnPropertyDescriptor(object,key);
                        }}}});
                        var handler={{get get() {{$262.gc();if ('{phase}'==='getter') throw {{kind:'getter'}};
                            if ('{phase}'==='invalid-trap') return 1;
                            if ('{phase}'==='fallback') return null;
                            return function(object,key,receiver) {{$262.gc();if ('{phase}'==='trap') throw {{kind:'trap'}};
                                if ('{phase}'==='result-symbol') return Symbol('answer');
                                if ('{phase}'==='frozen-same') return 42;
                                if ('{phase}'==='frozen-different') return 43;
                                if ('{phase}'==='no-getter-undefined') return undefined;
                                if ('{phase}'==='no-getter-value') return 42;
                                return {{rank:47,key:key,receiver:receiver}};
                            }};
                        }}}};
                        return [target,handler,{{rank:46}}];
                    }}
                "#
                );
                let program = compile(&source, "proxy-read-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let atom = vm.intern_atom("operands");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                let operands = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let args = match vm.heap.get(operands) {
                    Some(super::Cell::Array { elements, .. }) => elements.as_ref().clone(),
                    _ => panic!("operands"),
                };
                let handles = args
                    .iter()
                    .map(|value| vm.heap.weak_handle(*value).unwrap())
                    .collect::<Vec<_>>();
                let atom = vm.intern_atom("entry");
                let property = if symbol {
                    super::property_key::PropertyKey::symbol(
                        vm.heap.alloc(super::Cell::Symbol(Some("entry".into()))),
                    )
                } else {
                    super::property_key::PropertyKey::string(atom)
                };
                let key = if symbol {
                    property.symbol_value().unwrap()
                } else {
                    vm.heap.alloc(super::Cell::String("entry".into()))
                };
                let key_handle = vm.heap.weak_handle(key).unwrap();
                if phase.starts_with("frozen-")
                    || phase.starts_with("no-getter-")
                    || phase == "fallback"
                {
                    let record = if phase.starts_with("no-getter-") {
                        super::object_descriptors::PropertyDescriptorRecord {
                            value: None,
                            writable: None,
                            getter: Some(Value::UNDEFINED),
                            setter: Some(Value::UNDEFINED),
                            enumerable: Some(true),
                            configurable: Some(false),
                        }
                    } else {
                        super::object_descriptors::PropertyDescriptorRecord {
                            writable: Some(false),
                            configurable: Some(false),
                            ..super::object_descriptors::PropertyDescriptorRecord::data(
                                Value::number(42.0),
                            )
                        }
                    };
                    vm.define_property_or_throw(&program, args[0], key, record)
                        .unwrap();
                }
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let handler = if phase == "revoked" {
                    Value::NULL
                } else {
                    args[1]
                };
                let result = vm.proxy_get(&program, args[0], handler, args[2], property);
                assert_eq!(
                    result.is_ok(),
                    matches!(
                        phase,
                        "success"
                            | "fallback"
                            | "frozen-same"
                            | "no-getter-undefined"
                            | "result-symbol"
                    ),
                    "symbol={symbol} {phase}"
                );
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                let result_handle = match result {
                    Ok(result) if phase == "success" => {
                        let rank = vm.intern_atom("rank");
                        let receiver = vm.intern_atom("receiver");
                        let key_atom = vm.intern_atom("key");
                        assert_eq!(vm.own_property(result, rank), Some(Value::number(47.0)));
                        assert_eq!(vm.own_property(result, receiver), Some(args[2]));
                        assert_eq!(vm.own_property(args[2], rank), Some(Value::number(46.0)));
                        let returned_key = vm.own_property(result, key_atom).unwrap();
                        if symbol {
                            assert_eq!(returned_key, key);
                            assert!(
                                matches!(vm.heap.get(returned_key), Some(super::Cell::Symbol(Some(name))) if name == "entry")
                            );
                        } else {
                            assert_eq!(vm.to_string(&program, returned_key).unwrap(), "entry");
                        }
                        Some(vm.heap.weak_handle(result).unwrap())
                    }
                    Ok(result) if phase == "result-symbol" => {
                        assert!(
                            matches!(vm.heap.get(result),Some(super::Cell::Symbol(Some(name))) if name=="answer")
                        );
                        Some(vm.heap.weak_handle(result).unwrap())
                    }
                    Ok(result) => {
                        assert_eq!(
                            result,
                            if phase == "no-getter-undefined" {
                                Value::UNDEFINED
                            } else {
                                Value::number(42.0)
                            }
                        );
                        None
                    }
                    Err(error) => {
                        if matches!(
                            phase,
                            "invalid-trap" | "revoked" | "frozen-different" | "no-getter-value"
                        ) {
                            assert!(vm.format_error(&program, &error).contains("TypeError"));
                        } else {
                            let atom = vm.intern_atom("kind");
                            let kind = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                        }
                        None
                    }
                };
                vm.collect_now(&program);
                for handle in handles {
                    assert!(
                        vm.heap.weak_value(handle).is_none(),
                        "symbol={symbol} {phase}"
                    );
                }
                assert!(vm.heap.weak_value(key_handle).is_none());
                if let Some(handle) = result_handle {
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn proxy_mutations_root_fresh_operands_and_restore_scopes() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for reflect in [false, true] {
            for prototype in [false, true] {
                for phase in [
                    "success",
                    "getter",
                    "trap",
                    "extensible",
                    "prototype",
                    "reject",
                    "invalid-trap",
                    "fallback",
                    "invariant",
                    "revoked",
                ] {
                    if !prototype && phase == "prototype" {
                        continue;
                    }
                    let mut vm = Vm::new(Test262Host);
                    let source = format!(
                        r#"
                        function operand() {{
                            var raw={{}};
                            if ({prototype} && ('{phase}'==='prototype' || '{phase}'==='invariant')) Object.preventExtensions(raw);
                            var target=new Proxy(raw,{{
                                isExtensible(object) {{$262.gc();if ('{phase}'==='extensible') throw {{kind:'extensible'}};return Reflect.isExtensible(object);}},
                                getPrototypeOf(object) {{$262.gc();if ('{phase}'==='prototype') throw {{kind:'prototype'}};return Reflect.getPrototypeOf(object);}},
                                setPrototypeOf(object,proto) {{$262.gc();return Reflect.setPrototypeOf(object,proto);}},
                                preventExtensions(object) {{$262.gc();return Reflect.preventExtensions(object);}}
                            }});
                            function method() {{$262.gc();if ('{phase}'==='getter') throw {{kind:'getter'}};
                                if ('{phase}'==='invalid-trap') return 1;
                                if ('{phase}'==='fallback') return null;
                                return function(object,proto) {{$262.gc();if ('{phase}'==='trap') throw {{kind:'trap'}};
                                    if ('{phase}'==='reject') return false;
                                    if ('{phase}'==='invariant' || '{phase}'==='prototype' || '{phase}'==='extensible') return true;
                                    return {prototype} ? Reflect.setPrototypeOf(object,proto) : Reflect.preventExtensions(object);
                                }};
                            }}
                            if ('{phase}'==='revoked') {{var revocable=Proxy.revocable(target,{{}});revocable.revoke();return revocable.proxy;}}
                            return new Proxy(target,{{get setPrototypeOf() {{return method();}},get preventExtensions() {{return method();}}}});
                        }}
                    "#
                    );
                    let program = compile(&source, "proxy-mutation-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let atom = vm.intern_atom("operand");
                    let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                    let target = vm
                        .call_value(&program, factory, Value::UNDEFINED, &[])
                        .unwrap();
                    let target_handle = vm.heap.weak_handle(target).unwrap();
                    let proto = vm.object();
                    vm.set_named(&program, proto, "rank", Value::number(42.0))
                        .unwrap();
                    let proto_handle = vm.heap.weak_handle(proto).unwrap();
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let native = match (reflect, prototype) {
                        (false, false) => Native::ObjectPreventExtensions,
                        (false, true) => Native::ObjectSetPrototypeOf,
                        (true, false) => Native::ReflectPreventExtensions,
                        (true, true) => Native::ReflectSetPrototypeOf,
                    };
                    let args = if prototype {
                        vec![target, proto]
                    } else {
                        vec![target]
                    };
                    let result = if reflect {
                        vm.call_reflect_native(&program, native, &args)
                    } else {
                        vm.call_object_native(&program, native, &args)
                    };
                    let success = phase == "success" || phase == "fallback";
                    assert_eq!(
                        result.is_ok(),
                        success || (reflect && phase == "reject"),
                        "reflect={reflect} prototype={prototype} {phase}"
                    );
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    match result {
                        Ok(result) => {
                            assert_eq!(
                                result,
                                if reflect {
                                    Vm::<Test262Host>::integrity_bool(success)
                                } else {
                                    target
                                }
                            );
                            assert_eq!(
                                vm.heap.weak_value(target_handle),
                                Some(target),
                                "reflect={reflect} prototype={prototype} {phase}"
                            );
                            if prototype {
                                assert_eq!(vm.heap.weak_value(proto_handle), Some(proto));
                                let rank = vm.intern_atom("rank");
                                assert_eq!(vm.own_property(proto, rank), Some(Value::number(42.0)));
                                if success {
                                    let raw = vm.proxy_target(target);
                                    assert_eq!(vm.object_data(raw).unwrap().proto, proto);
                                }
                            } else if success {
                                let raw = vm.proxy_target(target);
                                assert!(!vm.object_data(raw).unwrap().is_extensible());
                            }
                        }
                        Err(error) => {
                            if matches!(phase, "reject" | "invalid-trap" | "invariant" | "revoked")
                            {
                                assert!(vm.format_error(&program, &error).contains("TypeError"));
                            } else {
                                let atom = vm.intern_atom("kind");
                                let kind = vm
                                    .own_property(error.thrown_value().unwrap(), atom)
                                    .unwrap();
                                assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                            }
                        }
                    }
                    vm.collect_now(&program);
                    assert!(vm.heap.weak_value(target_handle).is_none());
                    assert!(vm.heap.weak_value(proto_handle).is_none());
                }
            }
        }
    }
}


#[test]
fn proxy_invocations_root_fresh_operands_and_restore_scopes() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for construct in [false, true] {
            for phase in [
                "success",
                "fallback",
                "getter",
                "trap",
                "invalid-trap",
                "primitive-result",
                "revoked",
            ] {
                if !construct && phase == "primitive-result" {
                    continue;
                }
                let mut vm = Vm::new(Test262Host);
                let method = if construct { "construct" } else { "apply" };
                let source = format!(
                    r#"
                    function operand() {{
                        function target(arg) {{ $262.gc(); return {{rank:42,arg:arg,receiver:this}}; }}
                        var handler={{get {method}() {{
                            $262.gc();
                            if ('{phase}'==='getter') throw {{kind:'getter'}};
                            if ('{phase}'==='fallback') return null;
                            if ('{phase}'==='invalid-trap') return 1;
                            return function(target,second,third) {{
                                $262.gc();
                                if ('{phase}'==='trap') throw {{kind:'trap'}};
                                if ('{phase}'==='primitive-result') return 1;
                                return {{rank:42,arg:{construct} ? second[0] : third[0],receiver:{construct} ? third : second}};
                            }};
                        }}}};
                        if ('{phase}'==='revoked') {{var r=Proxy.revocable(target,handler);r.revoke();return r.proxy;}}
                        return new Proxy(target,handler);
                    }}
                "#
                );
                let program = compile(&source, "proxy-invocation-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let atom = vm.intern_atom("operand");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                let proxy = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let receiver = vm.object();
                vm.set_named(&program, receiver, "rank", Value::number(46.0))
                    .unwrap();
                let arg = vm.object();
                vm.set_named(&program, arg, "rank", Value::number(47.0))
                    .unwrap();
                let handles =
                    [proxy, receiver, arg].map(|value| vm.heap.weak_handle(value).unwrap());
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let result = if construct {
                    vm.proxy_construct(&program, proxy, proxy, &[arg])
                } else {
                    vm.proxy_call(&program, proxy, receiver, &[arg])
                };
                assert_eq!(
                    result.is_ok(),
                    matches!(phase, "success" | "fallback"),
                    "construct={construct} {phase}"
                );
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                let result_handle = match result {
                    Ok(result) => {
                        assert_eq!(
                            vm.heap.weak_value(handles[0]),
                            Some(proxy),
                            "construct={construct} {phase}"
                        );
                        assert_eq!(vm.heap.weak_value(handles[2]), Some(arg));
                        let rank = vm.intern_atom("rank");
                        assert_eq!(vm.own_property(result, rank), Some(Value::number(42.0)));
                        assert_eq!(vm.own_property(arg, rank), Some(Value::number(47.0)));
                        let atom = vm.intern_atom("arg");
                        assert_eq!(vm.own_property(result, atom), Some(arg));
                        if !construct || phase == "success" {
                            let atom = vm.intern_atom("receiver");
                            assert_eq!(
                                vm.own_property(result, atom),
                                Some(if construct { proxy } else { receiver })
                            );
                            if !construct {
                                assert_eq!(
                                    vm.own_property(receiver, rank),
                                    Some(Value::number(46.0))
                                );
                            }
                        }
                        Some(vm.heap.weak_handle(result).unwrap())
                    }
                    Err(error) => {
                        if matches!(phase, "getter" | "trap") {
                            let atom = vm.intern_atom("kind");
                            let kind = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                        } else {
                            assert!(
                                vm.format_error(&program, &error).contains("TypeError"),
                                "construct={construct} {phase}: {}",
                                vm.format_error(&program, &error)
                            );
                        }
                        None
                    }
                };
                vm.collect_now(&program);
                for handle in handles {
                    assert!(
                        vm.heap.weak_value(handle).is_none(),
                        "construct={construct} {phase}"
                    );
                }
                if let Some(handle) = result_handle {
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}


#[test]
fn proxy_presence_and_delete_root_fresh_operands_and_restore_scopes() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for remove in [false, true] {
            for symbol in [false, true] {
                for phase in [
                    "success",
                    "skip-validation",
                    "getter",
                    "trap",
                    "descriptor",
                    "field",
                    "extensible",
                    "invalid-trap",
                    "fallback",
                    "nonconfig",
                    "nonext",
                    "revoked",
                    "coercion",
                ] {
                    if symbol && phase == "coercion" {
                        continue;
                    }
                    let mut vm = Vm::new(Test262Host);
                    let method = if remove { "deleteProperty" } else { "has" };
                    let source = format!(
                        r#"
                        function operands() {{
                            var raw={{entry:42}};
                            if ('{phase}'==='nonconfig') Object.defineProperty(raw,'entry',{{configurable:false}});
                            if ('{phase}'==='nonext') Object.preventExtensions(raw);
                            var target=new Proxy(raw,{{
                                getOwnPropertyDescriptor(object,key) {{$262.gc();
                                    if ('{phase}'==='descriptor') throw {{kind:'descriptor'}};
                                    if ('{phase}'==='field') return {{value:42,writable:true,enumerable:true,get configurable() {{$262.gc();throw {{kind:'field'}};}}}};
                                    return Reflect.getOwnPropertyDescriptor(object,key);
                                }},
                                isExtensible(object) {{$262.gc();if ('{phase}'==='extensible') throw {{kind:'extensible'}};return Reflect.isExtensible(object);}}
                            }});
                            var handler={{get {method}() {{$262.gc();
                                if ('{phase}'==='getter') throw {{kind:'getter'}};
                                if ('{phase}'==='invalid-trap') return 1;
                                if ('{phase}'==='fallback') return null;
                                return function(object,key) {{$262.gc();if ('{phase}'==='trap') throw {{kind:'trap'}};
                                    return '{phase}'==='skip-validation' ? !{remove} : {remove};
                                }};
                            }}}};
                            if ('{phase}'==='revoked') {{var r=Proxy.revocable(target,handler);r.revoke();return [r.proxy,raw];}}
                            return [new Proxy(target,handler),raw];
                        }}
                        function keyOperand() {{return {{toString() {{$262.gc();throw {{kind:'coercion'}};}}}};}}
                    "#
                    );
                    let program = compile(&source, "proxy-presence-delete-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let atom = vm.intern_atom("operands");
                    let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                    let values = vm
                        .call_value(&program, factory, Value::UNDEFINED, &[])
                        .unwrap();
                    let (proxy, raw) = match vm.heap.get(values) {
                        Some(super::Cell::Array { elements, .. }) => (elements[0], elements[1]),
                        _ => panic!("operands"),
                    };
                    let key = if symbol {
                        vm.heap.alloc(super::Cell::Symbol(Some("entry".into())))
                    } else if phase == "coercion" {
                        let atom = vm.intern_atom("keyOperand");
                        let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                        vm.call_value(&program, factory, Value::UNDEFINED, &[])
                            .unwrap()
                    } else {
                        vm.heap.alloc(super::Cell::String("entry".into()))
                    };
                    if symbol {
                        if phase == "nonext" {
                            vm.object_data_mut(raw).unwrap().set_extensible(true);
                        }
                        vm.define_property_or_throw(
                            &program,
                            raw,
                            key,
                            super::object_descriptors::PropertyDescriptorRecord {
                                configurable: Some(phase != "nonconfig"),
                                ..super::object_descriptors::PropertyDescriptorRecord::data(
                                    Value::number(42.0),
                                )
                            },
                        )
                        .unwrap();
                        if phase == "nonext" {
                            vm.object_data_mut(raw).unwrap().set_extensible(false);
                        }
                    }
                    let handles = [proxy, key].map(|value| vm.heap.weak_handle(value).unwrap());
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let result = vm.call_reflect_native(
                        &program,
                        if remove {
                            Native::ReflectDeleteProperty
                        } else {
                            Native::ReflectHas
                        },
                        &[proxy, key],
                    );
                    assert_eq!(
                        result.is_ok(),
                        matches!(phase, "success" | "skip-validation" | "fallback"),
                        "remove={remove} symbol={symbol} {phase}"
                    );
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    match result {
                        Ok(result) => {
                            assert_eq!(
                                result,
                                Vm::<Test262Host>::integrity_bool(if phase == "fallback" {
                                    true
                                } else if phase == "skip-validation" {
                                    !remove
                                } else {
                                    remove
                                })
                            );
                            assert_eq!(
                                vm.heap.weak_value(handles[0]),
                                Some(proxy),
                                "remove={remove} symbol={symbol} {phase}"
                            );
                            assert_eq!(vm.heap.weak_value(handles[1]), Some(key));
                            assert!(if symbol {
                                matches!(vm.heap.get(key),Some(super::Cell::Symbol(Some(name))) if name=="entry")
                            } else {
                                matches!(vm.heap.get(key),Some(super::Cell::String(name)) if name.host_string()=="entry")
                            });
                        }
                        Err(error) => {
                            if matches!(phase, "invalid-trap" | "nonconfig" | "nonext" | "revoked")
                            {
                                assert!(vm.format_error(&program, &error).contains("TypeError"));
                            } else {
                                let atom = vm.intern_atom("kind");
                                let kind = vm
                                    .own_property(error.thrown_value().unwrap(), atom)
                                    .unwrap();
                                assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                            }
                        }
                    }
                    vm.collect_now(&program);
                    for handle in handles {
                        assert!(
                            vm.heap.weak_value(handle).is_none(),
                            "remove={remove} symbol={symbol} {phase}"
                        );
                    }
                }
            }
        }
    }
}


#[test]
fn proxy_revocation_consumes_captured_ownership_and_preserves_kind() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (kind, body, callable, constructable) in [
            ("object", "return {};", false, false),
            ("callable", "return ()=>42;", true, false),
            ("constructor", "return function(){return 42;};", true, true),
        ] {
            for owner in ["proxy", "revoker", "record"] {
                for entry in ["direct", "call", "guarded"] {
                    let mut vm = Vm::new(Test262Host);
                    let program = compile(
                        &format!("function operand() {{{body}}}"),
                        "proxy-revocation-owners.js",
                    )
                    .unwrap();
                    vm.execute(&program).unwrap();
                    let atom = vm.intern_atom("operand");
                    let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                    let target = vm
                        .call_value(&program, factory, Value::UNDEFINED, &[])
                        .unwrap();
                    let handler = vm.object();
                    let record = vm.proxy_revocable(&program, &[target, handler]).unwrap();
                    let atom = vm.intern_atom("proxy");
                    let proxy = vm.own_property(record, atom).unwrap();
                    let atom = vm.intern_atom("revoke");
                    let revoke = vm.own_property(record, atom).unwrap();
                    let handles = [target, handler, proxy, revoke, record]
                        .map(|value| vm.heap.weak_handle(value).unwrap());
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let active_native = vm.realm.promise.active_native.len();
                    let realm = vm.realm.globals;
                    let revoke_root = vm.heap.root(revoke);
                    let owner_root = match owner {
                        "proxy" => Some(vm.heap.root(proxy)),
                        "record" => Some(vm.heap.root(record)),
                        _ => None,
                    };
                    if owner == "record" {
                        let key = vm.heap.alloc(super::Cell::String("proxy".into()));
                        assert_eq!(
                            vm.object_delete_property(&program, &[record, key]).unwrap(),
                            Value::TRUE
                        );
                    }
                    vm.collect_now(&program);
                    for (handle, value) in handles[..4].iter().zip([target, handler, proxy, revoke])
                    {
                        assert_eq!(
                            vm.heap.weak_value(*handle),
                            Some(value),
                            "{kind} {owner} {entry}"
                        );
                    }
                    for _ in 0..2 {
                        let result = match entry {
                            "direct" => vm.proxy_revoke(revoke),
                            "call" => vm.call_value(&program, revoke, Value::TRUE, &[Value::FALSE]),
                            _ => vm.call_native_guarded(
                                &program,
                                Native::ProxyRevoke,
                                Value::TRUE,
                                &[Value::FALSE],
                                revoke,
                            ),
                        };
                        assert_eq!(result.unwrap(), Value::UNDEFINED);
                        assert_eq!(vm.active_call_roots.len(), calls);
                        assert_eq!(vm.realm.promise.active_native.len(), active_native);
                        assert_eq!(vm.realm.globals, realm);
                    }
                    assert_eq!(vm.is_function(proxy), callable, "{kind} {owner} {entry}");
                    assert_eq!(
                        vm.is_constructable(&program, proxy),
                        constructable,
                        "{kind} {owner} {entry}"
                    );
                    if owner != "revoker" {
                        vm.heap.release_root(revoke_root);
                    }
                    vm.collect_now(&program);
                    assert!(
                        vm.heap.weak_value(handles[0]).is_none(),
                        "target retained: {kind} {owner} {entry}"
                    );
                    assert!(
                        vm.heap.weak_value(handles[1]).is_none(),
                        "handler retained: {kind} {owner} {entry}"
                    );
                    assert_eq!(
                        vm.heap.weak_value(handles[2]),
                        (owner == "proxy").then_some(proxy),
                        "{kind} {owner} {entry}"
                    );
                    assert_eq!(
                        vm.heap.weak_value(handles[3]),
                        (owner != "proxy").then_some(revoke)
                    );
                    assert_eq!(
                        vm.heap.weak_value(handles[4]),
                        (owner == "record").then_some(record)
                    );
                    if owner != "proxy" {
                        assert!(
                            matches!(vm.heap.get(revoke),Some(super::Cell::Function {env,..}) if env.is_null())
                        );
                        assert_eq!(
                            vm.call_value(&program, revoke, Value::UNDEFINED, &[])
                                .unwrap(),
                            Value::UNDEFINED
                        );
                    }
                    if let Some(root) = owner_root {
                        vm.heap.release_root(root);
                    } else {
                        vm.heap.release_root(revoke_root);
                    }
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    vm.collect_now(&program);
                    for handle in handles {
                        assert!(
                            vm.heap.weak_value(handle).is_none(),
                            "{kind} {owner} {entry}"
                        );
                    }
                }
            }
        }
    }
}


#[test]
fn proxy_own_keys_keeps_captured_target_after_revoking_getter() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for trapped in [false, true] {
            for phase in ["success", "getter", "target", "trap"] {
                if !trapped && phase == "trap" {
                    continue;
                }
                let mut vm = Vm::new(Test262Host);
                let source = format!(
                    r#"
                    function operand() {{
                        var revoke;
                        var r=Proxy.revocable(new Proxy({{first:42,[Symbol('entry')]:43}},{{ownKeys(object) {{
                            $262.gc();if ('{phase}'==='target') throw {{kind:'target'}};return Reflect.ownKeys(object);
                        }}}}),{{get ownKeys() {{
                            revoke();$262.gc();if ('{phase}'==='getter') throw {{kind:'getter'}};
                            if (!{trapped}) return null;
                            return function(object) {{$262.gc();if ('{phase}'==='trap') throw {{kind:'trap'}};return Reflect.ownKeys(object);}};
                        }}}});
                        revoke=r.revoke;
                        return r.proxy;
                    }}
                "#
                );
                let program = compile(&source, "proxy-own-keys-revocation-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let atom = vm.intern_atom("operand");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                let proxy = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let (target, handler) = match vm.heap.get(proxy) {
                    Some(super::Cell::Proxy {
                        target, handler, ..
                    }) => (*target, *handler),
                    _ => panic!("proxy"),
                };
                let handles =
                    [proxy, target, handler].map(|value| vm.heap.weak_handle(value).unwrap());
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let result = vm.proxy_own_keys(&program, proxy);
                assert_eq!(
                    result.is_ok(),
                    phase == "success",
                    "trapped={trapped} {phase}"
                );
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                let keys = match result {
                    Ok(keys) => {
                        assert_eq!(keys.len(), 2);
                        assert_eq!(vm.to_string(&program, keys[0]).unwrap(), "first");
                        assert!(
                            matches!(vm.heap.get(keys[1]),Some(super::Cell::Symbol(Some(name))) if name=="entry")
                        );
                        for (handle, value) in handles.iter().zip([proxy, target, handler]) {
                            assert_eq!(
                                vm.heap.weak_value(*handle),
                                Some(value),
                                "trapped={trapped} {phase}"
                            );
                        }
                        assert!(
                            matches!(vm.heap.get(proxy),Some(super::Cell::Proxy {target,handler,..}) if target.is_null()&&handler.is_null())
                        );
                        keys.into_iter()
                            .map(|value| vm.heap.weak_handle(value).unwrap())
                            .collect::<Vec<_>>()
                    }
                    Err(error) => {
                        let atom = vm.intern_atom("kind");
                        let kind = vm
                            .own_property(error.thrown_value().unwrap(), atom)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                        Vec::new()
                    }
                };
                vm.collect_now(&program);
                for handle in handles.into_iter().chain(keys) {
                    assert!(
                        vm.heap.weak_value(handle).is_none(),
                        "trapped={trapped} {phase}"
                    );
                }
            }
        }
    }
}


#[test]
fn coerced_binary_operands_restore_roots_after_each_completion() {
    use oxc_ast::ast::BinaryOperator;
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for kind in ["string", "numeric-string", "bigint"] {
            for (operator, token) in [
                (BinaryOperator::LessThan, "<"),
                (BinaryOperator::GreaterEqualThan, ">="),
                (BinaryOperator::Addition, "+"),
                (BinaryOperator::Subtraction, "-"),
            ] {
                for phase in ["success", "left", "right"] {
                    let mut vm = Vm::new(Test262Host);
                    let source = format!(
                        r#"
                        function operands() {{return [
                            {{get [Symbol.toPrimitive]() {{$262.gc();if ('{phase}'==='left') throw {{kind:'left'}};
                                return function(hint) {{$262.gc();return '{kind}'==='bigint'?BigInt(42):'{kind}'==='numeric-string'?String.fromCharCode(52,50):String.fromCharCode(97,98);}};
                            }}}},
                            {{get [Symbol.toPrimitive]() {{$262.gc();if ('{phase}'==='right') throw {{kind:'right'}};
                                return function(hint) {{$262.gc();return '{kind}'==='bigint'?BigInt(7):'{kind}'==='numeric-string'?String.fromCharCode(55):String.fromCharCode(99);}};
                            }}}}
                        ];}}
                    "#
                    );
                    let program = compile(&source, "binary-coercion-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let atom = vm.intern_atom("operands");
                    let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                    let values = vm
                        .call_value(&program, factory, Value::UNDEFINED, &[])
                        .unwrap();
                    let (left, right) = match vm.heap.get(values) {
                        Some(super::Cell::Array { elements, .. }) => (elements[0], elements[1]),
                        _ => panic!("operands"),
                    };
                    let handles = [left, right].map(|value| vm.heap.weak_handle(value).unwrap());
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let result = vm.binary(&program, operator as u32, left, right);
                    assert_eq!(result.is_ok(), phase == "success", "{kind} {token} {phase}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    let result_handle = match result {
                        Ok(value) => {
                            let expected = match token {
                                "<" => {
                                    if kind == "bigint" {
                                        "false"
                                    } else {
                                        "true"
                                    }
                                }
                                ">=" => {
                                    if kind == "bigint" {
                                        "true"
                                    } else {
                                        "false"
                                    }
                                }
                                "+" => match kind {
                                    "bigint" => "49",
                                    "numeric-string" => "427",
                                    _ => "abc",
                                },
                                _ => {
                                    if kind == "string" {
                                        "NaN"
                                    } else {
                                        "35"
                                    }
                                }
                            };
                            assert_eq!(
                                vm.to_string(&program, value).unwrap(),
                                expected,
                                "{kind} {token}"
                            );
                            for (handle, value) in handles.iter().zip([left, right]) {
                                assert_eq!(vm.heap.weak_value(*handle), Some(value));
                            }
                            value.is_heap().then(|| vm.heap.weak_handle(value).unwrap())
                        }
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let kind = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                            None
                        }
                    };
                    vm.collect_now(&program);
                    for handle in handles.into_iter().chain(result_handle) {
                        assert!(
                            vm.heap.weak_value(handle).is_none(),
                            "{kind} {token} {phase}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn equality_roots_the_opposite_primitive_during_object_coercion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for bigint in [false, true] {
            for reversed in [false, true] {
                for throws in [false, true] {
                    let mut vm = Vm::new(Test262Host);
                    let source = format!(
                        r#"function operand() {{return {{[Symbol.toPrimitive]() {{$262.gc();if ({throws}) throw {{kind:'coercion'}};return {bigint}?BigInt(43):String.fromCharCode(97,99);}}}};}}"#
                    );
                    let program = compile(&source, "equality-coercion-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let atom = vm.intern_atom("operand");
                    let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                    let object = vm
                        .call_value(&program, factory, Value::UNDEFINED, &[])
                        .unwrap();
                    let primitive = vm.heap.alloc(if bigint {
                        super::Cell::BigInt("42".into())
                    } else {
                        super::Cell::String("ab".into())
                    });
                    let handles =
                        [object, primitive].map(|value| vm.heap.weak_handle(value).unwrap());
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let result = if reversed {
                        vm.equal(&program, object, primitive)
                    } else {
                        vm.equal(&program, primitive, object)
                    };
                    assert_eq!(
                        result.is_ok(),
                        !throws,
                        "bigint={bigint} reversed={reversed} throws={throws}"
                    );
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    match result {
                        Ok(value) => {
                            assert!(!value);
                            assert_eq!(
                                vm.to_string(&program, primitive).unwrap(),
                                if bigint { "42" } else { "ab" }
                            );
                            assert_eq!(vm.heap.weak_value(handles[1]), Some(primitive));
                        }
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let kind = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, kind).unwrap(), "coercion");
                        }
                    }
                    vm.collect_now(&program);
                    for handle in handles {
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn instanceof_roots_fresh_inputs_across_lookup_and_traversal() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for ordinary in [false, true] {
            for phase in ["success", "prototype", "traversal"] {
                let mut vm = Vm::new(Test262Host);
                let source = format!(
                    r#"
                    function operands() {{
                        function C() {{}}
                        return [new Proxy(Object.create(C.prototype), {{getPrototypeOf(target) {{
                            $262.gc(); if ('{phase}'==='traversal') throw {{kind:'traversal'}};
                            return Reflect.getPrototypeOf(target);
                        }}}}), new Proxy(C, {{get(target,key) {{
                            $262.gc();
                            if (key===Symbol.hasInstance) return null;
                            if (key==='prototype' && '{phase}'==='prototype') throw {{kind:'prototype'}};
                            return Reflect.get(target,key);
                        }}}})];
                    }}
                "#
                );
                let program = compile(&source, "instanceof-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let atom = vm.intern_atom("operands");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                let values = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let (value, constructor) = match vm.heap.get(values) {
                    Some(super::Cell::Array { elements, .. }) => (elements[0], elements[1]),
                    _ => panic!("operands"),
                };
                let handles = [value, constructor].map(|v| vm.heap.weak_handle(v).unwrap());
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let result = if ordinary {
                    vm.ordinary_has_instance(&program, constructor, value)
                } else {
                    vm.instanceof(&program, value, constructor)
                };
                assert_eq!(
                    result.is_ok(),
                    phase == "success",
                    "ordinary={ordinary} {phase}"
                );
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                match result {
                    Ok(result) => {
                        assert!(result, "ordinary={ordinary}");
                        for (handle, value) in handles.iter().zip([value, constructor]) {
                            assert_eq!(vm.heap.weak_value(*handle), Some(value));
                        }
                    }
                    Err(error) => {
                        let atom = vm.intern_atom("kind");
                        let kind = vm
                            .own_property(error.thrown_value().unwrap(), atom)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                    }
                }
                vm.collect_now(&program);
                for handle in handles {
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn instanceof_roots_detached_prototype_and_restores_cursor_after_throw() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for ordinary in [false, true] {
            for throws in [false, true] {
                let mut vm = Vm::new(Test262Host);
                let source = format!(
                    r#"
                    function operands() {{
                        function C() {{}}
                        Object.defineProperty(C,Symbol.hasInstance,{{value:null}});
                        return [new Proxy({{}},{{getPrototypeOf() {{
                            C.prototype={{}}; $262.gc();
                            return new Proxy({{}},{{getPrototypeOf() {{
                                $262.gc(); if ({throws}) throw {{kind:'cursor'}}; return null;
                            }}}});
                        }}}}),C,C.prototype];
                    }}
                "#
                );
                let program = compile(&source, "instanceof-detached-prototype.js").unwrap();
                vm.execute(&program).unwrap();
                let atom = vm.intern_atom("operands");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                let operands = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let values = match vm.heap.get(operands) {
                    Some(super::Cell::Array { elements, .. }) => {
                        [elements[0], elements[1], elements[2]]
                    }
                    _ => panic!("operands"),
                };
                let handles = values.map(|value| vm.heap.weak_handle(value).unwrap());
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let result = if ordinary {
                    vm.ordinary_has_instance(&program, values[1], values[0])
                } else {
                    vm.instanceof(&program, values[0], values[1])
                };
                assert_eq!(result.is_err(), throws);
                match result {
                    Ok(result) => assert!(!result),
                    Err(error) => {
                        let atom = vm.intern_atom("kind");
                        let kind = vm
                            .own_property(error.thrown_value().unwrap(), atom)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, kind).unwrap(), "cursor");
                    }
                }
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                for (handle, value) in handles.iter().zip(values) {
                    assert_eq!(vm.heap.weak_value(*handle), Some(value));
                }
                vm.collect_now(&program);
                for handle in handles {
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn iterator_step_roots_receiver_through_result_accessors() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for cached in [false, true] {
            for wrapped in [false, true] {
                for phase in ["value", "done", "done-throw", "value-throw", "invalid"] {
                    let mut vm = Vm::new(Test262Host);
                    let source = format!(
                        r#"
                    function step() {{if ('{phase}'==='invalid') return 42; return {{
                            get done() {{$262.gc(); if ('{phase}'==='done-throw') throw {{kind:'done-throw'}}; return '{phase}'==='done';}},
                            get value() {{$262.gc(); if ('{phase}'==='value-throw') throw {{kind:'value-throw'}}; return {{tag:42}};}}
                        }};}}
                    function operand() {{var iterator={{next() {{$262.gc();return step();}}}};return {wrapped}?Iterator.from(iterator):iterator;}}
                "#
                    );
                    let program = compile(&source, "iterator-step-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let atom = vm.intern_atom("operand");
                    let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                    let iterator = vm
                        .call_value(&program, factory, Value::UNDEFINED, &[])
                        .unwrap();
                    let handle = vm.heap.weak_handle(iterator).unwrap();
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let result = if cached {
                        let iterator_root = vm.heap.root(iterator);
                        let atom = vm.intern_atom("next");
                        let method = vm.get_property(&program, iterator, atom).unwrap();
                        let method_root = vm.heap.root(method);
                        let result =
                            vm.rooted_iterator_step_value(&program, iterator_root, method_root);
                        vm.heap.release_root(method_root);
                        vm.heap.release_root(iterator_root);
                        result
                    } else {
                        vm.iterator_step_value(&program, iterator)
                    };
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    assert_eq!(
                        vm.heap.weak_value(handle),
                        Some(iterator),
                        "cached={cached} {phase}"
                    );
                    let value_handle = match result {
                        Ok(Some(value)) => {
                            assert_eq!(phase, "value");
                            let atom = vm.intern_atom("tag");
                            assert_eq!(
                                vm.own_property(value, atom).unwrap().as_number(),
                                Some(42.0)
                            );
                            Some(vm.heap.weak_handle(value).unwrap())
                        }
                        Ok(None) => {
                            assert_eq!(phase, "done");
                            None
                        }
                        Err(error) => {
                            if phase == "invalid" {
                                assert!(error.to_string().contains("not an object"));
                            } else {
                                assert!(phase.ends_with("throw"));
                                let atom = vm.intern_atom("kind");
                                let kind = vm
                                    .own_property(error.thrown_value().unwrap(), atom)
                                    .unwrap();
                                assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                            }
                            None
                        }
                    };
                    vm.collect_now(&program);
                    for handle in std::iter::once(handle).chain(value_handle) {
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn suspended_owners_trace_complete_frame_and_request_state() {
    use super::activation::{AsyncGeneratorOperation, AsyncGeneratorRequest, GeneratorRecord};
    for completion in [
        Completion::Return,
        Completion::Throw,
        Completion::Yield,
        Completion::Await,
    ] {
        for generator in [false, true] {
            let mut vm = Vm::new(SilentHost);
            let program = Engine::specialize("", "suspended-owner-roots.js").unwrap();
            vm.initialize(&program).unwrap();
            let env = vm.object();
            let receiver = vm.object();
            let local = vm.object();
            let binding = vm.object();
            let register = vm.object();
            let iterator = vm.object();
            let completion_value = vm.object();
            let promise = vm.object();
            let realm = vm.object();
            let request_promise = vm.object();
            let request_value = vm.object();
            let frame_values = [
                env,
                receiver,
                local,
                binding,
                register,
                iterator,
                completion_value,
                promise,
            ];
            let owned = if generator {
                frame_values
                    .into_iter()
                    .chain([realm, request_promise, request_value])
                    .collect::<Vec<_>>()
            } else {
                frame_values.to_vec()
            };
            let handles = owned
                .iter()
                .map(|v| vm.heap.weak_handle(*v).unwrap())
                .collect::<Vec<_>>();
            let atom = vm.intern_atom("binding");
            let mut frame = super::Frame {
                program: super::program_store::ProgramId::MAIN,
                function: 0,
                pc: 0,
                env,
                this: receiver,
                locals: vec![local],
                dynamic_bindings: vec![(atom, binding)],
                captured: true,
                registers: vec![register, iterator, Value::FALSE],
                active_iterators: vec![super::ActiveIterator {
                    iterator: 1,
                    done: 2,
                }],
                with_base: 0,
            };
            let continuation = Continuation::from_frame(
                &mut frame,
                completion(completion_value),
                Some(0),
                promise,
            );
            assert!(
                frame.locals.is_empty()
                    && frame.dynamic_bindings.is_empty()
                    && frame.registers.is_empty()
                    && frame.active_iterators.is_empty()
            );
            let roots = vm.heap.root_count_for_test();
            let mut owner = None;
            let mut token = None;
            if generator {
                let value = vm.heap.alloc(super::Cell::Iterator {
                    object: Vm::<SilentHost>::empty_object(Value::NULL),
                    source: Value::NULL,
                    next_method: None,
                    helper: None,
                    helper_running: false,
                    helper_started: false,
                    kind: super::IteratorKind::AsyncGenerator,
                    index: 0,
                    done: false,
                    generator: Some(Box::new(GeneratorRecord {
                        continuation: Some(continuation),
                        realm,
                        done: false,
                        running: false,
                        requests: [AsyncGeneratorRequest {
                            operation: AsyncGeneratorOperation::Next,
                            promise: request_promise,
                            value: request_value,
                        }]
                        .into(),
                    })),
                });
                owner = Some(vm.heap.root(value));
            } else {
                token = Some(vm.suspend_continuation(continuation));
            }
            vm.collect_now(&program);
            for (handle, value) in handles.iter().zip(&owned) {
                assert_eq!(
                    vm.heap.weak_value(*handle),
                    Some(*value),
                    "generator={generator} completion={:?}",
                    completion(completion_value)
                );
            }
            let resumed = if let Some(token) = token {
                let continuation = vm.resume_continuation(token).unwrap();
                assert!(vm.resume_continuation(token).is_none());
                continuation
            } else {
                let value = vm.heap.root_value(owner.unwrap()).unwrap();
                match vm.heap.get_mut(value) {
                    Some(super::Cell::Iterator {
                        generator: Some(record),
                        ..
                    }) => record.continuation.take().unwrap(),
                    _ => panic!("generator owner"),
                }
            };
            assert_eq!(resumed.completion, completion(completion_value));
            assert_eq!(resumed.promise, promise);
            let resumed = resumed.into_frame(0);
            assert_eq!(resumed.locals, [local]);
            assert_eq!(resumed.dynamic_bindings, [(atom, binding)]);
            assert_eq!(resumed.registers, [register, iterator, Value::FALSE]);
            assert_eq!(
                resumed.active_iterators,
                [super::ActiveIterator {
                    iterator: 1,
                    done: 2
                }]
            );
            if let Some(owner) = owner {
                vm.heap.release_root(owner);
            }
            assert_eq!(vm.heap.root_count_for_test(), roots);
            vm.collect_now(&program);
            for handle in handles {
                assert!(vm.heap.weak_value(handle).is_none());
            }
        }
    }
}

#[test]
fn array_iterator_advance_roots_owner_and_applies_index_transition() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for native in [Native::ArrayKeys, Native::ArrayValues, Native::ArrayEntries] {
            for phase in ["value", "length", "item", "done"] {
                let mut vm = Vm::new(Test262Host);
                let source = format!(
                    r#"function source() {{return {{
                    get length() {{$262.gc(); if ('{phase}'==='length') throw {{kind:'length'}}; return '{phase}'==='done'?0:2;}},
                    get 0() {{$262.gc(); if ('{phase}'==='item') throw {{kind:'item'}};return {{tag:42}};}}
                }};}}"#
                );
                let program = compile(&source, "array-iterator-advance.js").unwrap();
                vm.execute(&program).unwrap();
                let atom = vm.intern_atom("source");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                let source = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let iterator = vm.array_iterator_native(&program, native, source).unwrap();
                let handles = [iterator, source].map(|v| vm.heap.weak_handle(v).unwrap());
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let result = vm.iterator_next(&program, iterator);
                let throws = phase == "length" || (phase == "item" && native != Native::ArrayKeys);
                assert_eq!(result.is_err(), throws);
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                assert_eq!(
                    vm.heap.weak_value(handles[0]),
                    Some(iterator),
                    "{native:?} {phase}"
                );
                match result {
                    Ok(result) => {
                        let atom = vm.intern_atom("done");
                        assert_eq!(
                            vm.own_property(result, atom),
                            Some(if phase == "done" {
                                Value::TRUE
                            } else {
                                Value::FALSE
                            })
                        );
                        if phase != "done" {
                            let atom = vm.intern_atom("value");
                            let mut value = vm.own_property(result, atom).unwrap();
                            if native == Native::ArrayEntries {
                                value = match vm.heap.get(value) {
                                    Some(super::Cell::Array { elements, .. }) => {
                                        assert_eq!(elements[0].as_number(), Some(0.0));
                                        elements[1]
                                    }
                                    _ => panic!("entry"),
                                };
                            }
                            if native == Native::ArrayKeys {
                                assert_eq!(value.as_number(), Some(0.0));
                            } else {
                                let atom = vm.intern_atom("tag");
                                assert_eq!(
                                    vm.own_property(value, atom).unwrap().as_number(),
                                    Some(42.0)
                                );
                            }
                        }
                    }
                    Err(error) => {
                        let atom = vm.intern_atom("kind");
                        let kind = vm
                            .own_property(error.thrown_value().unwrap(), atom)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                    }
                }
                match vm.heap.get(iterator) {
                    Some(super::Cell::Iterator {
                        index,
                        done,
                        source: owner,
                        ..
                    }) => {
                        assert_eq!(
                            *index,
                            if phase == "length" || phase == "done" {
                                0
                            } else {
                                1
                            }
                        );
                        assert_eq!(*done, phase == "done");
                        assert_eq!(
                            *owner,
                            if phase == "done" {
                                Value::UNDEFINED
                            } else {
                                source
                            }
                        );
                    }
                    _ => panic!("iterator"),
                }
                let owner = vm.heap.root(iterator);
                vm.collect_now(&program);
                assert_eq!(vm.heap.weak_value(handles[1]).is_none(), phase == "done");
                vm.heap.release_root(owner);
                vm.collect_now(&program);
                for handle in handles {
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn array_iterator_retains_captured_source_after_reentrant_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        let mut vm = Vm::new(Test262Host);
        let program = compile(
            r#"
            var iterator,nested=false;
            function lengthValue() {return {[Symbol.toPrimitive]() {
                nested=true;iterator.next();$262.gc();return 1;
            }};}
            function source() {return {
                get length() {return nested?0:lengthValue();},
                get 0() {$262.gc();return {tag:42};}
            };}
        "#,
            "array-iterator-captured-source.js",
        )
        .unwrap();
        vm.execute(&program).unwrap();
        let atom = vm.intern_atom("source");
        let factory = vm.own_property(vm.realm.globals, atom).unwrap();
        let source = vm
            .call_value(&program, factory, Value::UNDEFINED, &[])
            .unwrap();
        let iterator = vm
            .array_iterator_native(&program, Native::ArrayValues, source)
            .unwrap();
        let atom = vm.intern_atom("iterator");
        vm.set_property(vm.realm.globals, atom, iterator).unwrap();
        let handles = [source, iterator].map(|v| vm.heap.weak_handle(v).unwrap());
        let roots = vm.heap.root_count_for_test();
        let calls = vm.active_call_roots.len();
        let result = vm.iterator_next(&program, iterator).unwrap();
        let value_atom = vm.intern_atom("value");
        let value = vm.own_property(result, value_atom).unwrap();
        let tag = vm.intern_atom("tag");
        assert_eq!(vm.own_property(value, tag).unwrap().as_number(), Some(42.0));
        assert_eq!(vm.heap.weak_value(handles[0]), Some(source));
        assert_eq!(vm.heap.root_count_for_test(), roots);
        assert_eq!(vm.active_call_roots.len(), calls);
        assert!(
            matches!(vm.heap.get(iterator),Some(super::Cell::Iterator {source,index:1,done:true,..}) if source.is_undefined())
        );
        vm.collect_now(&program);
        assert!(vm.heap.weak_value(handles[0]).is_none());
        assert_eq!(vm.heap.weak_value(handles[1]), Some(iterator));
        vm.set_property(vm.realm.globals, atom, Value::UNDEFINED)
            .unwrap();
        vm.collect_now(&program);
        assert!(vm.heap.weak_value(handles[1]).is_none());
    }
}

#[test]
fn completed_typed_array_iterators_release_source_and_backing() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for native in [
            Native::Uint8ArrayKeys,
            Native::Uint8ArrayValues,
            Native::Uint8ArrayEntries,
        ] {
            let mut vm = Vm::new(Test262Host);
            let program = compile(
                "function source() {return new Uint8Array([42]);}",
                "typed-iterator-release.js",
            )
            .unwrap();
            vm.execute(&program).unwrap();
            let atom = vm.intern_atom("source");
            let factory = vm.own_property(vm.realm.globals, atom).unwrap();
            let source = vm
                .call_value(&program, factory, Value::UNDEFINED, &[])
                .unwrap();
            let buffer = match vm.heap.get(source) {
                Some(super::Cell::TypedArray { buffer, .. }) => *buffer,
                _ => panic!("typed source"),
            };
            let iterator = vm.array_iterator_native(&program, native, source).unwrap();
            let handles = [iterator, source, buffer].map(|v| vm.heap.weak_handle(v).unwrap());
            let owner = vm.heap.root(iterator);
            vm.collect_now(&program);
            for (handle, value) in handles.iter().zip([iterator, source, buffer]) {
                assert_eq!(vm.heap.weak_value(*handle), Some(value));
            }
            vm.iterator_next(&program, iterator).unwrap();
            let result = vm.iterator_next(&program, iterator).unwrap();
            let atom = vm.intern_atom("done");
            assert_eq!(vm.own_property(result, atom), Some(Value::TRUE));
            vm.collect_now(&program);
            assert_eq!(vm.heap.weak_value(handles[0]), Some(iterator));
            assert!(vm.heap.weak_value(handles[1]).is_none());
            assert!(vm.heap.weak_value(handles[2]).is_none());
            let result = vm.iterator_next(&program, iterator).unwrap();
            assert_eq!(vm.own_property(result, atom), Some(Value::TRUE));
            vm.heap.release_root(owner);
            vm.collect_now(&program);
            assert!(vm.heap.weak_value(handles[0]).is_none());
        }
    }
}

#[test]
fn iterator_close_retains_forwarded_wrappers_and_restores_scopes() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for wrapper in ["raw", "protocol", "async-from-sync"] {
            for phase in [
                "success",
                "getter",
                "call",
                "primitive",
                "method",
                "absent",
                "null",
            ] {
                let mut vm = Vm::new(Test262Host);
                let source = format!(
                    r#"
                    var log=[];function readLog() {{return log.join(',');}}
                    function close() {{log.push('call:'+this.tag);$262.gc();if ('{phase}'==='call') throw {{kind:'call'}};
                        return '{phase}'==='primitive'?42:{{tag:this.tag}};
                    }}
                    function source() {{return {{tag:42,next() {{return {{done:true}};}},get return() {{
                        log.push('get:'+this.tag);$262.gc();if ('{phase}'==='getter') throw {{kind:'getter'}};
                        return '{phase}'==='absent'?undefined:'{phase}'==='null'?null:'{phase}'==='method'?42:close;
                    }}}};}}
                "#
                );
                let program = compile(&source, "iterator-close-ownership.js").unwrap();
                vm.execute(&program).unwrap();
                let atom = vm.intern_atom("source");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                let source = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let iterator = match wrapper {
                    "raw" => source,
                    "protocol" => vm.iterator_from(&program, &[source]).unwrap(),
                    _ => vm.heap.alloc(super::Cell::Iterator {
                        object: Vm::<Test262Host>::empty_object(vm.async_from_sync_iterator_proto),
                        source,
                        next_method: None,
                        helper: None,
                        helper_running: false,
                        helper_started: false,
                        kind: super::IteratorKind::AsyncFromSync,
                        index: 0,
                        done: false,
                        generator: None,
                    }),
                };
                let handles = [source, iterator].map(|v| vm.heap.weak_handle(v).unwrap());
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let result = vm.iterator_close(&program, iterator);

                assert_eq!(
                    result.is_ok(),
                    matches!(phase, "success" | "absent" | "null")
                );
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                for (handle, value) in handles.iter().zip([source, iterator]) {
                    assert_eq!(
                        vm.heap.weak_value(*handle),
                        Some(value),
                        "{wrapper} {phase}"
                    );
                }
                let log = vm.with_call_roots(
                    result
                        .iter()
                        .copied()
                        .chain(result.as_ref().err().and_then(|error| error.thrown_value())),
                    |vm| {
                        let atom = vm.intern_atom("readLog");
                        let reader = vm.own_property(vm.realm.globals, atom).unwrap();
                        let log = vm
                            .call_value(&program, reader, Value::UNDEFINED, &[])
                            .unwrap();
                        vm.to_string(&program, log).unwrap()
                    },
                );
                assert_eq!(
                    log,
                    if matches!(phase, "success" | "call" | "primitive") {
                        "get:42,call:42"
                    } else {
                        "get:42"
                    },
                    "{wrapper} {phase}"
                );
                let result_handle = match result {
                    Ok(result) if phase == "success" => {
                        let atom = vm.intern_atom("tag");
                        assert_eq!(
                            vm.own_property(result, atom).unwrap().as_number(),
                            Some(42.0)
                        );
                        Some(vm.heap.weak_handle(result).unwrap())
                    }
                    Ok(result) => {
                        assert!(result.is_undefined());
                        None
                    }
                    Err(error) if matches!(phase, "getter" | "call") => {
                        let atom = vm.intern_atom("kind");
                        let kind = vm
                            .own_property(error.thrown_value().unwrap(), atom)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                        None
                    }
                    Err(error) => {
                        assert!(error.to_string().contains(if phase == "method" {
                            "not callable"
                        } else {
                            "not an object"
                        }));
                        None
                    }
                };
                vm.collect_now(&program);
                for handle in handles.into_iter().chain(result_handle) {
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn regexp_match_all_creation_roots_fresh_species_matcher_and_inputs() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in ["success", "flags", "construct", "index", "setter"] {
            let mut vm = Vm::new(Test262Host);
            let source = format!(
                r#"
                function speciesFactory() {{return function Species() {{$262.gc();if ('{phase}'==='construct') throw {{kind:'construct'}};return matcherFactory();}};}}
                function matcherFactory() {{return {{exec() {{return null;}},set lastIndex(value) {{$262.gc();if ('{phase}'==='setter') throw {{kind:'setter'}};}}}};}}
                function source() {{return {{
                    get constructor() {{$262.gc();return {{[Symbol.species]:speciesFactory()}};}},
                    get flags() {{$262.gc();if ('{phase}'==='flags') throw {{kind:'flags'}};return 'g';}},
                    get lastIndex() {{$262.gc();if ('{phase}'==='index') throw {{kind:'index'}};return {{[Symbol.toPrimitive]() {{$262.gc();return 0;}}}};}}
                }};}}
            "#
            );
            let program = compile(&source, "match-all-creation-roots.js").unwrap();
            vm.execute(&program).unwrap();
            let atom = vm.intern_atom("source");
            let factory = vm.own_property(vm.realm.globals, atom).unwrap();
            let receiver = vm
                .call_value(&program, factory, Value::UNDEFINED, &[])
                .unwrap();
            let input = vm.heap.alloc(super::Cell::String("a".into()));
            let handles = [receiver, input].map(|v| vm.heap.weak_handle(v).unwrap());
            let roots = vm.heap.root_count_for_test();
            let calls = vm.active_call_roots.len();
            let result = vm.regexp_symbol_match_all(&program, receiver, &[input]);
            assert_eq!(result.is_ok(), phase == "success", "{phase}");
            assert_eq!(vm.heap.root_count_for_test(), roots);
            assert_eq!(vm.active_call_roots.len(), calls);
            for (handle, value) in handles.iter().zip([receiver, input]) {
                assert_eq!(vm.heap.weak_value(*handle), Some(value), "{phase}");
            }
            match result {
                Ok(iterator) => {
                    let matcher = match vm.heap.get(iterator) {
                        Some(super::Cell::Iterator { source, .. }) => *source,
                        _ => panic!("regexp iterator"),
                    };
                    assert!(vm.is_object_like(matcher));
                    let matcher_handle = vm.heap.weak_handle(matcher).unwrap();
                    let owner = vm.heap.root(iterator);
                    vm.collect_now(&program);
                    assert_eq!(vm.heap.weak_value(matcher_handle), Some(matcher));
                    for handle in handles {
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                    vm.heap.release_root(owner);
                    vm.collect_now(&program);
                    assert!(vm.heap.weak_value(matcher_handle).is_none());
                }
                Err(error) => {
                    let atom = vm.intern_atom("kind");
                    let kind = vm
                        .own_property(error.thrown_value().unwrap(), atom)
                        .unwrap();
                    assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                    vm.collect_now(&program);
                    for handle in handles {
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn regexp_iterator_advance_roots_fresh_exec_result_and_restores_scopes() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (full, phases) in [
            (false, &["success"][..]),
            (
                true,
                &[
                    "success", "exec", "match", "coercion", "index", "numeric", "setter", "done",
                ][..],
            ),
        ] {
            for &phase in phases {
                let mut vm = Vm::new(Test262Host);
                let input_code = if full { 55296 } else { 97 };
                let source = format!(
                    r#"
                function execute(input) {{$262.gc();if (input.charCodeAt(0)!=={input_code}) throw {{kind:'input'}};return '{phase}'==='done'?null:resultFactory();}}
                function resultFactory() {{return {{tag:42,get 0() {{$262.gc();if ('{phase}'==='match') throw {{kind:'match'}};
                    return {full}?{{[Symbol.toPrimitive]() {{$262.gc();if ('{phase}'==='coercion') throw {{kind:'coercion'}};return '';}}}}:'';
                }}}};}}
                function numeric() {{return {{[Symbol.toPrimitive]() {{$262.gc();if ('{phase}'==='numeric') throw {{kind:'numeric'}};return 0;}}}};}}
                function matcher() {{if (!{full}) return {{exec:execute,lastIndex:{{valueOf() {{$262.gc();return 0;}}}}}};return {{
                    get exec() {{$262.gc();if ('{phase}'==='exec') throw {{kind:'exec'}};return execute;}},
                    get lastIndex() {{$262.gc();if ('{phase}'==='index') throw {{kind:'index'}};return numeric();}},
                    set lastIndex(value) {{$262.gc();if ('{phase}'==='setter') throw {{kind:'setter'}};}}
                }};}}
            "#
                );
                let program = compile(&source, "regexp-iterator-advance-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let atom = vm.intern_atom("matcher");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                let matcher = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let iterator = vm.heap.alloc(super::Cell::Iterator {
                    object: Vm::<Test262Host>::empty_object(vm.regexp_string_iterator_proto),
                    source: matcher,
                    next_method: None,
                    helper: Some(Box::new(
                        crate::heap::IteratorHelper::RegExpStringMatchAll {
                            input: JsString::from_units(if full { &[0xd800, 97] } else { &[97] }),
                            global: true,
                            unicode: true,
                        },
                    )),
                    helper_running: false,
                    helper_started: false,
                    kind: super::IteratorKind::RegExpStringMatchAll,
                    index: 0,
                    done: false,
                    generator: None,
                });
                let handles = [matcher, iterator].map(|v| vm.heap.weak_handle(v).unwrap());
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let result = vm.iterator_next(&program, iterator);
                assert_eq!(
                    result.is_ok(),
                    matches!(phase, "success" | "done"),
                    "{phase}"
                );
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                let result_handle = match result {
                    Ok(result) => {
                        let done = vm.intern_atom("done");
                        assert_eq!(
                            vm.own_property(result, done),
                            Some(if phase == "done" {
                                Value::TRUE
                            } else {
                                Value::FALSE
                            })
                        );
                        if phase != "done" {
                            let atom = vm.intern_atom("value");
                            let matched = vm.own_property(result, atom).unwrap();
                            let tag = vm.intern_atom("tag");
                            assert_eq!(
                                vm.own_property(matched, tag).unwrap().as_number(),
                                Some(42.0)
                            );
                        }
                        Some(vm.heap.weak_handle(result).unwrap())
                    }
                    Err(error) => {
                        let atom = vm.intern_atom("kind");
                        let kind = vm
                            .own_property(error.thrown_value().unwrap(), atom)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                        None
                    }
                };
                for (handle, value) in handles.iter().zip([matcher, iterator]) {
                    assert_eq!(vm.heap.weak_value(*handle), Some(value));
                }
                assert!(
                    matches!(vm.heap.get(iterator),Some(super::Cell::Iterator {done,..}) if *done==(phase=="done"))
                );
                vm.collect_now(&program);
                for handle in handles.into_iter().chain(result_handle) {
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn regexp_split_roots_inputs_matcher_results_and_accumulated_captures() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in [
            "limit",
            "set",
            "exec-get",
            "exec",
            "index",
            "index-number",
            "length",
            "length-number",
            "capture1",
            "capture2",
            "second-exec",
        ] {
            for abrupt in [false, true] {
                let mut vm = Vm::new(Test262Host);
                let source = format!(
                    r#"
                    function hit(name) {{if (name==='{phase}') {{$262.gc();if ({abrupt}) throw {{kind:name}};}}}}
                    function capture(tag) {{return {{tag:tag}};}}
                    function result() {{return {{
                        get length() {{hit('length');return {{valueOf() {{hit('length-number');return 3;}}}};}},
                        get 1() {{hit('capture1');return capture(42);}},
                        get 2() {{hit('capture2');return capture(43);}}
                    }};}}
                    function matcher() {{var index=0,matched=0;return {{
                        set lastIndex(value) {{hit('set');index=value;}},
                        get lastIndex() {{hit('index');return {{valueOf() {{hit('index-number');return index;}}}};}},
                        get exec() {{hit('exec-get');return function(input) {{hit('exec');if (input!=='a,b,c') throw {{kind:'input'}};if (index===1||index===3) {{if (matched++) hit('second-exec');index++;return result();}}return null;}};}}
                    }};}}
                    function receiver() {{return {{constructor:{{[Symbol.species]:function Species() {{return matcher();}}}},flags:'g'}};}}
                    function limit() {{return {{valueOf() {{hit('limit');return 100;}}}};}}
                "#
                );
                let program = compile(&source, "regexp-split-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let factory = vm.intern_atom("receiver");
                let factory = vm.own_property(vm.realm.globals, factory).unwrap();
                let receiver = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let input = vm.heap.alloc(super::Cell::String("a,b,c".into()));
                let receiver_root = vm.heap.root(receiver);
                let input_root = vm.heap.root(input);
                let factory = vm.intern_atom("limit");
                let factory = vm.own_property(vm.realm.globals, factory).unwrap();
                let limit = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                vm.heap.release_root(receiver_root);
                vm.heap.release_root(input_root);
                let handles = [receiver, input, limit].map(|v| vm.heap.weak_handle(v).unwrap());
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let outcome = vm.regexp_symbol_split(&program, receiver, &[input, limit]);
                assert_eq!(outcome.is_ok(), !abrupt, "{phase}/{abrupt}");
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                for (handle, value) in handles.iter().zip([receiver, input, limit]) {
                    assert_eq!(vm.heap.weak_value(*handle), Some(value), "{phase}/{abrupt}");
                }
                match outcome {
                    Ok(array) => {
                        let array_root = vm.heap.root(array);
                        vm.collect_now(&program);
                        for (index, expected) in [(0, "a"), (3, "b"), (6, "c")] {
                            let value = vm
                                .get_index(&program, array, Value::number(index as f64))
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), expected, "{phase}");
                        }
                        for (index, expected) in [(1, 42.0), (2, 43.0), (4, 42.0), (5, 43.0)] {
                            let value = vm
                                .get_index(&program, array, Value::number(index as f64))
                                .unwrap();
                            let tag = vm.intern_atom("tag");
                            assert_eq!(
                                vm.own_property(value, tag).and_then(Value::as_number),
                                Some(expected),
                                "{phase}/{index}"
                            );
                        }
                        vm.heap.release_root(array_root);
                    }
                    Err(error) => {
                        let kind = vm.intern_atom("kind");
                        let kind = vm
                            .own_property(error.thrown_value().unwrap(), kind)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                    }
                }
                vm.collect_now(&program);
                for handle in handles {
                    assert!(vm.heap.weak_value(handle).is_none(), "{phase}/{abrupt}");
                }
            }
        }
    }
}

#[test]
fn regexp_search_roots_saved_index_and_fresh_result_across_callbacks() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in [
            "input", "previous", "reset", "exec-get", "exec", "current", "restore", "index",
        ] {
            for abrupt in [false, true] {
                let mut vm = Vm::new(Test262Host);
                let source = format!(
                    r#"
                    function hit(name) {{if(name==='{phase}') {{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                    function previous() {{return {{tag:41,valueOf() {{throw {{kind:'coerced-previous'}};}}}};}}
                    function result() {{return {{get index() {{hit('index');return 43;}}}};}}
                    function receiver() {{var reads=0;return {{
                        get lastIndex() {{if(reads++===0) {{hit('previous');return previous();}}hit('current');return 1;}},
                        set lastIndex(value) {{if(value===0)hit('reset');else {{hit('restore');if(value.tag!==41)throw {{kind:'previous-value'}};}}}},
                        get exec() {{hit('exec-get');return function(input) {{hit('exec');if(input!=='a')throw {{kind:'input-value'}};return result();}};}}
                    }};}}
                    function input() {{return {{[Symbol.toPrimitive](hint) {{hit('input');if(hint!=='string')throw {{kind:'input-hint'}};return 'a';}}}};}}
                "#
                );
                let program = compile(&source, "regexp-search-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let factory = vm.intern_atom("receiver");
                let factory = vm.own_property(vm.realm.globals, factory).unwrap();
                let receiver = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let receiver_root = vm.heap.root(receiver);
                let factory = vm.intern_atom("input");
                let factory = vm.own_property(vm.realm.globals, factory).unwrap();
                let input = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                vm.heap.release_root(receiver_root);
                let handles = [receiver, input].map(|v| vm.heap.weak_handle(v).unwrap());
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let outcome = vm.regexp_symbol_search(&program, receiver, &[input]);
                assert_eq!(outcome.is_ok(), !abrupt, "{phase}/{abrupt}");
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                for (handle, value) in handles.iter().zip([receiver, input]) {
                    assert_eq!(vm.heap.weak_value(*handle), Some(value), "{phase}/{abrupt}");
                }
                match outcome {
                    Ok(value) => assert_eq!(value.as_number(), Some(43.0), "{phase}"),
                    Err(error) => {
                        let kind = vm.intern_atom("kind");
                        let value = vm
                            .own_property(error.thrown_value().unwrap(), kind)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, value).unwrap(), phase);
                    }
                }
                vm.collect_now(&program);
                for handle in handles {
                    assert!(vm.heap.weak_value(handle).is_none(), "{phase}/{abrupt}");
                }
            }
        }
    }
}

#[test]
fn regexp_match_roots_input_and_accumulated_strings_across_callbacks() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in [
            "input",
            "flags",
            "flags-string",
            "reset",
            "exec-get",
            "exec",
            "match",
            "match-string",
            "index",
            "index-number",
            "advance",
            "second-exec",
        ] {
            for abrupt in [false, true] {
                let mut vm = Vm::new(Test262Host);
                let source = format!(
                    r#"
                    function hit(name) {{if(name==='{phase}') {{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                    function result(text) {{return {{get 0() {{hit('match');return {{[Symbol.toPrimitive](hint) {{hit('match-string');if(hint!=='string')throw {{kind:'match-hint'}};return text;}}}};}}}};}}
                    function receiver() {{var index=0,writes=0,calls=0;return {{
                        get flags() {{hit('flags');return {{[Symbol.toPrimitive](hint) {{hit('flags-string');if(hint!=='string')throw {{kind:'flags-hint'}};return 'g';}}}};}},
                        set lastIndex(value) {{hit(writes++===0?'reset':'advance');index=value;}},
                        get lastIndex() {{hit('index');return {{[Symbol.toPrimitive](hint) {{hit('index-number');if(hint!=='number')throw {{kind:'index-hint'}};return index;}}}};}},
                        get exec() {{hit('exec-get');return function(input) {{hit('exec');if(input!=='a')throw {{kind:'input-value'}};calls++;if(calls===4) {{hit('second-exec');return null;}}return result(calls===1?'A':calls===2?'':'B');}};}}
                    }};}}
                    function input() {{return {{[Symbol.toPrimitive](hint) {{hit('input');if(hint!=='string')throw {{kind:'input-hint'}};return 'a';}}}};}}
                "#
                );
                let program = compile(&source, "regexp-match-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let factory = vm.intern_atom("receiver");
                let factory = vm.own_property(vm.realm.globals, factory).unwrap();
                let receiver = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let receiver_root = vm.heap.root(receiver);
                let factory = vm.intern_atom("input");
                let factory = vm.own_property(vm.realm.globals, factory).unwrap();
                let input = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                vm.heap.release_root(receiver_root);
                let handles = [receiver, input].map(|v| vm.heap.weak_handle(v).unwrap());
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let outcome = vm.regexp_symbol_match(&program, receiver, &[input]);
                assert_eq!(outcome.is_ok(), !abrupt, "{phase}/{abrupt}");
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                for (handle, value) in handles.iter().zip([receiver, input]) {
                    assert_eq!(vm.heap.weak_value(*handle), Some(value), "{phase}/{abrupt}");
                }
                match outcome {
                    Ok(array) => {
                        let root = vm.heap.root(array);
                        vm.collect_now(&program);
                        for (index, text) in ["A", "", "B"].iter().enumerate() {
                            let value = vm
                                .get_index(&program, array, Value::number(index as f64))
                                .unwrap();
                            assert_eq!(
                                vm.to_string(&program, value).unwrap(),
                                *text,
                                "{phase}/{index}"
                            );
                        }
                        vm.heap.release_root(root);
                    }
                    Err(error) => {
                        let kind = vm.intern_atom("kind");
                        let value = vm
                            .own_property(error.thrown_value().unwrap(), kind)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, value).unwrap(), phase);
                    }
                }
                vm.collect_now(&program);
                for handle in handles {
                    assert!(vm.heap.weak_value(handle).is_none(), "{phase}/{abrupt}");
                }
            }
        }
    }
}

#[test]
fn string_match_search_custom_dispatch_roots_original_inputs() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (native, name) in [
            (super::Native::StringMatch, "match"),
            (super::Native::StringSearch, "search"),
        ] {
            for phase in ["get", "call"] {
                for abrupt in [false, true] {
                    let mut vm = Vm::new(Test262Host);
                    let source = format!(
                        r#"
                        function hit(name) {{if(name==='{phase}') {{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                        function selected() {{return function(arg) {{hit('call');if(this.tag!==43||arg.tag!==42)throw {{kind:'identity'}};return {{tag:44,argument:arg,receiver:this}};}};}}
                        function receiver() {{return {{tag:42,[Symbol.toPrimitive]() {{throw {{kind:'coerced'}};}}}};}}
                        function pattern() {{return {{tag:43,get [Symbol.{name}]() {{hit('get');return selected();}}}};}}
                    "#
                    );
                    let program = compile(&source, "string-custom-dispatch-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let factory = vm.intern_atom("receiver");
                    let factory = vm.own_property(vm.realm.globals, factory).unwrap();
                    let receiver = vm
                        .call_value(&program, factory, Value::UNDEFINED, &[])
                        .unwrap();
                    let root = vm.heap.root(receiver);
                    let factory = vm.intern_atom("pattern");
                    let factory = vm.own_property(vm.realm.globals, factory).unwrap();
                    let pattern = vm
                        .call_value(&program, factory, Value::UNDEFINED, &[])
                        .unwrap();
                    vm.heap.release_root(root);
                    let handles = [receiver, pattern].map(|v| vm.heap.weak_handle(v).unwrap());
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let outcome =
                        vm.string_match_or_search_native(&program, native, receiver, &[pattern]);
                    assert_eq!(outcome.is_ok(), !abrupt, "{name}/{phase}/{abrupt}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    for (handle, value) in handles.iter().zip([receiver, pattern]) {
                        assert_eq!(vm.heap.weak_value(*handle), Some(value));
                    }
                    match outcome {
                        Ok(value) => {
                            let owner = vm.heap.root(value);
                            vm.collect_now(&program);
                            for (key, expected) in [("argument", receiver), ("receiver", pattern)] {
                                let atom = vm.intern_atom(key);
                                assert_eq!(vm.own_property(value, atom), Some(expected));
                            }
                            vm.heap.release_root(owner);
                        }
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let value = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), phase);
                        }
                    }
                    vm.collect_now(&program);
                    for handle in handles {
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn string_match_search_fallback_roots_converted_input_and_fresh_matcher() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (native, name) in [
            (super::Native::StringMatch, "match"),
            (super::Native::StringSearch, "search"),
        ] {
            for phase in ["input", "pattern", "method", "call"] {
                for abrupt in [false, true] {
                    let mut vm = Vm::new(Test262Host);
                    let source = format!(
                        r#"
                        function hit(name) {{if(name==='{phase}') {{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                        function selected() {{return function(input) {{hit('call');if(input.charCodeAt(0)!==55296||this.source!=='a')throw {{kind:'values'}};return {{tag:44,matcher:this,input:input}};}};}}
                        Object.defineProperty(RegExp.prototype,Symbol.{name},{{configurable:true,get() {{hit('method');return selected();}}}});
                        function receiver() {{return {{[Symbol.toPrimitive](hint) {{hit('input');if(hint!=='string')throw {{kind:'input-hint'}};return String.fromCharCode(55296,97);}}}};}}
                        function pattern() {{return {{[Symbol.{name}]:null,[Symbol.toPrimitive](hint) {{hit('pattern');if(hint!=='string')throw {{kind:'pattern-hint'}};return 'a';}}}};}}
                    "#
                    );
                    let program = compile(&source, "string-fallback-dispatch-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let factory = vm.intern_atom("receiver");
                    let factory = vm.own_property(vm.realm.globals, factory).unwrap();
                    let receiver = vm
                        .call_value(&program, factory, Value::UNDEFINED, &[])
                        .unwrap();
                    let root = vm.heap.root(receiver);
                    let factory = vm.intern_atom("pattern");
                    let factory = vm.own_property(vm.realm.globals, factory).unwrap();
                    let pattern = vm
                        .call_value(&program, factory, Value::UNDEFINED, &[])
                        .unwrap();
                    vm.heap.release_root(root);
                    let handles = [receiver, pattern].map(|v| vm.heap.weak_handle(v).unwrap());
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let outcome =
                        vm.string_match_or_search_native(&program, native, receiver, &[pattern]);
                    assert_eq!(outcome.is_ok(), !abrupt, "{name}/{phase}/{abrupt}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    for (handle, value) in handles.iter().zip([receiver, pattern]) {
                        assert_eq!(vm.heap.weak_value(*handle), Some(value));
                    }
                    match outcome {
                        Ok(value) => {
                            let owner = vm.heap.root(value);
                            vm.collect_now(&program);
                            let atom = vm.intern_atom("matcher");
                            let matcher = vm.own_property(value, atom).unwrap();
                            assert!(
                                matches!(vm.heap.get(matcher),Some(super::Cell::RegExp{source,..})if source.host_string()=="a")
                            );
                            let atom = vm.intern_atom("input");
                            let input = vm.own_property(value, atom).unwrap();
                            assert!(
                                matches!(vm.heap.get(input),Some(super::Cell::String(text))if text.units()==[0xd800,97])
                            );
                            vm.heap.release_root(owner);
                        }
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let value = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), phase);
                        }
                    }
                    vm.collect_now(&program);
                    for handle in handles {
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn string_match_all_custom_dispatch_roots_flags_and_original_receiver() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in ["is-regexp", "flags", "flags-string", "method", "call"] {
            for abrupt in [false, true] {
                let mut vm = Vm::new(Test262Host);
                let source = format!(
                    r#"
                    function hit(name) {{if(name==='{phase}') {{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                    function selected() {{return function(arg) {{hit('call');if(this.tag!==43||arg.tag!==42)throw {{kind:'identity'}};return {{tag:44,argument:arg,receiver:this}};}};}}
                    function receiver() {{return {{tag:42,[Symbol.toPrimitive]() {{throw {{kind:'coerced'}};}}}};}}
                    function pattern() {{return {{tag:43,
                        get [Symbol.match]() {{hit('is-regexp');return true;}},
                        get flags() {{hit('flags');return {{[Symbol.toPrimitive](hint) {{hit('flags-string');if(hint!=='string')throw {{kind:'flags-hint'}};return 'g';}}}};}},
                        get [Symbol.matchAll]() {{hit('method');return selected();}}
                    }};}}
                "#
                );
                let program = compile(&source, "string-match-all-custom-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let atom = vm.intern_atom("receiver");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                let receiver = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let root = vm.heap.root(receiver);
                let atom = vm.intern_atom("pattern");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                let pattern = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                vm.heap.release_root(root);
                let handles = [receiver, pattern].map(|v| vm.heap.weak_handle(v).unwrap());
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let outcome = vm.string_match_all_native(&program, receiver, &[pattern]);
                assert_eq!(outcome.is_ok(), !abrupt, "{phase}/{abrupt}");
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                for (handle, value) in handles.iter().zip([receiver, pattern]) {
                    assert_eq!(vm.heap.weak_value(*handle), Some(value));
                }
                match outcome {
                    Ok(value) => {
                        let root = vm.heap.root(value);
                        vm.collect_now(&program);
                        for (key, expected) in [("argument", receiver), ("receiver", pattern)] {
                            let atom = vm.intern_atom(key);
                            assert_eq!(vm.own_property(value, atom), Some(expected));
                        }
                        vm.heap.release_root(root);
                    }
                    Err(error) => {
                        let atom = vm.intern_atom("kind");
                        let value = vm
                            .own_property(error.thrown_value().unwrap(), atom)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, value).unwrap(), phase);
                    }
                }
                vm.collect_now(&program);
                for handle in handles {
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn string_match_all_fallback_roots_input_and_intrinsic_matcher() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (kind, phases) in [
            ("object", &["input", "pattern", "method", "call"][..]),
            ("primitive", &["input", "method", "call"][..]),
        ] {
            for phase in phases {
                for abrupt in [false, true] {
                    let mut vm = Vm::new(Test262Host);
                    let source = format!(
                        r#"
                        function hit(name) {{if(name==='{phase}') {{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                        function selected() {{return function(input) {{hit('call');if(input.charCodeAt(0)!==55296||this.source!=='a'||this.flags!=='g')throw {{kind:'values'}};return {{tag:44,matcher:this,input:input}};}};}}
                        Object.defineProperty(RegExp.prototype,Symbol.matchAll,{{configurable:true,get() {{hit('method');return selected();}}}});
                        function receiver() {{return {{[Symbol.toPrimitive](hint) {{hit('input');if(hint!=='string')throw {{kind:'input-hint'}};return String.fromCharCode(55296,97);}}}};}}
                        function pattern() {{return {{[Symbol.match]:false,[Symbol.matchAll]:null,[Symbol.toPrimitive](hint) {{hit('pattern');if(hint!=='string')throw {{kind:'pattern-hint'}};return 'a';}}}};}}
                    "#
                    );
                    let program = compile(&source, "string-match-all-fallback-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let atom = vm.intern_atom("receiver");
                    let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                    let receiver = vm
                        .call_value(&program, factory, Value::UNDEFINED, &[])
                        .unwrap();
                    let root = vm.heap.root(receiver);
                    let pattern = if kind == "object" {
                        let atom = vm.intern_atom("pattern");
                        let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                        vm.call_value(&program, factory, Value::UNDEFINED, &[])
                            .unwrap()
                    } else {
                        vm.heap.alloc(super::Cell::String("a".into()))
                    };
                    vm.heap.release_root(root);
                    let handles = [receiver, pattern].map(|v| vm.heap.weak_handle(v).unwrap());
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let outcome = vm.string_match_all_native(&program, receiver, &[pattern]);
                    assert_eq!(outcome.is_ok(), !abrupt, "{kind}/{phase}/{abrupt}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    for (handle, value) in handles.iter().zip([receiver, pattern]) {
                        assert_eq!(vm.heap.weak_value(*handle), Some(value));
                    }
                    match outcome {
                        Ok(value) => {
                            let root = vm.heap.root(value);
                            vm.collect_now(&program);
                            let atom = vm.intern_atom("matcher");
                            let matcher = vm.own_property(value, atom).unwrap();
                            assert!(
                                matches!(vm.heap.get(matcher),Some(super::Cell::RegExp{source,flags,..})if source.host_string()=="a"&&flags=="g")
                            );
                            let atom = vm.intern_atom("input");
                            let input = vm.own_property(value, atom).unwrap();
                            assert!(
                                matches!(vm.heap.get(input),Some(super::Cell::String(text))if text.units()==[0xd800,97])
                            );
                            vm.heap.release_root(root);
                        }
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let value = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), *phase);
                        }
                    }
                    vm.collect_now(&program);
                    for handle in handles {
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn string_replace_protocol_roots_receiver_search_and_replacement() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (all, phases) in [
            (false, &["method", "call"][..]),
            (
                true,
                &["is-regexp", "flags", "flags-string", "method", "call"][..],
            ),
        ] {
            for phase in phases {
                for abrupt in [false, true] {
                    let mut vm = Vm::new(Test262Host);
                    let source = format!(
                        r#"
                    function hit(name){{if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                    function selected(){{return function(input,replacement){{hit('call');if(this.tag!==43||input.tag!==42||replacement.tag!==44)throw {{kind:'identity'}};return {{input:input,search:this,replacement:replacement}};}};}}
                    function receiver(){{return {{tag:42,[Symbol.toPrimitive](){{throw {{kind:'coerced'}};}}}};}}
                    function replacement(){{return {{tag:44,[Symbol.toPrimitive](){{throw {{kind:'coerced'}};}}}};}}
                    function search(){{return {{tag:43,get [Symbol.match](){{hit('is-regexp');return true;}},get flags(){{hit('flags');return {{[Symbol.toPrimitive](hint){{hit('flags-string');return 'g';}}}};}},get [Symbol.replace](){{hit('method');return selected();}}}};}}
                "#
                    );
                    let program = compile(&source, "string-replace-protocol-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let mut values = Vec::new();
                    let mut owners = Vec::new();
                    for name in ["receiver", "search", "replacement"] {
                        let atom = vm.intern_atom(name);
                        let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                        let value = vm
                            .call_value(&program, factory, Value::UNDEFINED, &[])
                            .unwrap();
                        values.push(value);
                        owners.push(vm.heap.root(value));
                    }
                    for owner in owners {
                        vm.heap.release_root(owner);
                    }
                    let handles = values
                        .iter()
                        .map(|v| vm.heap.weak_handle(*v).unwrap())
                        .collect::<Vec<_>>();
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let outcome = vm.string_replace_native(&program, values[0], &values[1..], all);
                    assert_eq!(outcome.is_ok(), !abrupt, "{all}/{phase}/{abrupt}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    for (handle, value) in handles.iter().zip(&values) {
                        assert_eq!(vm.heap.weak_value(*handle), Some(*value));
                    }
                    match outcome {
                        Ok(value) => {
                            let owner = vm.heap.root(value);
                            vm.collect_now(&program);
                            for (key, expected) in
                                ["input", "search", "replacement"].iter().zip(&values)
                            {
                                let atom = vm.intern_atom(key);
                                assert_eq!(vm.own_property(value, atom), Some(*expected));
                            }
                            vm.heap.release_root(owner);
                        }
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let value = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), *phase);
                        }
                    }
                    vm.collect_now(&program);
                    for handle in handles {
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn string_replace_fallback_roots_conversions_and_repeated_callback_input() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for all in [false, true] {
            for kind in ["template", "empty", "nonempty"] {
                let phases = if kind == "template" {
                    &["input", "search", "replacement"][..]
                } else if all {
                    &["input", "search", "call", "return-string", "second-call"][..]
                } else {
                    &["input", "search", "call", "return-string"][..]
                };
                for phase in phases {
                    for abrupt in [false, true] {
                        let mut vm = Vm::new(Test262Host);
                        let source = format!(
                            r#"
                    function hit(name){{if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                    function receiver(){{return {{[Symbol.toPrimitive](hint){{hit('input');if(hint!=='string')throw {{kind:'input-hint'}};return 'a-a';}}}};}}
                    function search(){{return {{[Symbol.match]:false,[Symbol.replace]:null,[Symbol.toPrimitive](hint){{hit('search');if(hint!=='string')throw {{kind:'search-hint'}};return '{kind}'==='empty'?'':'a';}}}};}}
                    function replacement(){{if('{kind}'==='template')return {{[Symbol.toPrimitive](hint){{hit('replacement');if(hint!=='string')throw {{kind:'replacement-hint'}};return 'X';}}}};
                        var calls=0;return function(matched,index,input){{hit('call');if(calls++===1)hit('second-call');if(input!=='a-a'||matched!==('{kind}'==='empty'?'':'a'))throw {{kind:'arguments'}};return {{[Symbol.toPrimitive](hint){{hit('return-string');if(hint!=='string')throw {{kind:'return-hint'}};return 'X';}}}};}};}}
                "#
                        );
                        let program = compile(&source, "string-replace-fallback-roots.js").unwrap();
                        vm.execute(&program).unwrap();
                        let mut values = Vec::new();
                        let mut owners = Vec::new();
                        for name in ["receiver", "search", "replacement"] {
                            let atom = vm.intern_atom(name);
                            let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                            let value = vm
                                .call_value(&program, factory, Value::UNDEFINED, &[])
                                .unwrap();
                            values.push(value);
                            owners.push(vm.heap.root(value));
                        }
                        for owner in owners {
                            vm.heap.release_root(owner);
                        }
                        let handles = values
                            .iter()
                            .map(|v| vm.heap.weak_handle(*v).unwrap())
                            .collect::<Vec<_>>();
                        let roots = vm.heap.root_count_for_test();
                        let calls = vm.active_call_roots.len();
                        let outcome =
                            vm.string_replace_native(&program, values[0], &values[1..], all);
                        assert_eq!(outcome.is_ok(), !abrupt, "{all}/{kind}/{phase}/{abrupt}");
                        assert_eq!(vm.heap.root_count_for_test(), roots);
                        assert_eq!(vm.active_call_roots.len(), calls);
                        for (handle, value) in handles.iter().zip(&values) {
                            assert_eq!(vm.heap.weak_value(*handle), Some(*value));
                        }
                        match outcome {
                            Ok(value) => assert_eq!(
                                vm.to_string(&program, value).unwrap(),
                                match (all, kind) {
                                    (false, "empty") => "Xa-a",
                                    (true, "empty") => "XaX-XaX",
                                    (false, _) => "X-a",
                                    (true, _) => "X-X",
                                },
                                "{all}/{kind}/{phase}"
                            ),
                            Err(error) => {
                                let atom = vm.intern_atom("kind");
                                let value = vm
                                    .own_property(error.thrown_value().unwrap(), atom)
                                    .unwrap();
                                assert_eq!(vm.to_string(&program, value).unwrap(), *phase);
                            }
                        }
                        vm.collect_now(&program);
                        for handle in handles {
                            assert!(vm.heap.weak_value(handle).is_none());
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn string_split_roots_protocol_and_conversion_inputs() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (custom, phases) in [
            (true, &["method", "call"][..]),
            (false, &["method", "input", "limit", "separator"][..]),
        ] {
            for phase in phases {
                for abrupt in [false, true] {
                    let mut vm = Vm::new(Test262Host);
                    let source = format!(
                        r#"
                    function hit(name){{if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                    function receiver(){{return {{tag:42,[Symbol.toPrimitive](hint){{hit('input');if(hint!=='string')throw {{kind:'input-hint'}};return 'a-a';}}}};}}
                    function limit(){{return {{tag:44,[Symbol.toPrimitive](hint){{hit('limit');if(hint!=='number')throw {{kind:'limit-hint'}};return 2;}}}};}}
                    function separator(){{return {{tag:43,get [Symbol.split](){{hit('method');if(!{custom})return null;return function(input,limit){{hit('call');if(this.tag!==43||input.tag!==42||limit.tag!==44)throw {{kind:'identity'}};return {{input:input,separator:this,limit:limit}};}};}},[Symbol.toPrimitive](hint){{hit('separator');if(hint!=='string')throw {{kind:'separator-hint'}};return '-';}}}};}}
                "#
                    );
                    let program = compile(&source, "string-split-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let mut values = Vec::new();
                    let mut owners = Vec::new();
                    for name in ["receiver", "separator", "limit"] {
                        let atom = vm.intern_atom(name);
                        let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                        let value = vm
                            .call_value(&program, factory, Value::UNDEFINED, &[])
                            .unwrap();
                        values.push(value);
                        owners.push(vm.heap.root(value));
                    }
                    for owner in owners {
                        vm.heap.release_root(owner);
                    }
                    let handles = values
                        .iter()
                        .map(|v| vm.heap.weak_handle(*v).unwrap())
                        .collect::<Vec<_>>();
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let outcome = vm.string_split_native(&program, values[0], &values[1..]);
                    assert_eq!(outcome.is_ok(), !abrupt, "{custom}/{phase}/{abrupt}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    for (handle, value) in handles.iter().zip(&values) {
                        assert_eq!(vm.heap.weak_value(*handle), Some(*value));
                    }
                    match outcome {
                        Ok(value) => {
                            let owner = vm.heap.root(value);
                            vm.collect_now(&program);
                            if custom {
                                for (key, expected) in
                                    ["input", "separator", "limit"].iter().zip(&values)
                                {
                                    let atom = vm.intern_atom(key);
                                    assert_eq!(vm.own_property(value, atom), Some(*expected));
                                }
                            } else {
                                let atom = vm.intern_atom("join");
                                let join = vm.get_property(&program, value, atom).unwrap();
                                let joined = vm.call_value(&program, join, value, &[]).unwrap();
                                assert_eq!(vm.to_string(&program, joined).unwrap(), "a,a");
                            }
                            vm.heap.release_root(owner);
                        }
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let value = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), *phase);
                        }
                    }
                    vm.collect_now(&program);
                    for handle in handles {
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn regexp_replace_roots_inputs_results_and_converted_captures() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (callable, phases) in [
            (
                false,
                &[
                    "input",
                    "replacement",
                    "flags",
                    "flags-string",
                    "set-index",
                    "exec",
                    "matched",
                    "matched-string",
                    "length",
                    "length-number",
                    "index",
                    "index-number",
                    "capture",
                    "capture-string",
                    "groups",
                    "group",
                    "group-string",
                ][..],
            ),
            (
                true,
                &[
                    "input",
                    "flags",
                    "flags-string",
                    "set-index",
                    "exec",
                    "matched",
                    "matched-string",
                    "length",
                    "length-number",
                    "index",
                    "index-number",
                    "capture",
                    "capture-string",
                    "groups",
                    "call",
                    "return-string",
                ][..],
            ),
        ] {
            for phase in phases {
                for abrupt in [false, true] {
                    let mut vm = Vm::new(Test262Host);
                    let source = format!(
                        r#"
                    function hit(name){{if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                    function input(){{return {{[Symbol.toPrimitive](hint){{hit('input');if(hint!=='string')throw {{kind:'input-hint'}};return 'a-a';}}}};}}
                    function replacement(){{if(!{callable})return {{[Symbol.toPrimitive](hint){{hit('replacement');return '$<x>';}}}};return function(matched,capture,missing,index,input,groups){{hit('call');if(matched!=='a'||capture!=='C'||missing!==undefined||index!==0||input!=='a-a'||groups.tag!==44)throw {{kind:'arguments'}};return {{[Symbol.toPrimitive](hint){{hit('return-string');return 'X';}}}};}};}}
                    function receiver(){{var calls=0;return {{get flags(){{hit('flags');return {{[Symbol.toPrimitive](hint){{hit('flags-string');return 'g';}}}};}},set lastIndex(value){{hit('set-index');}},exec(input){{hit('exec');if(input!=='a-a')throw {{kind:'exec-input'}};if(calls++>0)return null;return {{get 0(){{hit('matched');return {{[Symbol.toPrimitive](hint){{hit('matched-string');return 'a';}}}};}},get length(){{hit('length');return {{[Symbol.toPrimitive](hint){{hit('length-number');if(hint!=='number')throw {{kind:'length-hint'}};return 3;}}}};}},get index(){{hit('index');return {{[Symbol.toPrimitive](hint){{hit('index-number');if(hint!=='number')throw {{kind:'index-hint'}};return 0;}}}};}},get 1(){{hit('capture');return {{[Symbol.toPrimitive](hint){{hit('capture-string');return 'C';}}}};}},get groups(){{hit('groups');return {{tag:44,get x(){{hit('group');return {{[Symbol.toPrimitive](hint){{hit('group-string');return 'X';}}}};}}}};}}}};}}}};}}
                "#
                    );
                    let program = compile(&source, "regexp-replace-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let mut values = Vec::new();
                    let mut owners = Vec::new();
                    for name in ["receiver", "input", "replacement"] {
                        let atom = vm.intern_atom(name);
                        let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                        let value = vm
                            .call_value(&program, factory, Value::UNDEFINED, &[])
                            .unwrap();
                        values.push(value);
                        owners.push(vm.heap.root(value));
                    }
                    for owner in owners {
                        vm.heap.release_root(owner);
                    }
                    let handles = values
                        .iter()
                        .map(|v| vm.heap.weak_handle(*v).unwrap())
                        .collect::<Vec<_>>();
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let outcome = vm.regexp_symbol_replace(&program, values[0], &values[1..]);
                    assert_eq!(outcome.is_ok(), !abrupt, "{callable}/{phase}/{abrupt}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    for (handle, value) in handles.iter().zip(&values) {
                        assert_eq!(vm.heap.weak_value(*handle), Some(*value));
                    }
                    match outcome {
                        Ok(value) => assert_eq!(vm.to_string(&program, value).unwrap(), "X-a"),
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let value = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), *phase);
                        }
                    }
                    vm.collect_now(&program);
                    for handle in handles {
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn regexp_construction_roots_protocol_inputs_and_initialization_projections() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (kind, phases) in [
            ("identity", &["is-regexp", "constructor"][..]),
            (
                "observable-call",
                &[
                    "is-regexp",
                    "constructor",
                    "source",
                    "flags",
                    "source-string",
                    "flags-string",
                ][..],
            ),
            (
                "observable-new",
                &[
                    "is-regexp",
                    "source",
                    "flags",
                    "prototype",
                    "source-string",
                    "flags-string",
                ][..],
            ),
            (
                "raw-new",
                &["is-regexp", "prototype", "source-string", "flags-string"][..],
            ),
            (
                "internal-new",
                &["is-regexp", "prototype", "flags-string"][..],
            ),
        ] {
            for phase in phases {
                for abrupt in [false, true] {
                    let mut vm = Vm::new(Test262Host);
                    let source = format!(
                        r#"
                        function hit(name){{if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                        function text(name,value){{return {{[Symbol.toPrimitive](hint){{hit(name);if(hint!=='string')throw {{kind:'hint'}};return value;}}}};}}
                        function pattern(){{if('{kind}'==='internal-new'){{var value=/a/g;Object.defineProperty(value,Symbol.match,{{get(){{hit('is-regexp');return false;}}}});return value;}}
                            return {{get [Symbol.match](){{hit('is-regexp');return '{kind}'!=='raw-new';}},get constructor(){{hit('constructor');return '{kind}'==='identity'?RegExp:null;}},get source(){{hit('source');return text('source-string','a');}},get flags(){{hit('flags');return text('flags-string','i');}},[Symbol.toPrimitive](hint){{hit('source-string');if(hint!=='string')throw {{kind:'hint'}};return 'a';}}}};}}
                        function flags(){{return text('flags-string','i');}}
                        function target(){{return new Proxy(function(){{}},{{get(object,key,receiver){{if(key==='prototype'){{hit('prototype');return {{tag:42}};}}return Reflect.get(object,key,receiver);}}}});}}
                    "#
                    );
                    let program = compile(&source, "regexp-construction-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let construct = kind.ends_with("new");
                    let explicit_flags = matches!(kind, "raw-new" | "internal-new");
                    let mut values = Vec::new();
                    let mut owners = Vec::new();
                    for name in [
                        Some("pattern"),
                        explicit_flags.then_some("flags"),
                        construct.then_some("target"),
                    ]
                    .into_iter()
                    .flatten()
                    {
                        let atom = vm.intern_atom(name);
                        let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                        let value = vm
                            .call_value(&program, factory, Value::UNDEFINED, &[])
                            .unwrap();
                        values.push(value);
                        owners.push(vm.heap.root(value));
                    }
                    for owner in owners {
                        vm.heap.release_root(owner);
                    }
                    let handles = values
                        .iter()
                        .map(|value| vm.heap.weak_handle(*value).unwrap())
                        .collect::<Vec<_>>();
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let args = if explicit_flags {
                        &values[..2]
                    } else {
                        &values[..1]
                    };
                    let target = construct.then(|| *values.last().unwrap());
                    let outcome = vm.construct_regexp_native(&program, args, target);
                    assert_eq!(outcome.is_ok(), !abrupt, "{kind}/{phase}/{abrupt}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    for (handle, value) in handles.iter().zip(&values) {
                        assert_eq!(vm.heap.weak_value(*handle), Some(*value));
                    }
                    match outcome {
                        Ok(value) if kind == "identity" => assert_eq!(value, values[0]),
                        Ok(value) => {
                            let owner = vm.heap.root(value);
                            vm.collect_now(&program);
                            let Some(super::Cell::RegExp {
                                object,
                                source,
                                flags,
                                ..
                            }) = vm.heap.get(value)
                            else {
                                panic!("constructor returns RegExp");
                            };
                            assert_eq!(source.units(), &[u16::from(b'a')]);
                            assert_eq!(flags, "i");
                            if construct {
                                let proto = object.proto;
                                let atom = vm.intern_atom("tag");
                                let tag = vm.own_property(proto, atom).unwrap();
                                assert_eq!(tag, Value::number(42.0));
                            }
                            vm.heap.release_root(owner);
                        }
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let value = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), *phase);
                        }
                    }
                    vm.collect_now(&program);
                    for handle in handles {
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn regexp_entrypoints_root_receivers_and_arguments_through_callbacks() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (kind, phases) in [
            ("compile", &["input", "flags-string"][..]),
            (
                "format",
                &["source", "source-string", "flags", "flags-string"][..],
            ),
            (
                "flags",
                &[
                    "hasIndices",
                    "global",
                    "ignoreCase",
                    "multiline",
                    "dotAll",
                    "unicode",
                    "unicodeSets",
                    "sticky",
                ][..],
            ),
            ("exec", &["input", "last-index"][..]),
            ("test", &["input", "exec-get", "exec-call"][..]),
        ] {
            for phase in phases {
                for abrupt in [false, true] {
                    let mut vm = Vm::new(Test262Host);
                    let source = format!(
                        r#"
                    function hit(name){{if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                    function text(name,value){{return {{[Symbol.toPrimitive](hint){{hit(name);if(hint!=='string')throw {{kind:'hint'}};return value;}}}};}}
                    function input(){{return text('input','a');}}
                    function flags(){{return text('flags-string','g');}}
                    function receiver(){{if('{kind}'==='compile')return /old/;
                        if('{kind}'==='exec'){{var value=/a/g;value.lastIndex={{[Symbol.toPrimitive](hint){{hit('last-index');if(hint!=='number')throw {{kind:'index-hint'}};return 0;}}}};return value;}}
                        if('{kind}'==='test')return {{get exec(){{hit('exec-get');return function(input){{hit('exec-call');if(input!=='a')throw {{kind:'input'}};return {{}};}};}}}};
                        if('{kind}'==='format')return {{get source(){{hit('source');return text('source-string','a');}},get flags(){{hit('flags');return text('flags-string','g');}}}};
                        var result={{}};for(var name of ['hasIndices','global','ignoreCase','multiline','dotAll','unicode','unicodeSets','sticky']){{(function(name){{Object.defineProperty(result,name,{{get(){{hit(name);return true;}}}});}})(name);}}return result;
                    }}
                "#
                    );
                    let program = compile(&source, "regexp-entrypoint-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let mut values = Vec::new();
                    let mut owners = Vec::new();
                    for name in [
                        Some("receiver"),
                        matches!(kind, "compile" | "exec" | "test").then_some("input"),
                        (kind == "compile").then_some("flags"),
                    ]
                    .into_iter()
                    .flatten()
                    {
                        let atom = vm.intern_atom(name);
                        let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                        let value = vm
                            .call_value(&program, factory, Value::UNDEFINED, &[])
                            .unwrap();
                        values.push(value);
                        owners.push(vm.heap.root(value));
                    }
                    for owner in owners {
                        vm.heap.release_root(owner);
                    }
                    let handles = values
                        .iter()
                        .map(|value| vm.heap.weak_handle(*value).unwrap())
                        .collect::<Vec<_>>();
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let outcome = match kind {
                        "compile" => vm.regexp_compile_native(&program, values[0], &values[1..]),
                        "format" => vm.regexp_to_string_native(&program, values[0]),
                        "flags" => vm.regexp_flags_native(&program, values[0]),
                        "exec" => vm.regexp_builtin_exec(&program, values[0], &values[1..]),
                        "test" => vm.regexp_test(&program, values[0], &values[1..]),
                        _ => unreachable!(),
                    };
                    assert_eq!(outcome.is_ok(), !abrupt, "{kind}/{phase}/{abrupt}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    for (handle, value) in handles.iter().zip(&values) {
                        assert_eq!(
                            vm.heap.weak_value(*handle),
                            Some(*value),
                            "{kind}/{phase}/{abrupt}"
                        );
                    }
                    match outcome {
                        Ok(value) => {
                            let owner = vm.heap.root(value);
                            vm.collect_now(&program);
                            match kind {
                                "compile" => {
                                    assert_eq!(value, values[0]);
                                    let Some(super::Cell::RegExp { source, flags, .. }) =
                                        vm.heap.get(value)
                                    else {
                                        panic!("compiled receiver retained");
                                    };
                                    assert_eq!(source.units(), &[u16::from(b'a')]);
                                    assert_eq!(flags, "g");
                                }
                                "format" => {
                                    assert_eq!(vm.to_string(&program, value).unwrap(), "/a/g")
                                }
                                "flags" => {
                                    assert_eq!(vm.to_string(&program, value).unwrap(), "dgimsuvy")
                                }
                                "test" => assert_eq!(value, Value::TRUE),
                                "exec" => {
                                    let matched =
                                        vm.get_index(&program, value, Value::number(0.0)).unwrap();
                                    assert_eq!(vm.to_string(&program, matched).unwrap(), "a");
                                }
                                _ => unreachable!(),
                            };
                            vm.heap.release_root(owner);
                        }
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let value = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), *phase);
                        }
                    }
                    vm.collect_now(&program);
                    for handle in handles {
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn native_dispatch_roots_raw_inputs_through_receiver_normalization_and_calls() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (native, kind, phases) in [
            (
                Native::StringSubstring,
                "substring",
                &["input", "first", "second"][..],
            ),
            (
                Native::StringConcat,
                "concat",
                &["input", "first", "second"][..],
            ),
            (Native::StringAnchor, "anchor", &["input", "first"][..]),
        ] {
            for phase in phases {
                for abrupt in [false, true] {
                    let mut vm = Vm::new(Test262Host);
                    let source = format!(
                        r#"
                    function hit(name){{if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                    function receiver(){{return {{[Symbol.toPrimitive](hint){{hit('input');if(hint!=='string')throw {{kind:'input-hint'}};return 'abc';}}}};}}
                    function first(){{return {{[Symbol.toPrimitive](hint){{hit('first');if(hint!==('{kind}'==='substring'?'number':'string'))throw {{kind:'first-hint'}};return '{kind}'==='substring'?1:'X';}}}};}}
                    function second(){{return {{[Symbol.toPrimitive](hint){{hit('second');if(hint!==('{kind}'==='substring'?'number':'string'))throw {{kind:'second-hint'}};return '{kind}'==='substring'?2:'Y';}}}};}}
                "#
                    );
                    let program = compile(&source, "native-dispatch-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let mut values = Vec::new();
                    let mut owners = Vec::new();
                    for name in [
                        Some("receiver"),
                        Some("first"),
                        (kind != "anchor").then_some("second"),
                    ]
                    .into_iter()
                    .flatten()
                    {
                        let atom = vm.intern_atom(name);
                        let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                        let value = vm
                            .call_value(&program, factory, Value::UNDEFINED, &[])
                            .unwrap();
                        values.push(value);
                        owners.push(vm.heap.root(value));
                    }
                    if kind != "substring" {
                        for owner in &owners {
                            vm.heap.release_root(*owner);
                        }
                    }
                    let handles = values
                        .iter()
                        .map(|value| vm.heap.weak_handle(*value).unwrap())
                        .collect::<Vec<_>>();
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let outcome = vm.call_native(&program, native, values[0], &values[1..]);
                    assert_eq!(outcome.is_ok(), !abrupt, "{kind}/{phase}/{abrupt}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    for (handle, value) in handles.iter().zip(&values) {
                        assert_eq!(
                            vm.heap.weak_value(*handle),
                            Some(*value),
                            "{kind}/{phase}/{abrupt}"
                        );
                    }
                    match outcome {
                        Ok(value) => {
                            let owner = vm.heap.root(value);
                            vm.collect_now(&program);
                            assert_eq!(
                                vm.to_string(&program, value).unwrap(),
                                match kind {
                                    "concat" => "abcXY",
                                    "anchor" => "<a name=\"X\">abc</a>",
                                    "substring" => "b",
                                    _ => unreachable!(),
                                }
                            );
                            vm.heap.release_root(owner);
                        }
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let value = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), *phase);
                        }
                    }
                    if kind == "substring" {
                        for owner in owners {
                            vm.heap.release_root(owner);
                        }
                    }
                    vm.collect_now(&program);
                    for handle in handles {
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn string_raw_roots_fresh_raw_views_and_original_substitutions() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (kind, phases) in [
            (
                "object",
                &[
                    "raw",
                    "length",
                    "length-number",
                    "segment0",
                    "segment0-string",
                    "substitution0-string",
                    "segment1",
                    "segment1-string",
                    "substitution1-string",
                    "segment2",
                    "segment2-string",
                ][..],
            ),
            (
                "string",
                &["raw", "substitution0-string", "substitution1-string"][..],
            ),
        ] {
            for phase in phases {
                for abrupt in [false, true] {
                    let mut vm = Vm::new(Test262Host);
                    let source = format!(
                        r#"
                    function hit(name){{if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                    function text(name,value){{return {{[Symbol.toPrimitive](hint){{hit(name);if(hint!=='string')throw {{kind:'hint'}};return value;}}}};}}
                    function template(){{return {{get raw(){{hit('raw');if('{kind}'==='string')return 'abc';return {{get length(){{hit('length');return {{[Symbol.toPrimitive](hint){{hit('length-number');return 3.9;}}}};}},get 0(){{hit('segment0');return text('segment0-string','a');}},get 1(){{hit('segment1');return text('segment1-string','b');}},get 2(){{hit('segment2');return text('segment2-string','c');}}}};}}}};}}
                    function first(){{return text('substitution0-string','X');}}
                    function second(){{return text('substitution1-string','Y');}}
                "#
                    );
                    let program = compile(&source, "string-raw-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let mut values = Vec::new();
                    let mut owners = Vec::new();
                    for name in ["template", "first", "second"] {
                        let atom = vm.intern_atom(name);
                        let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                        let value = vm
                            .call_value(&program, factory, Value::UNDEFINED, &[])
                            .unwrap();
                        values.push(value);
                        owners.push(vm.heap.root(value));
                    }
                    if kind != "object" {
                        for owner in &owners {
                            vm.heap.release_root(*owner);
                        }
                    }
                    let handles = values
                        .iter()
                        .map(|value| vm.heap.weak_handle(*value).unwrap())
                        .collect::<Vec<_>>();
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let outcome = vm.string_raw(&program, &values);
                    assert_eq!(outcome.is_ok(), !abrupt, "{kind}/{phase}/{abrupt}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    for (handle, value) in handles.iter().zip(&values) {
                        assert_eq!(
                            vm.heap.weak_value(*handle),
                            Some(*value),
                            "{kind}/{phase}/{abrupt}"
                        );
                    }
                    match outcome {
                        Ok(value) => {
                            let owner = vm.heap.root(value);
                            vm.collect_now(&program);
                            assert_eq!(vm.to_string(&program, value).unwrap(), "aXbYc");
                            vm.heap.release_root(owner);
                        }
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let value = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), *phase);
                        }
                    }
                    if kind == "object" {
                        for owner in owners {
                            vm.heap.release_root(owner);
                        }
                    }
                    vm.collect_now(&program);
                    for handle in handles {
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn collator_construction_roots_boxed_locale_and_option_views() {
    use super::Cell;
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in [
            "usage-string",
            "usage",
            "length",
            "index",
            "locale",
            "localeMatcher",
            "collation",
            "numeric",
            "caseFirst",
            "sensitivity",
            "ignorePunctuation",
        ] {
            for abrupt in [false, true] {
                let source = format!(
                    r#"
                var trace=[];function traceLog(){{return trace.join(',');}}
                function hit(name){{trace.push(name);if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                function localeValue(){{return {{toString(){{hit('locale');return 'en';}}}};}}
                function usageValue(){{return {{toString(){{hit('usage-string');return 'sort';}}}};}}
                Object.defineProperty(Number.prototype,'length',{{configurable:true,get(){{hit('length');return 1;}}}});
                Object.defineProperty(Number.prototype,'0',{{configurable:true,get(){{hit('index');return localeValue();}}}});
                for(var key of ['usage','localeMatcher','collation','numeric','caseFirst','sensitivity','ignorePunctuation']){{
                    ((name)=>Object.defineProperty(Number.prototype,name,{{configurable:true,get(){{hit(name);if(name==='usage')return usageValue();return undefined;}}}}))(key);
                }}
                "#
                );
                let program = compile(&source, "collator-boxed-roots.js").unwrap();
                let mut vm = Vm::new(Test262Host);
                vm.execute(&program).unwrap();
                let constructor = *vm
                    .realm
                    .intrinsics
                    .intl_collator_constructors
                    .get(&vm.realm.globals)
                    .unwrap();
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let locales = if matches!(phase, "length" | "index" | "locale") {
                    Value::number(42.0)
                } else {
                    vm.heap.alloc(Cell::String("en".into()))
                };
                let result = vm.intl_collator_construct(
                    &program,
                    &[locales, Value::number(7.0)],
                    constructor,
                );
                assert_eq!(result.is_ok(), !abrupt, "{phase}/{abrupt}");
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                match result {
                    Ok(collator) => {
                        let owner = vm.heap.root(collator);
                        vm.collect_now(&program);
                        let left = vm.heap.alloc(Cell::String("a".into()));
                        let right = vm.heap.alloc(Cell::String("b".into()));
                        let result = vm
                            .intl_collator_native(
                                &program,
                                Native::IntlCollatorCompare,
                                collator,
                                &[left, right],
                            )
                            .unwrap();
                        assert_eq!(result.as_number(), Some(-1.0), "{phase}");
                        let atom = vm.intern_atom("traceLog");
                        let logger = vm.own_property(vm.realm.globals, atom).unwrap();
                        let trace = vm
                            .call_value(&program, logger, Value::UNDEFINED, &[])
                            .unwrap();
                        let prefix = if matches!(phase, "length" | "index" | "locale") {
                            "length,index,locale,"
                        } else {
                            ""
                        };
                        assert_eq!(
                            vm.to_string(&program, trace).unwrap(),
                            format!(
                                "{prefix}usage,usage-string,localeMatcher,collation,numeric,caseFirst,sensitivity,ignorePunctuation"
                            ),
                            "{phase}"
                        );
                        vm.heap.release_root(owner);
                    }
                    Err(error) => {
                        let kind = vm.intern_atom("kind");
                        let value = vm
                            .own_property(error.thrown_value().unwrap(), kind)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, value).unwrap(), phase);
                    }
                }
                vm.collect_now(&program);
            }
        }
    }
}

#[test]
fn intl_coerced_option_views_survive_fresh_conversion_callbacks() {
    use super::Cell;
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (kind, key, expected) in [
            ("NumberFormat", "style", "percent"),
            ("PluralRules", "type", "ordinal"),
            ("RelativeTimeFormat", "numeric", "auto"),
        ] {
            for phase in ["convert", "matcher", "selected"] {
                for abrupt in [false, true] {
                    let source = format!(
                        r#"
                    var trace=[];
                    function hit(name){{trace.push(name);if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                    function matcher(){{return {{toString(){{hit('convert');return 'lookup';}}}};}}
                    Object.defineProperty(Number.prototype,'localeMatcher',{{get(){{hit('matcher');return matcher();}},configurable:true}});
                    Object.defineProperty(Number.prototype,'{key}',{{get(){{hit('selected');return '{expected}';}},configurable:true}});
                    function inspect(value){{return Intl.{kind}.prototype.resolvedOptions.call(value)['{key}'];}}
                    function traceLog(){{return trace.join(',');}}
                    "#
                    );
                    let program = compile(&source, "intl-coerced-view.js").unwrap();
                    let mut vm = Vm::new(Test262Host);
                    vm.execute(&program).unwrap();
                    let intl = vm.intern_atom("Intl");
                    let intl = vm.own_property(vm.realm.globals, intl).unwrap();
                    let name = vm.intern_atom(kind);
                    let constructor = vm.own_property(intl, name).unwrap();
                    let locale = vm.heap.alloc(Cell::String("en".into()));
                    let weak = vm.heap.weak_handle(locale).unwrap();
                    let locale_owner = vm.heap.root(locale);
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let result = match kind {
                        "NumberFormat" => vm.intl_number_format_construct(
                            &program,
                            &[locale, Value::number(7.0)],
                            constructor,
                        ),
                        "PluralRules" => vm.intl_plural_rules_construct(
                            &program,
                            &[locale, Value::number(7.0)],
                            constructor,
                        ),
                        "RelativeTimeFormat" => vm.intl_relative_time_format_construct(
                            &program,
                            &[locale, Value::number(7.0)],
                            constructor,
                        ),
                        _ => unreachable!(),
                    };
                    assert_eq!(result.is_ok(), !abrupt, "{kind}/{phase}/{abrupt}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    assert_eq!(vm.heap.weak_value(weak), Some(locale), "{kind}/{phase}");
                    match result {
                        Ok(instance) => {
                            let owner = vm.heap.root(instance);
                            vm.collect_now(&program);
                            let atom = vm.intern_atom("inspect");
                            let inspect = vm.own_property(vm.realm.globals, atom).unwrap();
                            let output = vm
                                .call_value(&program, inspect, Value::UNDEFINED, &[instance])
                                .unwrap();
                            assert_eq!(
                                vm.to_string(&program, output).unwrap(),
                                expected,
                                "{kind}/{phase}"
                            );
                            let atom = vm.intern_atom("traceLog");
                            let logger = vm.own_property(vm.realm.globals, atom).unwrap();
                            let trace = vm
                                .call_value(&program, logger, Value::UNDEFINED, &[])
                                .unwrap();
                            assert_eq!(
                                vm.to_string(&program, trace).unwrap(),
                                "matcher,convert,selected",
                                "{kind}/{phase}"
                            );
                            vm.heap.release_root(owner);
                        }
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let value = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), phase);
                        }
                    }
                    vm.heap.release_root(locale_owner);
                    vm.collect_now(&program);
                    assert!(vm.heap.weak_value(weak).is_none());
                }
            }
        }
    }
}

#[test]
fn intl_constructor_views_survive_prototype_and_option_callbacks() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (kind, key, expected) in [
            ("DisplayNames", "type", "language"),
            ("DurationFormat", "style", "short"),
            ("DateTimeFormat", "year", "numeric"),
        ] {
            for phase in ["prototype", "locale", "matcher", "convert", "selected"] {
                for abrupt in [false, true] {
                    let source = format!(
                        r#"
                    function hit(name){{if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                    function matcher(){{return {{toString(){{hit('convert');return 'lookup';}}}};}}
                    function locales(){{return {{get length(){{hit('locale');return 1;}},0:'en'}};}}
                    function options(){{var value={{get localeMatcher(){{hit('matcher');return matcher();}}}};Object.defineProperty(value,'{key}',{{get(){{hit('selected');return '{expected}';}}}});return value;}}
                    function target(){{return new Proxy(function(){{}},{{get(value,key){{if(key==='prototype'){{hit('prototype');return {{marker:'expected'}};}}return Reflect.get(value,key);}}}});}}
                    function inspect(value){{return Object.getPrototypeOf(value).marker+':'+Intl.{kind}.prototype.resolvedOptions.call(value)['{key}'];}}
                    "#
                    );
                    let program = compile(&source, "intl-constructor-views.js").unwrap();
                    let mut vm = Vm::new(Test262Host);
                    vm.execute(&program).unwrap();
                    let mut args = Vec::new();
                    let mut owners = Vec::new();
                    for name in ["locales", "options", "target"] {
                        let atom = vm.intern_atom(name);
                        let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                        let value = vm
                            .call_value(&program, factory, Value::UNDEFINED, &[])
                            .unwrap();
                        args.push(value);
                        owners.push(vm.heap.root(value));
                    }
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let result = match kind {
                        "DisplayNames" => {
                            vm.intl_display_names_construct(&program, &args[..2], args[2])
                        }
                        "DurationFormat" => {
                            vm.intl_duration_format_construct(&program, &args[..2], args[2])
                        }
                        "DateTimeFormat" => {
                            vm.intl_date_time_format_construct(&program, &args[..2], args[2])
                        }
                        _ => unreachable!(),
                    };
                    assert_eq!(result.is_ok(), !abrupt, "{kind}/{phase}/{abrupt}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    match result {
                        Ok(instance) => {
                            let owner = vm.heap.root(instance);
                            vm.collect_now(&program);
                            let atom = vm.intern_atom("inspect");
                            let inspect = vm.own_property(vm.realm.globals, atom).unwrap();
                            let output = vm
                                .call_value(&program, inspect, Value::UNDEFINED, &[instance])
                                .unwrap();
                            assert_eq!(
                                vm.to_string(&program, output).unwrap(),
                                format!("expected:{expected}"),
                                "{kind}/{phase}"
                            );
                            vm.heap.release_root(owner);
                        }
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let value = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), phase);
                        }
                    }
                    for owner in owners {
                        vm.heap.release_root(owner);
                    }
                    vm.collect_now(&program);
                }
            }
        }
    }
}
