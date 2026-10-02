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
