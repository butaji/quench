use super::*;
use crate::{CapabilityId, Engine, HostGlobal, ResidualProgram};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Default)]
struct Capture(Rc<RefCell<Vec<String>>>);

impl Host for Capture {
    fn write_line(&mut self, text: &str) {
        self.0.borrow_mut().push(text.into());
    }

    fn clock_millis(&mut self) -> f64 {
        0.0
    }

    fn globals(&self) -> &'static [HostGlobal] {
        &[HostGlobal {
            name: "$262",
            capability: CapabilityId::CreateRealm,
        }]
    }
}

fn initialized(source: &str) -> (Runtime<Capture>, ResidualProgram, Capture) {
    let host = Capture::default();
    let mut runtime = Runtime::new(host.clone());
    let program = Engine::specialize(source, "embedding.js").unwrap();
    runtime.execute(&program).unwrap();
    (runtime, program, host)
}

fn property(runtime: &mut Runtime<Capture>, object: RootId, name: &str) -> RootId {
    let key = runtime.string_rooted(name);
    let result = runtime.get_property_rooted(object, key).unwrap();
    assert!(runtime.release_root(key));
    result
}

#[test]
fn invalid_inputs_are_rejected_before_guest_effects() {
    let (mut runtime, _, host) = initialized(
        "var target = {get value() { print('get'); return 1; }, set value(input) { print('set'); }};
         var key = {toString() { print('key'); return 'value'; }};
         var callback = function() { print('call'); return 1; };",
    );
    let global = runtime.global_root().unwrap();
    let target = property(&mut runtime, global, "target");
    let key = property(&mut runtime, global, "key");
    let callback = property(&mut runtime, global, "callback");
    let receiver = runtime.root(Value::UNDEFINED);
    let mut other = Runtime::new(Capture::default());
    let foreign = other.root(Value::UNDEFINED);
    let stale = runtime.root(Value::UNDEFINED);
    assert!(runtime.release_root(stale));
    let replacement = runtime.root(Value::TRUE);
    assert_ne!(stale, replacement);

    for invalid in [foreign, stale] {
        for error in [
            runtime.get_property_rooted(invalid, key).unwrap_err(),
            runtime.get_property_rooted(target, invalid).unwrap_err(),
            runtime.call_rooted(invalid, receiver, &[]).unwrap_err(),
            runtime.call_rooted(callback, invalid, &[]).unwrap_err(),
            runtime
                .call_rooted(callback, receiver, &[replacement, invalid])
                .unwrap_err(),
            runtime
                .construct_rooted(invalid, callback, &[])
                .unwrap_err(),
            runtime
                .construct_rooted(callback, invalid, &[])
                .unwrap_err(),
            runtime
                .construct_rooted(callback, callback, &[replacement, invalid])
                .unwrap_err(),
            runtime
                .set_property_rooted(invalid, key, replacement, target)
                .unwrap_err(),
            runtime
                .set_property_rooted(target, invalid, replacement, target)
                .unwrap_err(),
            runtime
                .set_property_rooted(target, key, invalid, target)
                .unwrap_err(),
            runtime
                .set_property_rooted(target, key, replacement, invalid)
                .unwrap_err(),
        ] {
            assert!(error.exception.is_none());
            assert_eq!(error.to_string(), "released or foreign embedding root");
        }
    }
    assert!(host.0.borrow().is_empty());
    assert!(runtime.root_is_live(replacement));
    runtime.get_property_rooted(target, key).unwrap();
    assert!(
        runtime
            .set_property_rooted(target, key, replacement, target)
            .unwrap()
    );
    runtime.call_rooted(callback, receiver, &[]).unwrap();
    assert_eq!(
        host.0.borrow().as_slice(),
        ["key", "get", "key", "set", "call"]
    );
}

#[test]
fn constructed_results_and_exceptions_retain_dynamic_program_ownership() {
    let (mut runtime, program, _) = initialized(
        "var payload = {}; var Target = function() {};\n         var Constructor = eval('(function(value) { this.payload = value; })');\n         var Throwing = eval('(function(value) { throw {payload: value}; })');",
    );
    let global = runtime.global_root().unwrap();
    let payload = property(&mut runtime, global, "payload");
    let target = property(&mut runtime, global, "Target");
    let constructor = property(&mut runtime, global, "Constructor");
    let throwing = property(&mut runtime, global, "Throwing");
    let ordinary = runtime.object_rooted().unwrap();
    let instance = runtime
        .construct_rooted(constructor, target, &[payload])
        .unwrap();
    let error = runtime
        .construct_rooted(throwing, target, &[payload])
        .unwrap_err();
    let exception = error.exception.unwrap();
    runtime.collect(&program).unwrap();
    assert!(runtime.root_is_live(ordinary));
    for root in [instance, exception] {
        let retained_payload = property(&mut runtime, root, "payload");
        assert_eq!(
            runtime.rooted_value(retained_payload),
            runtime.rooted_value(payload)
        );
        assert!(runtime.release_root(retained_payload));
    }
    assert_eq!(runtime.rooted_value(exception), error.error.thrown_value());
    runtime.execute(&program).unwrap();
    for root in [
        ordinary,
        instance,
        exception,
        payload,
        target,
        constructor,
        throwing,
    ] {
        assert!(!runtime.root_is_live(root));
    }
}

#[test]
fn host_property_writes_preserve_receiver_roots_and_thrown_values() {
    let (mut runtime, program, _) = initialized(
        "var target = {set value(input) { $262.gc(); this.saved = input; }};
         var receiver = {};
         var key = {toString() { $262.gc(); return 'value'; }};
         var rejecting = {set value(input) { $262.gc(); throw {payload: input}; }};
         var refused = Object.freeze({value: 1});",
    );
    let global = runtime.global_root().unwrap();
    let target = property(&mut runtime, global, "target");
    let receiver = property(&mut runtime, global, "receiver");
    let key = property(&mut runtime, global, "key");
    let rejecting = property(&mut runtime, global, "rejecting");
    let refused = property(&mut runtime, global, "refused");
    let input = runtime.string_rooted("retained host input");
    let expected = runtime.rooted_value(input).unwrap();
    assert!(
        runtime
            .set_property_rooted(target, key, input, receiver)
            .unwrap()
    );
    assert!(runtime.release_root(input));
    runtime.collect(&program).unwrap();
    let saved = property(&mut runtime, receiver, "saved");
    assert_eq!(runtime.rooted_value(saved), Some(expected));
    let target_saved = property(&mut runtime, target, "saved");
    assert_eq!(runtime.rooted_value(target_saved), Some(Value::UNDEFINED));
    let input = runtime.string_rooted("thrown host input");
    let expected = runtime.rooted_value(input).unwrap();
    let error = runtime
        .set_property_rooted(rejecting, key, input, receiver)
        .unwrap_err();
    let exception = error.exception.unwrap();
    assert!(runtime.release_root(input));
    runtime.collect(&program).unwrap();
    let payload = property(&mut runtime, exception, "payload");
    assert_eq!(runtime.rooted_value(payload), Some(expected));
    assert_eq!(runtime.rooted_value(exception), error.error.thrown_value());
    assert!(
        !runtime
            .set_property_rooted(refused, key, payload, refused)
            .unwrap()
    );
    assert!(runtime.release_root(exception));
    assert!(!runtime.root_is_live(exception));
}

#[test]
fn host_call_inputs_and_results_keep_identity_through_collection() {
    let (mut runtime, program, _) = initialized(
        "var make = function() { return {}; };
         var callback = function(arg) { $262.gc(); return {receiver: this, argument: arg}; };",
    );
    let global = runtime.global_root().unwrap();
    let make = property(&mut runtime, global, "make");
    let callback = property(&mut runtime, global, "callback");
    let undefined = runtime.root(Value::UNDEFINED);
    let receiver = runtime.call_rooted(make, undefined, &[]).unwrap();
    let argument = runtime.call_rooted(make, undefined, &[]).unwrap();
    let receiver_value = runtime.rooted_value(receiver).unwrap();
    let argument_value = runtime.rooted_value(argument).unwrap();
    let result = runtime
        .call_rooted(callback, receiver, &[argument])
        .unwrap();
    assert!(runtime.release_root(receiver));
    assert!(runtime.release_root(argument));
    runtime.collect(&program).unwrap();
    let retained_receiver = property(&mut runtime, result, "receiver");
    let retained_argument = property(&mut runtime, result, "argument");
    assert_eq!(
        runtime.rooted_value(retained_receiver),
        Some(receiver_value)
    );
    assert_eq!(
        runtime.rooted_value(retained_argument),
        Some(argument_value)
    );
    assert!(runtime.release_root(result));
    assert!(runtime.release_root(retained_receiver));
    assert!(runtime.release_root(retained_argument));
}

#[test]
fn thrown_values_are_retained_before_returning_to_the_host() {
    let (mut runtime, program, _) = initialized(
        "var callback = function(arg) { throw {payload: arg}; };
         var target = {get value() { throw {payload: 23}; }};",
    );
    let global = runtime.global_root().unwrap();
    let callback = property(&mut runtime, global, "callback");
    let target = property(&mut runtime, global, "target");
    let receiver = runtime.root(Value::UNDEFINED);
    let argument = runtime.string_rooted("host payload");
    let expected = runtime.rooted_value(argument).unwrap();
    let call_error = runtime
        .call_rooted(callback, receiver, &[argument])
        .unwrap_err();
    let call_exception = call_error.exception.unwrap();
    assert!(runtime.release_root(argument));
    runtime.collect(&program).unwrap();
    let payload = property(&mut runtime, call_exception, "payload");
    assert_eq!(runtime.rooted_value(payload), Some(expected));
    assert_eq!(
        runtime.rooted_value(call_exception),
        call_error.error.thrown_value()
    );
    let key = runtime.string_rooted("value");
    let getter_error = runtime.get_property_rooted(target, key).unwrap_err();
    let getter_exception = getter_error.exception.unwrap();
    runtime.collect(&program).unwrap();
    let getter_payload = property(&mut runtime, getter_exception, "payload");
    assert_eq!(
        runtime.rooted_value(getter_payload),
        Some(Value::number(23.0))
    );
    assert!(runtime.release_root(call_exception));
    assert!(runtime.release_root(getter_exception));
    assert!(!runtime.root_is_live(call_exception));
    assert!(!runtime.root_is_live(getter_exception));
}

#[test]
fn host_calls_use_callable_program_ownership_and_restore_the_realm() {
    let (mut runtime, program, _) = initialized(
        "var dynamic = eval('(function(arg) { return {payload: arg}; })');
         var foreign = $262.createRealm().evalScript('(function(arg) { return {realm: globalThis, payload: arg}; })');",
    );
    let global = runtime.global_root().unwrap();
    let dynamic = property(&mut runtime, global, "dynamic");
    let foreign = property(&mut runtime, global, "foreign");
    let receiver = runtime.root(Value::UNDEFINED);
    let argument = runtime.string_rooted("host argument");
    let expected = runtime.rooted_value(argument).unwrap();
    for callback in [dynamic, foreign] {
        let result = runtime
            .call_rooted(callback, receiver, &[argument])
            .unwrap();
        runtime.collect(&program).unwrap();
        let payload = property(&mut runtime, result, "payload");
        assert_eq!(runtime.rooted_value(payload), Some(expected));
        if callback == foreign {
            let realm = property(&mut runtime, result, "realm");
            assert_ne!(runtime.rooted_value(realm), runtime.rooted_value(global));
        }
        let restored_global = runtime.global_root().unwrap();
        assert_eq!(
            runtime.rooted_value(restored_global),
            runtime.rooted_value(global)
        );
    }
}

#[test]
fn initialization_and_reset_bound_embedding_roots() {
    let mut runtime = Runtime::new(Capture::default());
    assert!(runtime.global_root().is_err());
    let pre_execution = runtime.string_rooted("before initialization");
    let undefined = runtime.root(Value::UNDEFINED);
    for result in [
        runtime.get_property_rooted(undefined, pre_execution),
        runtime.call_rooted(undefined, undefined, &[]),
    ] {
        let error = result.unwrap_err();
        assert_eq!(error.to_string(), "runtime is not initialized");
        assert!(error.exception.is_none());
    }
    let error = runtime
        .set_property_rooted(undefined, pre_execution, undefined, undefined)
        .unwrap_err();
    assert_eq!(error.to_string(), "runtime is not initialized");
    assert!(error.exception.is_none());
    let program = Engine::specialize("var callback = function() {};", "reset.js").unwrap();
    runtime.execute(&program).unwrap();
    assert!(!runtime.root_is_live(pre_execution));
    let global = runtime.global_root().unwrap();
    let callback = property(&mut runtime, global, "callback");
    let key = runtime.string_rooted("A💡");
    runtime.collect(&program).unwrap();
    let length = property(&mut runtime, key, "length");
    assert_eq!(runtime.rooted_value(length), Some(Value::number(3.0)));
    runtime.execute(&program).unwrap();
    for root in [global, callback, key, length] {
        assert!(!runtime.root_is_live(root));
    }
    let receiver = runtime.root(Value::UNDEFINED);
    assert!(
        runtime
            .call_rooted(callback, receiver, &[])
            .unwrap_err()
            .exception
            .is_none()
    );
    assert!(
        runtime
            .get_property_rooted(global, key)
            .unwrap_err()
            .exception
            .is_none()
    );
    assert!(
        runtime
            .set_property_rooted(global, key, receiver, receiver)
            .unwrap_err()
            .exception
            .is_none()
    );
}
