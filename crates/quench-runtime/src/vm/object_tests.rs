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

fn field_cache_entries<H: Host>(
    vm: &Vm<H>,
    program: &ResidualProgram,
    atom: Atom,
) -> Vec<FieldCache> {
    let mut sites = Vec::new();
    for function in &program.functions {
        for instruction in &function.code {
            if instruction.op() != Op::GetField {
                continue;
            }
            match instruction.field_lookup() {
                Some(crate::bytecode::FieldLookup::Atom {
                    atom: field,
                    cache_site,
                    ..
                }) if field == atom => sites.push(cache_site),
                Some(crate::bytecode::FieldLookup::Site(index)) => {
                    let Some(site) = program.field_sites.get(index) else {
                        continue;
                    };
                    for (field, cache_site) in [Some(site.first), site.second].into_iter().flatten()
                    {
                        if field == atom {
                            sites.push(cache_site);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    sites
        .into_iter()
        .flat_map(|site| {
            let index = vm.field_cache_index(site);
            let primary = vm
                .field_caches
                .get(index)
                .copied()
                .filter(|entry| entry.receiver != NO_FIELD_RECEIVER);
            let table = vm
                .megamorphic_field_indices
                .get(index)
                .copied()
                .filter(|index| *index != NO_MEGAMORPHIC_FIELD)
                .and_then(|index| vm.megamorphic_fields.get(index as usize));
            let entries = table.into_iter().flat_map(|table| {
                table.overflow.as_ref().map_or_else(
                    || table.entries[..usize::from(table.len)].to_vec(),
                    |overflow| overflow.values().copied().collect(),
                )
            });
            primary.into_iter().chain(entries)
        })
        .collect()
}

// Exercise the supported residual CallMethod opcode directly. Optional-call
// lowering now resolves the callee before arguments through GetField + Call.
fn method_cache_program(mut program: ResidualProgram) -> ResidualProgram {
    let atom = program
        .atoms
        .iter()
        .position(|atom| atom == "method")
        .unwrap() as Atom;
    let cache = program.cache_sites;
    program.cache_sites = program.cache_sites.checked_add(1).unwrap();
    let site = program.method_sites.len() as u32;
    program.method_sites.push(crate::bytecode::MethodSite {
        atom,
        cache,
        argument_start: 0,
        argument_count: 0,
        receiver_path: None,
    });
    let function = program
        .functions
        .iter_mut()
        .find(|function| {
            function
                .name
                .is_some_and(|name| &program.atoms[name as usize] == "callMethod")
        })
        .unwrap();
    assert_eq!(function.params, 1);
    assert!(function.wide.is_empty());
    function.code = vec![
        crate::bytecode::Instr::new(Op::LoadLocal, 0, 0, 0, 0),
        crate::bytecode::Instr::new(Op::CallMethod, 1, 0, 0, site),
        crate::bytecode::Instr::new(Op::Return, 1, 0, 0, 0),
    ];
    function.local_registers.clear();
    function.parameter_end_pc = 0;
    function.source_positions.clear();
    let methods = program
        .method_sites
        .iter()
        .map(|site| (site.atom, site.cache, Vec::new(), site.receiver_path))
        .collect::<Vec<_>>();
    program.register_roots = crate::compile::liveness::derive(
        &mut program.functions,
        &methods,
        &program.field_sites,
        &program.superinstructions,
    );
    program.validate().unwrap();
    program
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
                holder: NO_FIELD_HOLDER,
                slot: 0,
            },
        );
    }
    let table = &vm.megamorphic_fields[0];
    assert_eq!(table.len(), 4);
    assert!(table.get(1, NO_FIELD_HOLDER).is_some() && table.get(4, NO_FIELD_HOLDER).is_some());
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
                field_cache_entries(&vm, &program, value)
                    .iter()
                    .any(|entry| entry.receiver != NO_FIELD_RECEIVER),
                "specialized field read should populate its cache"
            );
        }
    }
}

#[test]
fn static_three_field_object_literals_share_the_shape_site() {
    let source = r#"
      var order = "";
      function value(item) { order += item; return item * 3; }
      function make() { return { first: value(1), second: value(2), third: value(3) }; }
      var result = make();
      print(order);
      print(Object.keys(result).join(","));
      print(result.first + "," + result.second + "," + result.third);
      print(Object.getOwnPropertyDescriptor(result, "second").enumerable);
    "#;
    let expected = ["123", "first,second,third", "3,6,9", "true"];
    for (mode, compile) in [
        ("specialized", Engine::specialize as fn(&str, &str) -> _),
        ("unspecialized", Engine::specialize_unspecialized),
    ] {
        let program = compile(source, "static-object-literal.js").unwrap();
        let function = program
            .functions
            .iter()
            .find(|function| {
                function
                    .name
                    .is_some_and(|name| &program.atoms[name as usize] == "make")
            })
            .unwrap();
        let (pc, instruction) = function
            .code
            .iter()
            .copied()
            .enumerate()
            .find(|(_, instruction)| instruction.op() == Op::MakeObjectLiteral)
            .unwrap_or_else(|| panic!("{mode} compiler did not select the literal site"));
        let window = instruction.register_window();
        assert_eq!(window.count, 3, "{mode}");
        let root_map = program.register_roots[function.register_root_offset as usize + pc];
        for register in window.base..window.base + window.count {
            assert_ne!(
                root_map & (1u64 << register),
                0,
                "{mode}: register {register}"
            );
        }
        assert_eq!(
            program.object_sites[instruction.object_site_index()]
                .atoms
                .len(),
            3
        );

        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(RecordingHost(output.clone()));
        vm.execute(&program).unwrap();
        assert_eq!(output.borrow().as_slice(), expected, "{mode}");
    }
}

#[test]
fn custom_prototype_fallback_tracks_changes_and_rejects_cycles() {
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
fn inherited_field_cache_reads_live_object_prototype_data() {
    let source = r#"
      Object.prototype.cacheValue = 1;
      function readValue(value) { return value.cacheValue; }
      var receiver = {};
      print(readValue(receiver));
      print(readValue(receiver));
      Object.prototype.cacheValue = 2;
      print(readValue(receiver));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(RecordingHost(output.clone()));
    let program = Engine::specialize(source, "inherited-field-cache-live.js").unwrap();
    vm.execute(&program).unwrap();
    assert_eq!(output.borrow().as_slice(), ["1", "1", "2"]);
    let value = vm.intern_atom("cacheValue");
    assert!(
        field_cache_entries(&vm, &program, value)
            .iter()
            .any(|entry| entry.holder != NO_FIELD_HOLDER),
        "ordinary receivers should cache an immediate-prototype data property"
    );
    #[cfg(feature = "profile-aggregate")]
    assert!(vm.profile.field_cache_depths[1] > 0);
}

#[test]
fn custom_prototype_fallback_reads_live_data_and_keeps_accessor_fallback() {
    let source = r#"
      var getterCalls = 0;
      var prototype = { value: 1 };
      var receiver = Object.create(prototype);
      function readValue(value) { return value.value; }
      print(readValue(receiver));
      print(readValue(receiver));
      prototype.value = 2;
      print(readValue(receiver));
      Object.defineProperty(prototype, "value", {
        get: function() { getterCalls += 1; return this === receiver ? getterCalls + 2 : -1; },
        configurable: true
      });
      print(readValue(receiver));
      print(readValue(receiver));
      print(getterCalls);
      var nextPrototype = { value: 8 };
      Object.setPrototypeOf(receiver, nextPrototype);
      print(readValue(receiver));
      receiver.value = 9;
      print(readValue(receiver));
    "#;
    for (mode, compile) in [
        ("specialized", Engine::specialize as fn(&str, &str) -> _),
        ("unspecialized", Engine::specialize_unspecialized),
    ] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(RecordingHost(output.clone()));
        let program = compile(source, "inherited-field-cache.js").unwrap();
        vm.execute(&program).unwrap();
        assert_eq!(
            output.borrow().as_slice(),
            ["1", "1", "2", "3", "4", "2", "8", "9"],
            "{mode}"
        );
    }
}

#[test]
fn inherited_builtin_fields_cache_array_and_object_prototypes() {
    let source = r#"
      var array = [];
      var object = { own: 1 };
      function readPush(value) { return value.push; }
      function readHasOwn(value) { return value.hasOwnProperty; }
      var initialPush = Array.prototype.push;
      var initialHasOwn = Object.prototype.hasOwnProperty;
      print(readPush(array) === initialPush);
      print(readPush(array) === initialPush);
      print(readHasOwn(object) === initialHasOwn);
      print(readHasOwn(object) === initialHasOwn);
      Array.prototype.push = function(value) { this[0] = value; return 1; };
      Object.prototype.hasOwnProperty = function(key) { return this.own === key; };
      var replacementPush = Array.prototype.push;
      var replacementHasOwn = Object.prototype.hasOwnProperty;
      print(readPush(array) === replacementPush);
      print(readHasOwn(object) === replacementHasOwn);
      print(array.push(7));
      print(array[0]);
      print(object.hasOwnProperty(1));
    "#;
    for (mode, compile) in [
        ("specialized", Engine::specialize as fn(&str, &str) -> _),
        ("unspecialized", Engine::specialize_unspecialized),
    ] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(RecordingHost(output.clone()));
        let program = compile(source, "inherited-builtin-fields.js").unwrap();
        vm.execute(&program).unwrap();
        assert_eq!(
            output.borrow().as_slice(),
            [
                "true", "true", "true", "true", "true", "true", "1", "7", "true"
            ],
            "{mode}"
        );
        if vm.specialized {
            for atom in [vm.intern_atom("push"), vm.intern_atom("hasOwnProperty")] {
                assert!(
                    field_cache_entries(&vm, &program, atom)
                        .iter()
                        .any(|entry| entry.holder != NO_FIELD_HOLDER),
                    "inherited builtin reads should cache their immediate prototype"
                );
            }
        }
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
                field_cache_entries(&vm, &program, method)
                    .iter()
                    .any(|entry| entry.receiver != NO_FIELD_RECEIVER),
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
        let program = method_cache_program(
            compile(source, "optional-method-cache-callable-replacement.js").unwrap(),
        );
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
        let program = method_cache_program(
            compile(source, "optional-method-cache-inherited-replacement.js").unwrap(),
        );
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
        let program =
            method_cache_program(compile(source, "optional-method-cache-megamorphic.js").unwrap());
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
                field_cache_entries(&vm, &program, value)
                    .iter()
                    .all(|entry| entry.receiver == NO_FIELD_RECEIVER),
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
                field_cache_entries(&vm, &program, value)
                    .iter()
                    .all(|entry| entry.receiver == NO_FIELD_RECEIVER),
                "Proxy reads must retain the generic trap path"
            );
        }
    }
}

#[test]
fn inherited_field_cache_preserves_proxy_prototype_traps() {
    let source = r#"
      var reads = 0;
      var prototype = new Proxy({}, {
        get: function(target, key, receiver) {
          if (key === "value") { reads += 1; return reads; }
          return Reflect.get(target, key, receiver);
        }
      });
      var receiver = Object.create(prototype);
      function readValue(value) { return value.value; }
      print(readValue(receiver));
      print(readValue(receiver));
      print(reads);
    "#;
    for (mode, compile) in [
        ("specialized", Engine::specialize as fn(&str, &str) -> _),
        ("unspecialized", Engine::specialize_unspecialized),
    ] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(RecordingHost(output.clone()));
        let program = compile(source, "inherited-proxy-field-cache.js").unwrap();
        vm.execute(&program).unwrap();
        assert_eq!(output.borrow().as_slice(), ["1", "2", "2"], "{mode}");
        if vm.specialized {
            let value = vm.intern_atom("value");
            assert!(
                field_cache_entries(&vm, &program, value)
                    .iter()
                    .all(|entry| entry.receiver == NO_FIELD_RECEIVER),
                "Proxy prototype reads must retain the generic trap path"
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
        let program = method_cache_program(compile(source, "method-cache-proxy-get.js").unwrap());
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
        let program = method_cache_program(compile(source, "optional-method-accessor.js").unwrap());
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
                field_cache_entries(&vm, &program, method)
                    .iter()
                    .all(|entry| entry.receiver == NO_FIELD_RECEIVER),
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

#[test]
fn static_index_field_cache_admits_only_shape_backed_storage() {
    let source = r#"
      var ordinary = {'0': 41};
      var array = [42];
      var view = new Uint8Array([43]);
      view['01'] = 44;
      function first(value) {return value['0'];}
      function noncanonical(value) {return value['01'];}
      print(first(ordinary)); print(first(ordinary));
      print(first(array)); print(first(view));
      print(noncanonical(view)); print(noncanonical(view));
    "#;
    for (mode, compile) in [
        ("specialized", Engine::specialize as fn(&str, &str) -> _),
        ("unspecialized", Engine::specialize_unspecialized),
    ] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(RecordingHost(output.clone()));
        let program = compile(source, "static-index-cache.js").unwrap();
        vm.execute(&program).unwrap();
        assert_eq!(
            output.borrow().as_slice(),
            ["41", "41", "42", "43", "44", "44"],
            "{mode}"
        );
        if vm.specialized {
            let atom = vm.intern_atom("ordinary");
            let ordinary = vm.own_property(vm.realm.globals, atom).unwrap();
            let index = vm.intern_atom("0");
            let entries = field_cache_entries(&vm, &program, index);
            assert!(
                !entries.is_empty(),
                "ordinary indexed names must still exercise the field cache"
            );
            let ordinary_shape = vm.object_data(ordinary).unwrap().shape();
            assert!(entries.iter().all(|entry| entry.receiver == ordinary_shape));
            let atom = vm.intern_atom("view");
            let view = vm.own_property(vm.realm.globals, atom).unwrap();
            let noncanonical = vm.intern_atom("01");
            assert!(
                field_cache_entries(&vm, &program, noncanonical)
                    .iter()
                    .any(|entry| entry.receiver == vm.object_data(view).unwrap().shape()),
                "noncanonical typed-array names use ordinary shape storage"
            );
        }
    }
}

#[test]
fn exhausted_field_cache_preserves_generic_reads() {
    let count = FIELD_MEGAMORPHIC_LIMIT + 1;
    let source = format!(
        "var sum = 0; function read(value) {{return value.payload;}} for (var index = 1; index <= {count}; index++) {{var value = {{payload:index}}; value['shape' + index] = index; sum += read(value);}} print(sum);"
    );
    let expected = (count * (count + 1) / 2).to_string();
    for (mode, compile) in [
        ("specialized", Engine::specialize as fn(&str, &str) -> _),
        ("unspecialized", Engine::specialize_unspecialized),
    ] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(RecordingHost(output.clone()));
        let program = compile(&source, "field-cache-exhaustion.js").unwrap();
        vm.initialize(&program).unwrap();
        vm.heap.retain_allocations_for_test();
        let root = vm.closure(&program, 0, Value::NULL).unwrap();
        vm.call_value(&program, root, vm.realm.globals, &[])
            .unwrap();
        assert_eq!(
            vm.heap.stats().1,
            0,
            "cache budget must fill before collection clears it"
        );
        assert_eq!(output.borrow().as_slice(), [expected.as_str()], "{mode}");
        if vm.specialized {
            assert!(
                vm.megamorphic_fields
                    .iter()
                    .any(|set| set.len() == FIELD_MEGAMORPHIC_LIMIT),
                "test must exhaust the existing per-site shape budget"
            );
            assert!(
                vm.megamorphic_fields
                    .iter()
                    .all(|set| set.len() <= FIELD_MEGAMORPHIC_LIMIT)
            );
        }
    }
}
