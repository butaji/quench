use super::wtf16::JsString;
use super::{
    CallTarget, JsError, MethodCache, Vm, activation::Completion, activation::Continuation,
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
    let program = Engine::specialize(source, "accessor.js").unwrap();
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(RecordingHost(output.clone()));
    vm.execute(&program).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        [
            "3", "9", "function", "function", "7", "true", "blocked", "4", "6", "class"
        ]
    );
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
    let globals = vm.realm.globals;
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
    let id = vm.suspend_continuation(Continuation {
        program: super::program_store::ProgramId::MAIN,
        active_iterators: vec![],
        function: 0,
        pc: 0,
        env: live,
        this: Value::UNDEFINED,
        locals: vec![],
        registers: vec![],
        completion: Completion::Yield(Value::UNDEFINED),
        captured: false,
        resume_register: None,
        promise: Value::UNDEFINED,
    });
    vm.collect_now(&program);
    assert!(vm.heap.get(live).is_some());
    assert!(vm.resume_continuation(id).is_some());
    assert!(vm.resume_continuation(id).is_none());
    vm.collect_now(&program);
    assert!(vm.heap.get(live).is_none());
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
