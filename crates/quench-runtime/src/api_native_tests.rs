use super::*;
use crate::{Engine, ResidualProgram};
use crate::test_host::TestHost;
use std::{cell::RefCell, rc::Rc};

fn capturing_host() -> TestHost {
    TestHost::default()
        .with_natives(crate::host_functions![method "captured" (0) => captured])
        .with_install(|context: &mut NativeContext<'_, TestHost>| {
            let external = context.external_rooted(0x1234)?;
            assert_eq!(context.external_address_rooted(external), Some(0x1234));
            assert!(context.has_builtin_brand_rooted(external, BuiltinBrand::External)?);
            let object = context.object_rooted()?;
            assert!(!context.has_builtin_brand_rooted(object, BuiltinBrand::External)?);
            assert_eq!(context.external_address_rooted(object), None);
            let global = context.global_root()?;
            let key = context.string_rooted("external");
            context.set_property_rooted(global, key, external, global)?;
            let data = context.object_rooted()?;
            let key = context.string_rooted("answer");
            let answer = context.number(42.0);
            assert!(context.set_property_rooted(data, key, answer, data)?);
            let operation = context.host_function_with_data(HostFunctionId(0), data)?;
            let global = context.global_root()?;
            let key = context.string_rooted("captured");
            assert!(context.set_property_rooted(global, key, operation, global)?);
            Ok(())
        })
}

fn captured(
    context: &mut NativeContext<'_, TestHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    context.collect()?;
    let global = context.global_root()?;
    let key = context.string_rooted("external");
    let external = context.get_property_rooted(global, key)?;
    assert_eq!(context.external_address_rooted(external), Some(0x1234));
    assert!(context.has_builtin_brand_rooted(external, BuiltinBrand::External)?);
    let fake = context.evaluate_script_rooted("Object.create(external)", "external-prototype.js")?;
    assert!(!context.has_builtin_brand_rooted(fake, BuiltinBrand::External)?);
    // Named host re-entry must neither reset roots nor capture the calling frame.
    let wrapper = context.evaluate_script_rooted(
        "(function(value) { if (typeof callerLocal !== 'undefined') throw 1; return value; });",
        "host-reentry.js",
    )?;
    let receiver = context.undefined();
    let result = context.call_rooted(wrapper, receiver, &[data])?;
    assert_eq!(context.rooted_value(result), context.rooted_value(data));
    assert!(context.truthy_rooted(result)?);
    assert!(context.same_value_rooted(result, data)?);
    assert!(context.equal_rooted(result, data)?);
    let restored = context.host_function_data()?;
    assert_eq!(context.rooted_value(restored), context.rooted_value(data));
    context.collect()?;
    Ok(result)
}

#[test]
fn captured_host_environment_survives_scope_drop_collection_and_guest_reentry() {
    let mut runtime = Runtime::new(capturing_host());
    let program = Engine::specialize(
        "function caller() { let callerLocal = 7; return captured(); } var result = caller();",
        "host-capture.js",
    )
    .unwrap();
    runtime.execute(&program).unwrap();
    runtime.collect(&program).unwrap();
    let global = runtime.global_root().unwrap();
    let key = runtime.string_rooted("external");
    let external = runtime.get_property_rooted(global, key).unwrap();
    assert!(runtime.rooted_value(external).is_some());
    let key = runtime.string_rooted("result");
    let data = runtime.get_property_rooted(global, key).unwrap();
    let key = runtime.string_rooted("answer");
    let answer = runtime.get_property_rooted(data, key).unwrap();
    assert_eq!(
        runtime.rooted_value(answer).unwrap().as_number(),
        Some(42.0)
    );
    let key = runtime.string_rooted("captured");
    let operation = runtime.get_property_rooted(global, key).unwrap();
    let receiver = runtime.root(Value::UNDEFINED);
    let again = runtime.call_rooted(operation, receiver, &[]).unwrap();
    assert_eq!(runtime.rooted_value(again), runtime.rooted_value(data));
    runtime.execute(&program).unwrap();
    assert!(!runtime.root_is_live(data));
    assert!(!runtime.root_is_live(operation));
}

const ECHO: HostFunctionId = HostFunctionId(0);
const REENTER: HostFunctionId = HostFunctionId(1);
const REJECT: HostFunctionId = HostFunctionId(2);
const ESCAPED: HostFunctionId = HostFunctionId(3);
const TRAP: HostFunctionId = HostFunctionId(4);
const CONSTRUCT: HostFunctionId = HostFunctionId(5);
const COMPARE: HostFunctionId = HostFunctionId(6);

#[derive(Clone, Copy, Default)]
enum Initialization {
    #[default]
    Install,
    Throw,
    Foreign(RootId),
    Trap,
    Unrooted(Value),
}

#[derive(Default)]
struct InitializerState {
    mode: Initialization,
    scoped: Vec<RootId>,
}


fn initializing_host(state: Rc<RefCell<InitializerState>>) -> TestHost {
    TestHost::default()
        .with_state(state)
        .with_install(|context: &mut NativeContext<'_, TestHost>| {
            let object = context.object_rooted()?;
            let global = context.global_root()?;
            let key = context.string_rooted("installed");
            let state = context.host_mut().state::<RefCell<InitializerState>>();
            state.borrow_mut().scoped.extend([object, global, key]);
            context.collect()?;
            let mode = state.borrow().mode;
            match mode {
                Initialization::Install => {
                    assert!(context.set_property_rooted(global, key, object, global)?);
                    context.collect()
                }
                Initialization::Throw => {
                    let error = context.throw(object);
                    state.borrow_mut().scoped.extend(error.exception);
                    context.collect()?;
                    Err(error)
                }
                Initialization::Foreign(root) => {
                    let mut error = RootedError::host("foreign initializer exception");
                    error.exception = Some(root);
                    Err(error)
                }
                Initialization::Trap => typed_trap(context, global, &[]).map(drop),
                Initialization::Unrooted(value) => Err(RootedError {
                    error: JsError::thrown(value, "unrooted initializer exception".into()),
                    exception: None,
                }),
            }
        })
}

#[test]
fn host_initialization_scopes_objects_and_validates_exceptions_before_guest_execution() {
    let host = initializing_host(Rc::default());
    let mut runtime = Runtime::new(host.clone());
    let program = Engine::specialize("var reached = installed;", "host-initialization.js").unwrap();
    for _ in 0..2 {
        runtime.execute(&program).unwrap();
        let global = runtime.global_root().unwrap();
        let key = runtime.string_rooted("reached");
        let result = runtime.get_property_rooted(global, key).unwrap();
        let initialized = runtime.string_rooted("installed");
        let object = runtime.get_property_rooted(global, initialized).unwrap();
        runtime.collect(&program).unwrap();
        assert_eq!(runtime.rooted_value(result), runtime.rooted_value(object));
        for root in &host.state::<RefCell<InitializerState>>().borrow().scoped {
            assert!(!runtime.root_is_live(*root));
        }
    }
    host.state::<RefCell<InitializerState>>().borrow_mut().mode = Initialization::Throw;
    let error = runtime.execute(&program).unwrap_err();
    let exception = runtime.root(error.thrown_value().unwrap());
    runtime.collect(&program).unwrap();
    assert!(runtime.root_is_live(exception));
    for root in &host.state::<RefCell<InitializerState>>().borrow().scoped {
        assert!(!runtime.root_is_live(*root));
    }
    let global = runtime.global_root().unwrap();
    let key = runtime.string_rooted("reached");
    let result = runtime.get_property_rooted(global, key).unwrap();
    assert!(runtime.rooted_value(result).unwrap().is_undefined());
    let mut other = Runtime::new(state_host(Rc::default()));
    let foreign = other.root(Value::number(1.0));
    host.state::<RefCell<InitializerState>>().borrow_mut().mode = Initialization::Foreign(foreign);
    let error = runtime.execute(&program).unwrap_err();
    assert_eq!(error.to_string(), "released or foreign embedding root");
    assert!(error.thrown_value().is_none());
    for root in &host.state::<RefCell<InitializerState>>().borrow().scoped {
        assert!(!runtime.root_is_live(*root));
    }
    host.state::<RefCell<InitializerState>>().borrow_mut().mode = Initialization::Trap;
    let error = runtime.execute(&program).unwrap_err();
    assert_eq!(error.wasm_trap(), Some(crate::WasmTrap::Unreachable));
    host.state::<RefCell<InitializerState>>().borrow_mut().mode = Initialization::Unrooted(other.rooted_value(foreign).unwrap());
    let error = runtime.execute(&program).unwrap_err();
    assert!(error.thrown_value().is_none());
    assert_eq!(error.to_string(), "unrooted initializer exception");
}

#[derive(Default)]
struct State {
    borrowed: Vec<RootId>,
    retained: Vec<RootId>,
    invalid_result: Option<RootId>,
    invalid_exception: bool,
    calls: usize,
}


fn state_host(state: Rc<RefCell<State>>) -> TestHost {
    TestHost::default()
        .with_state(state)
        .with_natives(crate::host_functions![
            #[cfg(not(any()))]
            global "echo" (1) => echo,
            #[cfg(any())]
            method "unavailable" (0) => unavailable_platform::callback,
            method "reenter" (1) => reenter,
            method "reject" (1) => reject,
            method "escaped" (0) => escaped,
            method "trap" (0) => typed_trap,
            method "construct" (2) => construct,
            method "compare" (2) => compare,
            global "collectJSON" (0) => collect_json,
        ])
}

fn typed_trap<H: Host>(
    _: &mut NativeContext<'_, H>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    Err(RootedError {
        error: JsError::wasm_trap_error(crate::WasmTrap::Unreachable),
        exception: None,
    })
}

fn echo(
    ctx: &mut NativeContext<'_, TestHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let retained = ctx.retain(args[0])?;
    let state = ctx.host_mut().state::<RefCell<State>>();
    {
        let mut state = state.borrow_mut();
        state.borrowed.extend([receiver, args[0]]);
        state.retained.push(retained);
        state.calls += 1;
    }
    ctx.collect()?;
    Ok(args[0])
}

fn reenter(
    ctx: &mut NativeContext<'_, TestHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let result = ctx.call_rooted(args[0], receiver, &args[1..]);
    ctx.collect()?;
    let root = match &result {
        Ok(root) => Some(*root),
        Err(error) => error.exception,
    };
    ctx.host_mut().state::<RefCell<State>>().borrow_mut().borrowed.extend(root);
    result
}

fn reject(
    ctx: &mut NativeContext<'_, TestHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    ctx.collect()?;
    Err(ctx.throw(args[0]))
}

fn construct(
    ctx: &mut NativeContext<'_, TestHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let result = ctx.construct_rooted(args[0], args[1], &args[2..]);
    ctx.collect()?;
    let state = ctx.host_mut().state::<RefCell<State>>();
    state.borrow_mut().borrowed.extend_from_slice(args);
    match &result {
        Ok(root) => state.borrow_mut().borrowed.push(*root),
        Err(error) => state.borrow_mut().borrowed.extend(error.exception),
    }
    result
}

#[test]
fn native_construction_reentry_scopes_results_and_retains_thrown_identity() {
    let (mut runtime, program, host) = initialized(
        "var payload = {}; var Target = function() {};\n         var Constructor = eval('(function(value) { this.payload = echo(value); })');\n         var Throwing = eval('(function(value) { throw {payload: echo(value)}; })');",
    );
    let global = runtime.global_root().unwrap();
    let payload = property(&mut runtime, global, "payload");
    let target = property(&mut runtime, global, "Target");
    let constructor = property(&mut runtime, global, "Constructor");
    let throwing = property(&mut runtime, global, "Throwing");
    let callback = runtime.host_function(CONSTRUCT).unwrap();
    let receiver = runtime.root(Value::UNDEFINED);
    let instance = runtime
        .call_rooted(callback, receiver, &[constructor, target, payload])
        .unwrap();
    let error = runtime
        .call_rooted(callback, receiver, &[throwing, target, payload])
        .unwrap_err();
    let exception = error.exception.unwrap();
    runtime.collect(&program).unwrap();
    for root in [instance, exception] {
        let value = property(&mut runtime, root, "payload");
        assert_eq!(runtime.rooted_value(value), runtime.rooted_value(payload));
        assert!(runtime.release_root(value));
    }
    assert_eq!(runtime.rooted_value(exception), error.error.thrown_value());
    for root in &host.state::<RefCell<State>>().borrow().borrowed {
        assert!(!runtime.root_is_live(*root));
    }
    runtime.execute(&program).unwrap();
    for root in [instance, exception, callback] {
        assert!(!runtime.root_is_live(root));
    }
}

#[test]
fn native_backed_ordinary_constructors_preserve_completion_and_new_target() {
    let (mut runtime, _program, _host) = initialized(
        "var payload = {}; var Target = function() {};",
    );
    let global = runtime.global_root().unwrap();
    let payload = property(&mut runtime, global, "payload");
    let target = property(&mut runtime, global, "Target");
    let constructor = runtime.host_constructor(ECHO).unwrap();
    let prototype = property(&mut runtime, constructor, "prototype");
    let linked_constructor = property(&mut runtime, prototype, "constructor");
    assert_eq!(runtime.rooted_value(linked_constructor), runtime.rooted_value(constructor));
    let callback = runtime.host_function(CONSTRUCT).unwrap();
    let receiver = runtime.root(Value::UNDEFINED);
    let number = runtime.root(Value::number(42.0));
    let instance = runtime.call_rooted(callback, receiver, &[constructor, target, number]).unwrap();
    let object = property(&mut runtime, global, "Object");
    let get_prototype = property(&mut runtime, object, "getPrototypeOf");
    let object_prototype = runtime.call_rooted(get_prototype, object, &[instance]).unwrap();
    let target_prototype = property(&mut runtime, target, "prototype");
    assert_eq!(runtime.rooted_value(object_prototype), runtime.rooted_value(target_prototype));
    let returned = runtime.call_rooted(callback, receiver, &[constructor, target, payload]).unwrap();
    assert_eq!(runtime.rooted_value(returned), runtime.rooted_value(payload));
    let original_prototype = runtime.call_rooted(get_prototype, object, &[payload]).unwrap();
    assert_ne!(runtime.rooted_value(original_prototype), runtime.rooted_value(target_prototype));

    // Adding a public prototype does not change the private [[Construct]] fact.
    let ordinary = runtime.host_function(ECHO).unwrap();
    let key = runtime.string_rooted("prototype");
    assert!(runtime.set_property_rooted(ordinary, key, target_prototype, ordinary).unwrap());
    assert!(runtime.call_rooted(callback, receiver, &[ordinary, ordinary, number]).is_err());
}

fn escaped(
    ctx: &mut NativeContext<'_, TestHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let state = ctx.host_mut().state::<RefCell<State>>();
    let mut state = state.borrow_mut();
    state.borrowed.push(receiver);
    let root = state.invalid_result.unwrap();
    if state.invalid_exception {
        Err(RootedError {
            error: JsError("invalid host exception".into()),
            exception: Some(root),
        })
    } else {
        Ok(root)
    }
}

fn initialized(source: &str) -> (Runtime<TestHost>, ResidualProgram, TestHost) {
    let host = state_host(Rc::default());
    let mut runtime = Runtime::new(host.clone());
    let program = Engine::specialize(source, "native-host.js").unwrap();
    runtime.execute(&program).unwrap();
    (runtime, program, host)
}

fn property(runtime: &mut Runtime<TestHost>, object: RootId, name: &str) -> RootId {
    let key = runtime.string_rooted(name);
    let result = runtime.get_property_rooted(object, key).unwrap();
    assert!(runtime.release_root(key));
    result
}

#[test]
fn callback_scopes_expire_but_promoted_handles_keep_guest_identity() {
    let (mut runtime, program, host) = initialized("var value = {}; var result = echo(value);");
    let global = runtime.global_root().unwrap();
    let value = property(&mut runtime, global, "value");
    let result = property(&mut runtime, global, "result");
    assert_eq!(runtime.rooted_value(value), runtime.rooted_value(result));
    assert_eq!(host.state::<RefCell<State>>().borrow().calls, 1);
    for root in &host.state::<RefCell<State>>().borrow().borrowed {
        assert!(!runtime.root_is_live(*root));
    }
    let retained = host.state::<RefCell<State>>().borrow().retained[0];
    runtime.collect(&program).unwrap();
    assert_eq!(runtime.rooted_value(retained), runtime.rooted_value(result));
    assert!(runtime.release_root(retained));
    let callback = runtime.host_function(ECHO).unwrap();
    let receiver = runtime.root(Value::UNDEFINED);
    let result = runtime.call_rooted(callback, receiver, &[value]).unwrap();
    assert_eq!(runtime.rooted_value(result), runtime.rooted_value(value));
    for root in &host.state::<RefCell<State>>().borrow().borrowed {
        assert!(!runtime.root_is_live(*root));
    }
    let retained = host.state::<RefCell<State>>().borrow().retained[1];
    runtime.execute(&program).unwrap();
    assert!(!runtime.root_is_live(retained));
    assert!(!runtime.root_is_live(callback));
}

#[test]
fn native_reentry_preserves_program_ownership_and_propagates_retained_exceptions() {
    let (mut runtime, program, host) = initialized(
        "var argument = {}; var callback = eval('(function(value) { return {payload: value}; })');
         var throwing = eval('(function(value) { throw {payload: value}; })');",
    );
    let global = runtime.global_root().unwrap();
    let callback = property(&mut runtime, global, "callback");
    let throwing = property(&mut runtime, global, "throwing");
    let argument = property(&mut runtime, global, "argument");
    let receiver = runtime.root(Value::UNDEFINED);
    let reenter = runtime.host_function(REENTER).unwrap();
    let result = runtime
        .call_rooted(reenter, receiver, &[callback, argument])
        .unwrap();
    runtime.collect(&program).unwrap();
    let payload = property(&mut runtime, result, "payload");
    assert_eq!(
        runtime.rooted_value(payload),
        runtime.rooted_value(argument)
    );
    for root in &host.state::<RefCell<State>>().borrow().borrowed {
        assert!(!runtime.root_is_live(*root));
    }
    let rejecting = runtime.host_function(REJECT).unwrap();
    let error = runtime
        .call_rooted(rejecting, receiver, &[result])
        .unwrap_err();
    let exception = error.exception.unwrap();
    assert!(runtime.release_root(result));
    runtime.collect(&program).unwrap();
    let payload = property(&mut runtime, exception, "payload");
    assert_eq!(
        runtime.rooted_value(payload),
        runtime.rooted_value(argument)
    );
    assert!(runtime.release_root(exception));
    let error = runtime
        .call_rooted(reenter, receiver, &[throwing, argument])
        .unwrap_err();
    let exception = error.exception.unwrap();
    runtime.collect(&program).unwrap();
    let payload = property(&mut runtime, exception, "payload");
    assert_eq!(
        runtime.rooted_value(payload),
        runtime.rooted_value(argument)
    );
    for root in &host.state::<RefCell<State>>().borrow().borrowed {
        assert!(!runtime.root_is_live(*root));
    }
    assert!(runtime.release_root(exception));
}

#[test]
fn callbacks_cannot_return_foreign_or_released_handles() {
    let (mut runtime, _, host) = initialized("");
    let mut other = Runtime::new(state_host(Rc::default()));
    let foreign = other.root(Value::number(1.0));
    let stale = runtime.root(Value::number(2.0));
    assert!(runtime.release_root(stale));
    let receiver = runtime.root(Value::UNDEFINED);
    let callback = runtime.host_function(ESCAPED).unwrap();
    for exception in [false, true] {
        host.state::<RefCell<State>>().borrow_mut().invalid_exception = exception;
        for invalid in [foreign, stale] {
            host.state::<RefCell<State>>().borrow_mut().invalid_result = Some(invalid);
            let error = runtime.call_rooted(callback, receiver, &[]).unwrap_err();
            assert!(error.exception.is_none());
            assert_eq!(error.to_string(), "released or foreign embedding root");
            for root in &host.state::<RefCell<State>>().borrow().borrowed {
                assert!(!runtime.root_is_live(*root));
            }
        }
    }
    assert!(
        runtime
            .host_function(HostFunctionId(u32::MAX))
            .unwrap_err()
            .exception
            .is_none()
    );
    let trap = runtime.host_function(TRAP).unwrap();
    let error = runtime.call_rooted(trap, receiver, &[]).unwrap_err();
    assert_eq!(error.error.wasm_trap(), Some(crate::WasmTrap::Unreachable));
    assert!(error.exception.is_none());
}

fn compare(
    context: &mut NativeContext<'_, TestHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    context.collect()?;
    let result = context.equal_rooted(args[0], args[1]);
    context.collect()?;
    if let Err(error) = &result {
        context
            .host_mut()
            .state::<RefCell<State>>()
            .borrow_mut()
            .borrowed
            .extend(error.exception);
    }
    result.map(|equal| context.boolean(equal))
}

#[test]
fn native_comparison_preserves_coercion_exception_roots_through_collection() {
    let (mut runtime, program, host) = initialized(
        "var payload = {}; var input = { [Symbol.toPrimitive]() { throw echo(payload); } };",
    );
    let global = runtime.global_root().unwrap();
    let payload = property(&mut runtime, global, "payload");
    let input = property(&mut runtime, global, "input");
    let primitive = runtime.root(Value::number(f64::NAN));
    let receiver = runtime.root(Value::UNDEFINED);
    let callback = runtime.host_function(COMPARE).unwrap();
    for operands in [[input, primitive], [primitive, input]] {
        let error = runtime
            .call_rooted(callback, receiver, &operands)
            .unwrap_err();
        let exception = error.exception.unwrap();
        runtime.collect(&program).unwrap();
        assert_eq!(
            runtime.rooted_value(exception),
            runtime.rooted_value(payload)
        );
        assert_eq!(runtime.rooted_value(exception), error.error.thrown_value());
        assert!(runtime.release_root(exception));
        for root in &host.state::<RefCell<State>>().borrow().borrowed {
            assert!(!runtime.root_is_live(*root));
        }
    }
}

#[test]
fn native_value_operations_reject_foreign_and_released_roots() {
    let (mut runtime, _, _) = initialized("");
    let mut other = Runtime::new(state_host(Rc::default()));
    let foreign = other.root(Value::TRUE);
    let stale = runtime.root(Value::TRUE);
    assert!(runtime.release_root(stale));
    let valid = runtime.root(Value::TRUE);
    let mut context = NativeContext::new(&mut runtime.vm);
    for invalid in [foreign, stale] {
        for result in [
            context.truthy_rooted(invalid),
            context.same_value_rooted(invalid, valid),
            context.same_value_rooted(valid, invalid),
            context.equal_rooted(invalid, valid),
            context.equal_rooted(valid, invalid),
        ] {
            let error = result.unwrap_err();
            assert_eq!(error.to_string(), "released or foreign embedding root");
            assert!(error.exception.is_none());
        }
    }
}

#[test]
fn intrinsic_json_serialization_preserves_hooks_exceptions_and_scoped_roots() {
    let (mut runtime, program, _) = initialized(
        r#"
var marker={tag:'original'},calls=[];
var object={toJSON(key){calls.push(key);return {get value(){collectJSON();return 7}}}};
var throwing={toJSON(){collectJSON();throw marker}};
JSON.stringify=()=>{throw 'public alias'};
"#,
    );
    let global = runtime.global_root().unwrap();
    let object = property(&mut runtime, global, "object");
    let throwing = property(&mut runtime, global, "throwing");
    let marker = property(&mut runtime, global, "marker");
    let serialized;
    {
        let mut context = NativeContext::new(&mut runtime.vm);
        serialized = context.stringify_json_rooted(object).unwrap();
        assert_eq!(
            context.string_text(serialized).unwrap().as_deref(),
            Some("{\"value\":7}")
        );
        let error = context.stringify_json_rooted(throwing).unwrap_err();
        assert_eq!(
            context.rooted_value(error.exception.unwrap()),
            context.rooted_value(marker)
        );
        assert_eq!(error.error.thrown_value(), context.rooted_value(marker));
        context.release_root(error.exception.unwrap());
        let undefined = context.undefined();
        let output = context.stringify_json_rooted(undefined).unwrap();
        assert!(context.rooted_value(output).unwrap().is_undefined());
    }
    assert!(!runtime.root_is_live(serialized));
    runtime.collect(&program).unwrap();
    assert!(runtime.root_is_live(marker));
}

fn collect_json(
    context: &mut NativeContext<'_, TestHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    context.collect()?;
    Ok(context.undefined())
}



fn restart_intrinsics_host() -> TestHost {
    TestHost::default()
        .with_install(|context: &mut NativeContext<'_, TestHost>| {
            context.with_startup_intrinsics(|context| {
                context.evaluate_script_rooted(
                    "Array.bootstrapRealmMarker = { fresh: true };",
                    "restart-intrinsics-bootstrap.js",
                )
            })?;
            context.seal_startup_intrinsics();
            Ok(())
        })
}

#[test]
fn startup_intrinsics_are_reset_before_a_replacement_realm_bootstraps() {
    let source = "if (!Array.bootstrapRealmMarker.fresh) throw new Error('stale bootstrap'); Array.bootstrapRealmMarker.fresh = false;";
    for program in [Engine::compile(crate::ExecutionRequest { source, name: "restart-intrinsics.js", kind: crate::SourceKind::Script }).unwrap(), Engine::specialize(source, "restart-intrinsics.js").unwrap()] {
        let mut runtime = Runtime::new(restart_intrinsics_host());
        for _ in 0..2 {
            runtime.execute(&program).unwrap();
        }
    }
}

const LAZY_GROUP_SCRIPT: &str = "globalThis.inits = (globalThis.inits || 0) + 1; \
    globalThis.A = 'a'; globalThis.B = 'b'; \
    Object.defineProperty(globalThis, 'C', { value: 'c', writable: true, configurable: true });";

fn lazy_host() -> TestHost {
    TestHost::default()
        .with_natives(crate::host_functions![
            method "initializeGroup" (0) => initialize_lazy_group,
            method "initializeSingle" (0) => initialize_lazy_single,
            method "initializeIntegrity" (0) => initialize_lazy_integrity,
        ])
        .with_install(|context: &mut NativeContext<'_, TestHost>| {
            let global = context.global_root()?;
            let marker = context.string_rooted("before");
            let key = context.string_rooted("before");
            assert!(context.set_property_rooted(global, key, marker, global)?);
            for (operation, names) in [(0, &["A", "B", "C"][..]), (1, &["D"][..])] {
                let initializer = context.host_function(HostFunctionId(operation))?;
                context.define_lazy_data_properties_rooted(global, names, false, initializer)?;
            }
            let key = context.string_rooted("after");
            assert!(context.set_property_rooted(global, key, marker, global)?);
            for (name, property, indexed) in [("lazyIntegrity", "value", false), ("lazyArrayIntegrity", "0", true)] {
                let holder = if indexed { context.array_rooted(&[])? } else { context.object_rooted()? };
                let key = context.string_rooted(property);
                let data = context.array_rooted(&[holder, key])?;
                let initializer = context.host_function_with_data(HostFunctionId(2), data)?;
                context.define_lazy_data_properties_rooted(holder, &[property], true, initializer)?;
                let key = context.string_rooted(name);
                assert!(context.set_property_rooted(global, key, holder, global)?);
            }
            context.seal_startup_intrinsics();
            Ok(())
        })
}

fn initialize_lazy_group(
    context: &mut NativeContext<'_, TestHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    context.evaluate_script_rooted(LAZY_GROUP_SCRIPT, "lazy-group.js")
}

fn initialize_lazy_single(
    context: &mut NativeContext<'_, TestHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    context.evaluate_script_rooted("globalThis.D = 'd';", "lazy-single.js")
}

fn initialize_lazy_integrity(
    context: &mut NativeContext<'_, TestHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    context.evaluate_script_rooted(
        "globalThis.integrityInits=(globalThis.integrityInits||0)+1",
        "lazy-integrity.js",
    )?;
    let data = context.host_function_data()?;
    let key = context.string_rooted("0");
    let holder = context.get_property_rooted(data, key)?;
    let key = context.string_rooted("1");
    let key = context.get_property_rooted(data, key)?;
    let value = context.number(42.0);
    context.define_data_property_rooted(holder, key, value, true, true, true)?;
    Ok(context.undefined())
}

#[test]
fn lazy_data_properties_materialize_before_integrity_descriptor_changes() {
    for (holder, key) in [("lazyIntegrity", "value"), ("lazyArrayIntegrity", "0")] {
        for (operation, writable, configurable) in [
            ("Object.freeze(target)", false, false),
            ("Object.seal(target)", true, false),
            (
                "Object.preventExtensions(target); if(Object.isFrozen(target))throw Error('frozen')",
                true,
                true,
            ),
            (
                "Object.preventExtensions(target); if(Object.isSealed(target))throw Error('sealed')",
                true,
                true,
            ),
        ] {
            let source = format!(
                r#"
          const target=globalThis[{holder:?}],key={key:?};
          {operation};
          if(integrityInits!==1)throw Error('integrity query did not observe the descriptor');
          const descriptor=Object.getOwnPropertyDescriptor(target,key);
          if(descriptor.value!==42||descriptor.writable!=={writable}||descriptor.configurable!=={configurable}||!descriptor.enumerable)throw Error('integrity descriptor');
          if(target[key]!==42||integrityInits!==1)throw Error('repeated initialization');
        "#
            );
            for program in [
                Engine::compile(crate::ExecutionRequest {
                    source: &source,
                    name: "lazy-integrity.js",
                    kind: crate::SourceKind::Script,
                })
                .unwrap(),
                Engine::specialize(&source, "lazy-integrity.js").unwrap(),
            ] {
                Runtime::new(lazy_host()).execute(&program).unwrap_or_else(|error| panic!("{holder}: {operation}: {error:?}"));
            }
        }
    }
}

#[test]
fn lazy_data_properties_materialize_once_as_data_in_place() {
    let mut runtime = Runtime::new(lazy_host());
    let program = Engine::specialize(
        r#"
const check = (ok, message) => { if (!ok) throw new Error(message); };
const names = Object.getOwnPropertyNames(globalThis);
const at = names.indexOf("before");
check(names.slice(at, at + 6).join() === "before,A,B,C,D,after", "definition order");
check(!Object.keys(globalThis).includes("A"), "non-enumerable");
check(globalThis.inits === undefined, "lazy until observed");
globalThis.B = 5;
delete globalThis.C;
check(globalThis.inits === undefined, "assignment and delete need no value");
const a = Object.getOwnPropertyDescriptor(globalThis, "A");
check(globalThis.inits === 1, "one initialization");
check(a.value === "a" && a.writable && !a.enumerable && a.configurable, "data descriptor");
check(B === 5 && !("C" in globalThis), "script changes survive initialization");
check(A === "a" && globalThis.inits === 1, "initialized once");
Object.defineProperty(globalThis, "D", { enumerable: true });
const d = Object.getOwnPropertyDescriptor(globalThis, "D");
check(d.value === "d" && d.writable && d.enumerable, "partial definition keeps the value");
const after = Object.getOwnPropertyNames(globalThis);
check(after.slice(after.indexOf("before"), after.indexOf("before") + 5).join() === "before,A,B,D,after", "order kept");
var result = "ok";
"#,
        "lazy-data.js",
    )
    .unwrap();
    runtime.execute(&program).unwrap();
    let global = runtime.global_root().unwrap();
    let key = runtime.string_rooted("result");
    let result = runtime.get_property_rooted(global, key).unwrap();
    assert!(
        runtime
            .rooted_value(result)
            .is_some_and(|value| !value.is_undefined())
    );
}

fn number_coercion_host() -> TestHost {
    TestHost::default()
        .with_natives(crate::host_functions![global "nativeNumber" (1) => native_number_coercion])
}
fn native_number_coercion(context: &mut NativeContext<'_, TestHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    context.collect()?;
    let number = context.to_number(args[0])?;
    Ok(context.number(number))
}
#[test]
fn native_number_conversion_uses_original_hooks_and_shared_language_errors() {
    let source = r#"
      const log=[];
      const value={ [Symbol.toPrimitive](hint){log.push(hint);return '0x10';} };
      if(nativeNumber(value)!==16||log.join()!=='number')throw Error('number coercion changed');
      if(!Number.isNaN(nativeNumber(undefined))||!Object.is(nativeNumber('-0'),-0))throw Error('numeric primitive changed');
      const marker={};try{nativeNumber({valueOf(){throw marker;}});throw Error('missing exception');}catch(error){if(error!==marker)throw error;}
      for(const value of [Symbol('x'),1n]) {try{nativeNumber(value);throw Error('missing type error');}catch(error){if(!(error instanceof TypeError))throw error;}}
    "#;
    for program in [
        Engine::compile(crate::ExecutionRequest { source, name: "native-number.js", kind: crate::SourceKind::Script }).unwrap(),
        Engine::specialize(source,"native-number.js").unwrap(),
    ] { Runtime::new(number_coercion_host()).execute(&program).unwrap(); }
}

fn instance_of_host() -> TestHost {
    TestHost::default()
        .with_natives(crate::host_functions![global "nativeInstanceOf" (2) => native_instanceof])
}
fn native_instanceof(
    context: &mut NativeContext<'_, TestHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    context.collect()?;
    let yes = context.instance_of_rooted(args[0], args[1])?;
    Ok(context.boolean(yes))
}
#[test]
fn native_instanceof_preserves_original_hooks_proxy_effects_and_abrupt_completions() {
    let source = r#"
    function C(){};const object=new C(),marker={},log=[];
    if(!nativeInstanceOf(object,C)||nativeInstanceOf({},C)||nativeInstanceOf(null,C))throw Error('ordinary membership');
    const constructor={get [Symbol.hasInstance](){log.push('get');return function(value){log.push(this===constructor,value===object);return marker;};}};
    if(!nativeInstanceOf(object,constructor)||JSON.stringify(log)!=='["get",true,true]')throw Error('original hook');
    const proxy=new Proxy(object,{getPrototypeOf(){throw marker;}});
    try{nativeInstanceOf(proxy,C);throw Error('missing proxy error');}catch(error){if(error!==marker)throw error;}
    try{nativeInstanceOf(object,{[Symbol.hasInstance](){throw marker;}});throw Error('missing hook error');}catch(error){if(error!==marker)throw error;}
    for(const constructor of [null,{}, {[Symbol.hasInstance]:1}]){try{nativeInstanceOf(object,constructor);throw Error('missing type error');}catch(error){if(!(error instanceof TypeError))throw error;}}
    "#;
    for program in [
        Engine::compile(crate::ExecutionRequest {
            source,
            name: "native-instanceof.js",
            kind: crate::SourceKind::Script,
        })
        .unwrap(),
        Engine::specialize(source, "native-instanceof.js").unwrap(),
    ] {
        Runtime::new(instance_of_host()).execute(&program).unwrap();
    }
}

fn caller_sources_host() -> TestHost {
    TestHost::default()
        .with_natives(crate::host_functions![global "nativeCallerSources" (0) => native_caller_sources, global "nativeDynamicCaller" (0) => native_dynamic_caller])
}
fn native_caller_sources(context: &mut NativeContext<'_, TestHost>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    context.collect()?;
    let sources=context.call_stack_source_names();
    let sources=sources.iter().map(|source|context.string_rooted(source)).collect::<Vec<_>>();
    context.array_rooted(&sources)
}
fn native_dynamic_caller(context: &mut NativeContext<'_, TestHost>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    context.evaluate_script_rooted("nativeCallerSources()", "dynamic-caller-source.js")
}
#[test]
fn native_call_stack_source_names_follow_original_compiler_metadata_after_collection() {
    let source=r#"
    function inner(){return nativeCallerSources();}function outer(){return inner();}
    const names=outer();if(names.length===0||names.some(name=>name!=='native-caller-sources.js'))throw Error('caller metadata changed');
    const dynamic=nativeDynamicCaller();if(dynamic[0]!=='dynamic-caller-source.js'||!dynamic.includes('native-caller-sources.js'))throw Error('dynamic caller metadata changed');
    "#;
    for program in [Engine::compile(crate::ExecutionRequest{source,name:"native-caller-sources.js",kind:crate::SourceKind::Script}).unwrap(),Engine::specialize(source,"native-caller-sources.js").unwrap()] {
        Runtime::new(caller_sources_host()).execute(&program).unwrap();
    }
}

fn snapshot_metadata_host() -> TestHost {
    TestHost::default()
        .with_natives(crate::host_functions![global "nativeSnapshotMetadata" (1) => native_snapshot_metadata, global "nativeCollectionPreview" (1) => native_collection_preview])
        .with_install(|context: &mut NativeContext<'_, TestHost>| { context.seal_startup_intrinsics(); Ok(()) })
}
fn native_collection_preview(context: &mut NativeContext<'_, TestHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    let Some(preview) = context.collection_preview_rooted(value)? else { return Ok(context.undefined()); };
    if args.len() > 1 { context.evaluate_script_rooted("previewOwner.clear()", "clear-preview-owner.js")?; }
    context.collect()?;
    let values = context.array_rooted(&preview.entries)?;
    let pairs = context.boolean(preview.is_key_value);
    context.array_rooted(&[values, pairs])
}
fn native_snapshot_metadata(context: &mut NativeContext<'_, TestHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    let target = context.proxy_target_rooted(value)?.unwrap_or_else(|| context.undefined());
    let (detail_target, handler) = context.proxy_details_rooted(value)?
        .unwrap_or_else(|| (context.undefined(), context.undefined()));
    context.collect()?;
    let object = context.boolean(context.has_startup_global_name("Object"));
    let added = context.boolean(context.has_startup_global_name("AddedAfterBootstrap"));
    context.array_rooted(&[object, added, target, detail_target, handler])
}
#[test]
fn native_bootstrap_names_and_proxy_targets_survive_mutation_revocation_and_collection() {
    let source=r#"
      delete globalThis.Object;globalThis.AddedAfterBootstrap=function AddedAfterBootstrap(){};
      let traps=0;
      const target={marker:42};
      const handler={get(){traps++;throw Error('introspection invoked get');},ownKeys(){traps++;throw Error('introspection invoked ownKeys');}};
      const pair=Proxy.revocable(target,handler);
      const info=nativeSnapshotMetadata(pair.proxy);
      if(info[0]!==true||info[1]!==false||info[2]!==target||info[3]!==target||info[4]!==handler||traps!==0)throw Error('snapshot or proxy slots changed');
      pair.revoke();const revoked=nativeSnapshotMetadata(pair.proxy);if(revoked[2]!==null||revoked[3]!==null||revoked[4]!==null||traps!==0)throw Error('revoked slots changed');
      if(nativeSnapshotMetadata(target)[2]!==undefined)throw Error('ordinary object acquired proxy slots');
      const map=new Map([[1,'a'],[2,'b'],[3,'c']]);const iterator=map.entries();iterator.next();map.delete(2);map.set(4,'d');
      const preview=nativeCollectionPreview(iterator);
      if(preview[1]!==true||preview[0].join(',')!=='3,c,4,d'||iterator.next().value.join(',')!=='3,c')throw Error('preview consumed iterator or ignored live slots');
      const setPreview=nativeCollectionPreview(new Set([42]).entries());
      if(setPreview[1]!==true||setPreview[0].join(',')!=='42,42')throw Error('Set pair preview changed');
      if(nativeCollectionPreview(new Proxy(map,handler))!==undefined||traps!==0)throw Error('preview invoked proxy traps');
      globalThis.previewOwner=new Map([[{key:42},{marker:99}]]);
      const retained=nativeCollectionPreview(previewOwner,true);
      if(previewOwner.size!==0||retained[0][0].key!==42||retained[0][1].marker!==99)throw Error('preview roots lost cleared entries');
    "#;
    for program in [Engine::compile(crate::ExecutionRequest{source,name:"native-snapshot-metadata.js",kind:crate::SourceKind::Script}).unwrap(),Engine::specialize(source,"native-snapshot-metadata.js").unwrap()] {
        Runtime::new(snapshot_metadata_host()).execute(&program).unwrap();
    }
}

fn bounded_view_host() -> TestHost {
    TestHost::default()
        .with_natives(crate::host_functions![global "nativeBoundedView" (1) => native_bounded_view])
}
fn native_bounded_view(context:&mut NativeContext<'_,TestHost>,_:RootId,args:&[RootId])->Result<RootId,RootedError>{
    let value=args.first().copied().unwrap_or_else(||context.undefined());
    let keys=context.own_non_index_keys_rooted(value,false)?;context.collect()?;
    let bytes=context.bytes_prefix_rooted(value,2).ok_or_else(||RootedError::host("buffer bytes missing"))?;
    let bytes=context.uint8_array_rooted(&bytes)?;let keys=context.array_rooted(&keys)?;context.array_rooted(&[bytes,keys])
}
#[test]
fn native_bounded_byte_snapshots_and_non_index_keys_preserve_views_and_gc_roots(){
    let source=r#"
      const ab=new Uint8Array([1,2,3,4]).buffer,view=new Uint8Array(ab,1,3),symbol=Symbol('key');view.extra=1;view[symbol]=2;Object.defineProperty(view,'hidden',{value:3});
      const result=nativeBoundedView(view);
      if(result[0].join()!=='2,3'||result[1].length!==3||result[1][0]!=='extra'||result[1][1]!=='hidden'||result[1][2]!==symbol)throw Error('view bytes or keys changed');
      if(nativeBoundedView(ab)[0].join()!=='1,2')throw Error('backing prefix changed');
      if(nativeBoundedView(new DataView(ab,2,2))[0].join()!=='3,4')throw Error('DataView prefix changed');
    "#;
    for program in [Engine::compile(crate::ExecutionRequest{source,name:"native-bounded-view.js",kind:crate::SourceKind::Script}).unwrap(),Engine::specialize(source,"native-bounded-view.js").unwrap()]{Runtime::new(bounded_view_host()).execute(&program).unwrap();}
}

fn non_index_key_host() -> TestHost {
    TestHost::default()
        .with_natives(crate::host_functions![global "nativeNonIndexKeys" (2) => native_non_index_keys])
}
fn native_non_index_keys(context: &mut NativeContext<'_, TestHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let enumerable = context.truthy_rooted(args[1])?;
    let keys = context.own_non_index_keys_rooted(args[0], enumerable)?;
    context.collect()?;
    context.array_rooted(&keys)
}
#[test]
fn native_non_index_keys_keep_array_length_order_descriptors_symbols_and_proxy_semantics() {
    let source = r#"
      const symbol=Symbol('extra');
      for(const array of [[1,2],new Array(4096)]) {
        array.extra=3;Object.defineProperty(array,'hidden',{value:4});array[symbol]=5;
        for(const value of [array,new Proxy(array,{})]) {
          const all=nativeNonIndexKeys(value,false),visible=nativeNonIndexKeys(value,true);
          if(all.length!==4||all[0]!=='length'||all[1]!=='extra'||all[2]!=='hidden'||all[3]!==symbol)throw Error('non-index own-key order');
          if(visible.length!==2||visible[0]!=='extra'||visible[1]!==symbol)throw Error('non-index enumerable filter');
        }
      }
      const object={length:7,'0':1,extra:2};
      if(nativeNonIndexKeys(object,false).join()!=='length,extra')throw Error('ordinary length key');
      const view=new Uint8Array(4096);view.extra=1;
      if(nativeNonIndexKeys(view,false).join()!=='extra')throw Error('view indices expanded');
    "#;
    for program in [Engine::compile(crate::ExecutionRequest{source,name:"native-non-index-keys.js",kind:crate::SourceKind::Script}).unwrap(),Engine::specialize(source,"native-non-index-keys.js").unwrap()] {
        Runtime::new(non_index_key_host()).execute(&program).unwrap();
    }
}

fn bootstrap_reg_exp_host() -> TestHost {
    TestHost::default()
        .with_natives(crate::host_functions![global "nativeBootstrapRegExp" (1) => native_bootstrap_regexp])
        .with_install(|context: &mut NativeContext<'_, TestHost>| {
            context.seal_startup_intrinsics(); Ok(())
        })
}
fn native_bootstrap_regexp(context: &mut NativeContext<'_, TestHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let missing = context.undefined();
    let fail = context.truthy_rooted(args.first().copied().unwrap_or(missing))?;
    context.with_startup_intrinsics(|context| {
        context.evaluate_script_rooted("/(host)(setup)/.exec('beforehostsetupafter');", "host-regexp.js")?;
        context.collect()?;
        context.with_startup_intrinsics(|context| {
            context.evaluate_script_rooted("/(nested)/.exec('nested');", "nested-host-regexp.js")
        })?;
        if fail {
            context.evaluate_script_rooted("throw globalThis.failure;", "failing-host-regexp.js")
        } else { Ok(context.undefined()) }
    })
}
#[test]
fn native_builtin_setup_preserves_regexp_statics_across_nested_gc_and_throw() {
    let source = r#"
      function state(){return [RegExp.input,RegExp.lastMatch,RegExp.lastParen,RegExp.leftContext,RegExp.rightContext,RegExp.$1,RegExp.$2,RegExp.$9]}
      /(left)-(right)/.exec('beforeleft-rightafter');
      RegExp.input='caller assigned input';
      const before=JSON.stringify(state());
      nativeBootstrapRegExp(false);
      if(JSON.stringify(state())!==before)throw Error('builtin setup changed caller match');
      globalThis.failure={marker:42};let caught;
      try{nativeBootstrapRegExp(true)}catch(error){caught=error}
      if(caught!==failure)throw Error('bootstrap throw lost identity');
      if(JSON.stringify(state())!==before)throw Error('failed builtin setup changed caller match');
      /(next)/.exec('next');if(RegExp.$1!=='next')throw Error('later guest match was suppressed');
    "#;
    for program in [Engine::compile(crate::ExecutionRequest{source,name:"native-bootstrap-regexp.js",kind:crate::SourceKind::Script}).unwrap(),Engine::specialize(source,"native-bootstrap-regexp.js").unwrap()] {
        Runtime::new(bootstrap_reg_exp_host()).execute(&program).unwrap();
    }
}

fn promise_slots_host() -> TestHost {
    TestHost::default()
        .with_natives(crate::host_functions![global "promiseSlots" (1) => promise_slots])
}
fn promise_slots(context: &mut NativeContext<'_, TestHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let snapshot = context.promise_snapshot_rooted(args[0])?;
    context.collect()?;
    let result = match snapshot {
        Some(PromiseSnapshot::Pending) => context.string_rooted("pending"),
        Some(PromiseSnapshot::Fulfilled(value) | PromiseSnapshot::Rejected(value)) => value,
        None => context.undefined(),
    };
    if snapshot.is_some() { context.mark_promise_handled_rooted(args[0])?; }
    Ok(result)
}
#[test]
fn native_promise_slots_preserve_identity_without_observation_or_guest_hooks() {
    let source = r#"
      let ran=false; const value={answer:42};
      const pending=new Promise(()=>{}), fulfilled=Promise.resolve(value), rejected=Promise.reject(value);
      for(const p of [pending,fulfilled,rejected]) {
        Object.defineProperty(p,'then',{get(){throw Error('then getter')}});
        Object.defineProperty(p,'constructor',{get(){throw Error('constructor getter')}});
      }
      Promise.resolve().then(()=>{ran=true});
      if(promiseSlots(pending)!=='pending')throw Error('pending');
      if(promiseSlots(fulfilled)!==value || promiseSlots(rejected)!==value)throw Error('result identity');
      if(promiseSlots({})!==undefined)throw Error('brand');
      if(ran)throw Error('inspection drained jobs');
    "#;
    for program in [Engine::compile(crate::ExecutionRequest{source,name:"promise-slots.js",kind:crate::SourceKind::Script}).unwrap(),Engine::specialize(source,"promise-slots.js").unwrap()] {
        let mut runtime=Runtime::new(promise_slots_host());
        runtime.execute(&program).unwrap();
        assert!(runtime.take_promise_rejection_unhandled_events().is_empty());
    }
}

fn function_metadata_host() -> TestHost {
    TestHost::default()
        .with_natives(crate::host_functions![global "metadataCallback" (2) => empty_metadata_callback])
        .with_install(|context: &mut NativeContext<'_, TestHost>| {
            let global = context.global_root()?;
            let data = context.object_rooted()?;
            for (name, constructor) in [("MetadataCaptured", false), ("MetadataCtor", true)] {
                let function = if constructor {
                    context.host_constructor_with_data(HostFunctionId(0), data)?
                } else {
                    context.host_function_with_data(HostFunctionId(0), data)?
                };
                let key = context.string_rooted(name);
                context.define_data_property_rooted(global, key, function, true, true, true)?;
            }
            let named_constructor = context.host_constructor_named(HostFunctionId(0), "")?;
            let key = context.string_rooted("MetadataAnonymousCtor");
            context.define_data_property_rooted(global, key, named_constructor, true, true, true)?;
            let captured_constructor = context.host_constructor_named_with_data(HostFunctionId(0), "", data)?;
            let key = context.string_rooted("MetadataAnonymousCapturedCtor");
            context.define_data_property_rooted(global, key, captured_constructor, true, true, true)?;
            let anonymous = context.host_function_named(HostFunctionId(0), "")?;
            let key = context.string_rooted("MetadataAnonymous");
            context.define_data_property_rooted(global, key, anonymous, true, true, true)?;
            Ok(())
        })
}
fn empty_metadata_callback(
    context: &mut NativeContext<'_, TestHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    Ok(context.undefined())
}
#[test]
fn native_function_metadata_preserves_standard_key_order_and_constructor_views() {
    let source = r#"
for(const [functionValue,constructor] of [[metadataCallback,false],[MetadataCaptured,false],[MetadataCtor,true],[MetadataAnonymous,false],[MetadataAnonymousCtor,true],[MetadataAnonymousCapturedCtor,true]]) {
 const keys=Reflect.ownKeys(functionValue).join(',');
 if(keys!==(constructor?'length,name,prototype':'length,name'))throw Error(keys);
 if(functionValue.name!==(functionValue===MetadataAnonymous||functionValue===MetadataAnonymousCtor||functionValue===MetadataAnonymousCapturedCtor?'':'metadataCallback')||functionValue.length!==2)throw Error('metadata values');
 for(const name of ['length','name']){
  const d=Object.getOwnPropertyDescriptor(functionValue,name);
  if(d.writable||d.enumerable||!d.configurable)throw Error('metadata attributes');
 }
 if(constructor){if(!(new functionValue() instanceof functionValue))throw Error('constructor view');}
 else{let threw=false;try{new functionValue()}catch(error){threw=error instanceof TypeError}if(!threw)throw Error('callback is constructible');}
}
"#;
    for program in [
        Engine::compile(crate::ExecutionRequest {
            source,
            name: "host-function-metadata.js",
            kind: crate::SourceKind::Script,
        })
        .unwrap(),
        Engine::specialize(source, "host-function-metadata.js").unwrap(),
    ] {
        Runtime::new(function_metadata_host())
            .execute(&program)
            .unwrap();
    }
}

fn proxy_snapshot_host() -> TestHost {
    TestHost::default()
        .with_natives(crate::host_functions![global "snapshotProxy" (2) => snapshot_proxy])
}
fn snapshot_proxy(
    context: &mut NativeContext<'_, TestHost>, _: RootId, args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let proxy = args.first().copied().unwrap_or(undefined);
    let Some((target, handler)) = context.proxy_details_rooted(proxy)? else { return Ok(undefined); };
    if let Some(callback) = args.get(1).copied() {
        context.call_rooted(callback, undefined, &[])?;
    }
    context.collect()?;
    context.array_rooted(&[target, handler])
}
#[test]
fn native_proxy_snapshots_survive_revocation_reentry_and_collection_without_traps() {
    let source = r#"
let target={tag:'target'},handler={tag:'handler',get(){throw Error('proxy trap')}};
const pair=Proxy.revocable(target,handler);
const held=snapshotProxy(pair.proxy,()=>{target=null;handler=null;pair.revoke()});
if(held[0].tag!=='target'||held[1].tag!=='handler')throw Error('snapshot lost roots');
const revoked=snapshotProxy(pair.proxy);
if(revoked[0]!==null||revoked[1]!==null)throw Error('revoked snapshot');
if(snapshotProxy({})!==undefined)throw Error('ordinary object snapshot');
"#;
    for program in [
        Engine::specialize_unspecialized(source, "proxy-snapshot.js").unwrap(),
        Engine::specialize(source, "proxy-snapshot.js").unwrap(),
    ] {
        Runtime::new(proxy_snapshot_host()).execute(&program).unwrap();
    }
}

fn collection_preview_host() -> TestHost {
    TestHost::default()
        .with_natives(crate::host_functions![global "previewCollection" (2) => preview_collection])
}
fn preview_collection(
    context: &mut NativeContext<'_, TestHost>, _: RootId, args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let source = args.first().copied().unwrap_or(undefined);
    let Some(preview) = context.collection_preview_rooted(source)? else { return Ok(undefined); };
    if let Some(callback) = args.get(1).copied() { context.call_rooted(callback, undefined, &[])?; }
    context.collect()?;
    let entries = context.array_rooted(&preview.entries)?;
    let paired = context.boolean(preview.is_key_value);
    context.array_rooted(&[entries, paired])
}
#[test]
fn native_collection_previews_keep_cursors_and_root_entries_across_clear_and_collection() {
    let source = r#"
const map=new Map([[{key:1},{value:2}]]),iterator=map.entries();
const snapshot=previewCollection(iterator,()=>map.clear());
if(!snapshot[1]||snapshot[0][0].key!==1||snapshot[0][1].value!==2)throw Error('preview roots');
if(!iterator.next().done)throw Error('clear visibility');
map.set('a',1);map.set('b',2);const keys=map.keys();keys.next();
const view=previewCollection(keys);if(view[1]||view[0].join()!=='b'||keys.next().value!=='b')throw Error('preview consumed iterator');
if(previewCollection({})!==undefined)throw Error('ordinary object preview');
"#;
    for program in [
        Engine::specialize_unspecialized(source, "collection-preview.js").unwrap(),
        Engine::specialize(source, "collection-preview.js").unwrap(),
    ] {
        Runtime::new(collection_preview_host()).execute(&program).unwrap();
    }
}

/// The weak root a test host's `remember*` native keeps.
type Remembered = std::cell::Cell<Option<HostWeakRoot>>;
fn constructor_provenance_host() -> TestHost {
    TestHost::default()
        .with_state(Rc::new(Remembered::default()))
        .with_natives(crate::host_functions![
            global "rememberConstructor" (1) => remember_constructor,
            global "constructedName" (1) => constructed_name,
        ])
}
fn remember_constructor(
    context: &mut NativeContext<'_, TestHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let weak = context.host_weak_root(args[0])?;
    context.host_mut().state::<Remembered>().set(Some(weak));
    Ok(context.undefined())
}
fn constructed_name(
    context: &mut NativeContext<'_, TestHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    Ok(context
        .constructed_name_rooted(args[0])?
        .unwrap_or_else(|| context.undefined()))
}
#[test]
fn constructor_provenance_roots_only_live_nonprototype_instances_across_shape_collection() {
    let source = r#"
var instance,child;
function createInstance(){function Original(){};rememberConstructor(Original);instance=new Original;Object.setPrototypeOf(instance,null);Object.defineProperty(Original,'name',{value:'changed'})}
function checkInstance(){if(constructedName(instance)!=='changed')throw Error('lost debug name')}
function dropInstance(){instance=null}
function createReflected(){function Base(){};function Target(){};rememberConstructor(Base);instance=Reflect.construct(Base,[],Target);Object.setPrototypeOf(instance,null)}
function checkReflected(){if(constructedName(instance)!==undefined)throw Error('derived base name visible')}
function createPrototype(){function PrototypeOwner(){};rememberConstructor(PrototypeOwner);const prototype=new PrototypeOwner;Object.setPrototypeOf(prototype,null);child=Object.create(prototype)}
function checkPrototype(){if(constructedName(Object.getPrototypeOf(child))!==undefined)throw Error('prototype provenance visible')}
"#;
    for program in [
        Engine::specialize_unspecialized(source, "constructor-provenance.js").unwrap(),
        Engine::specialize(source, "constructor-provenance.js").unwrap(),
    ] {
        let mut runtime = Runtime::new(constructor_provenance_host());
        runtime.execute(&program).unwrap();
        // Check release after each activation returns, avoiding conservative
        // roots held by an active guest frame's locals or registers.
        for (operation, alive) in [
            ("createInstance", true),
            ("checkInstance", true),
            ("dropInstance", false),
            ("createReflected", true),
            ("checkReflected", true),
            ("dropInstance", false),
            ("createPrototype", false),
            ("checkPrototype", false),
        ] {
            let global = runtime.global_root().unwrap();
            let key = runtime.string_rooted(operation);
            let function = runtime.get_property_rooted(global, key).unwrap();
            let result = runtime.call_rooted(function, global, &[]).unwrap();
            for root in [result, function, key, global] {
                assert!(runtime.release_root(root));
            }
            runtime.collect(&program).unwrap();
            let weak = runtime
                .host_mut()
                .state::<Remembered>()
                .get()
                .expect("remembered constructor");
            assert_eq!(runtime.host_weak_root_is_live(weak), alive, "{operation}");
        }
    }
}

#[test]
fn captured_errors_and_exposed_callsites_trace_callable_identity_until_release() {
    let source = r#"
var error,sites;
Error.prepareStackTrace=(_,frames)=>frames;
function createStack(){function Frame(){return new Error};rememberConstructor(Frame);error=Frame()}
function exposeSites(){sites=error.stack;if(sites[0].getFunctionName()!=='Frame')throw Error('lost frame name')}
function dropError(){error=null}
function dropSites(){sites=null}
"#;
    for program in [
        Engine::specialize_unspecialized(source,"stack-name-roots.js").unwrap(),
        Engine::specialize(source,"stack-name-roots.js").unwrap(),
    ] {
        let mut runtime = Runtime::new(constructor_provenance_host());
        runtime.execute(&program).unwrap();
        for (operation, alive) in [
            ("createStack", true), ("exposeSites", true),
            ("dropError", true), ("dropSites", false),
        ] {
            let global=runtime.global_root().unwrap();
            let key=runtime.string_rooted(operation);
            let function=runtime.get_property_rooted(global,key).unwrap();
            let result=runtime.call_rooted(function,global,&[]).unwrap();
            for root in [result,function,key,global]{assert!(runtime.release_root(root));}
            runtime.collect(&program).unwrap();
            let weak=runtime.host_mut().state::<Remembered>().get().expect("remembered frame");
            assert_eq!(runtime.host_weak_root_is_live(weak),alive,"{operation}");
        }
    }
}

fn protected_startup_host() -> TestHost {
    TestHost::default()
        .with_natives(crate::host_functions![global "protectedStartup" (1) => protected_startup])
        .with_install(|context: &mut NativeContext<'_, TestHost>| {
            context.seal_startup_intrinsics();
            Ok(())
        })
}
fn protected_startup(
    context: &mut NativeContext<'_, TestHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let fail = args.first().is_some_and(|arg| context.rooted_value(*arg).and_then(Value::as_bool) == Some(true));
    context.with_startup_intrinsics(|context| {
        let result = context.evaluate_script_rooted(
            "JSON.stringify([[1,2,3].slice(1), [2,3].map(x=>x*2), 'abc'.slice(1), Object.entries({x:1})])",
            "protected-startup-originals.js",
        )?;
        context.collect()?;
        if fail {
            context.evaluate_script_rooted("throw globalThis.protectedMarker", "protected-startup-throw.js")
        } else {
            Ok(result)
        }
    })
}
#[test]
fn startup_intrinsics_restore_frozen_replacements_and_deleted_nonextensible_properties() {
    let source = r#"
const descriptor=Object.getOwnPropertyDescriptor;
const bad=function bad(){throw Error('guest replacement');};
const getter=function getter(){throw Error('guest getter');};
globalThis.protectedMarker={};
Object.defineProperty(Array.prototype,'slice',{value:bad,writable:false,configurable:false});
delete Array.prototype.map;
Object.preventExtensions(Array.prototype);
Object.defineProperty(String.prototype,'slice',{get:getter,configurable:false});
Object.defineProperty(Object,'entries',{value:bad,writable:false,configurable:false});
function check(){
 const a=descriptor(Array.prototype,'slice'),s=descriptor(String.prototype,'slice'),o=descriptor(Object,'entries');
 if(a.value!==bad||a.writable||a.configurable||s.get!==getter||s.configurable||o.value!==bad||o.writable||o.configurable)throw Error('guest descriptor changed');
 if(Object.isExtensible(Array.prototype)||descriptor(Array.prototype,'map')!==undefined)throw Error('guest integrity changed');
}
check();
if(protectedStartup(false)!=='[[2,3],[4,6],"bc",[["x",1]]]')throw Error('startup values changed');
check();
try{protectedStartup(true);throw Error('missing throw');}catch(error){if(error!==protectedMarker)throw error;}
check();
if(protectedStartup(false)!=='[[2,3],[4,6],"bc",[["x",1]]]')throw Error('repeated startup changed');
check();
"#;
    for program in [
        Engine::compile(crate::ExecutionRequest { source, name: "protected-startup.js", kind: crate::SourceKind::Script }).unwrap(),
        Engine::specialize(source, "protected-startup.js").unwrap(),
    ] {
        Runtime::new(protected_startup_host()).execute(&program).unwrap();
    }
}

#[test]
fn function_factories_preserve_original_intrinsic_constructor_and_prototype_identity() {
    let source = r#"
const factories=[()=>function(){},()=>()=>{},()=>async function(){},()=>function*(){},()=>async function*(){}];
for(const factory of factories){
 const first=factory(),prototype=Object.getPrototypeOf(first),C=prototype.constructor;
 const name=Object.getOwnPropertyDescriptor(C,'name'),descriptor=Object.getOwnPropertyDescriptor(prototype,'constructor');
 Object.defineProperty(C,'name',{value:'Changed'+name.value});
 Object.defineProperty(prototype,'constructor',{value:null});
 const second=factory();
 if(Object.getPrototypeOf(second)!==prototype)throw Error('factory prototype follows public constructor');
 for(const value of [first,second]){Object.setPrototypeOf(value,null);if(constructedName(value)!=='Changed'+name.value)throw Error('factory lost original constructor')}
 Object.defineProperty(prototype,'constructor',descriptor);Object.defineProperty(C,'name',name);
}
const asyncPrototype=Object.getPrototypeOf(async function(){}),generatorPrototype=Object.getPrototypeOf(function*(){}),asyncGeneratorPrototype=Object.getPrototypeOf(async function*(){});
globalThis.AsyncFunction={prototype:{fake:true}};globalThis.GeneratorFunction={prototype:{fake:true}};globalThis.AsyncGeneratorFunction={prototype:{fake:true}};
if(Object.getPrototypeOf(async function(){})!==asyncPrototype||Object.getPrototypeOf(function*(){})!==generatorPrototype||Object.getPrototypeOf(async function*(){})!==asyncGeneratorPrototype)throw Error('factory follows mutable globals');
"#;
    for program in [
        Engine::specialize_unspecialized(source, "function-factory-origins.js").unwrap(),
        Engine::specialize(source, "function-factory-origins.js").unwrap(),
    ] {
        Runtime::new(constructor_provenance_host())
            .execute(&program)
            .unwrap();
    }
}

#[test]
fn completed_generator_creation_origin_retains_then_releases_its_function_identity() {
    let source = r#"
var state;
function createIterator(){function* Creator(){};rememberConstructor(Creator);const value=Creator();state={value,next:value.next};Object.setPrototypeOf(value,null);Object.defineProperty(Creator,'name',{value:'Changed'})}
function closeIterator(){state.next.call(state.value);if(constructedName(state.value)!=='Changed')throw Error('lost completed generator origin')}
function dropIterator(){state=null}
"#;
    for program in [
        Engine::specialize_unspecialized(source, "generator-origin-roots.js").unwrap(),
        Engine::specialize(source, "generator-origin-roots.js").unwrap(),
    ] {
        let mut runtime = Runtime::new(constructor_provenance_host());
        runtime.execute(&program).unwrap();
        for (operation, alive) in [
            ("createIterator", true),
            ("closeIterator", true),
            ("dropIterator", false),
        ] {
            let global = runtime.global_root().unwrap();
            let key = runtime.string_rooted(operation);
            let function = runtime.get_property_rooted(global, key).unwrap();
            let result = runtime.call_rooted(function, global, &[]).unwrap();
            for root in [result, function, key, global] {
                assert!(runtime.release_root(root));
            }
            runtime.collect(&program).unwrap();
            let weak = runtime.host_mut().state::<Remembered>().get().unwrap();
            assert_eq!(runtime.host_weak_root_is_live(weak), alive, "{operation}");
        }
    }
}

fn boxed_primitive_host() -> TestHost {
    TestHost::default()
        .with_state(Rc::new(Remembered::default()))
        .with_natives(crate::host_functions![global "rememberPayload" (1) => remember_boxed_payload])
}
fn remember_boxed_payload(
    context: &mut NativeContext<'_, TestHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let weak = context.host_weak_root(args[0])?;
    context.host_mut().state::<Remembered>().set(Some(weak));
    Ok(context.undefined())
}
#[test]
fn boxed_primitive_internal_values_survive_collection_and_release_with_the_wrapper() {
    let source = r#"
var wrapper;
function createWrapper(){const value=Symbol('payload');rememberPayload(value);wrapper=Object(value);Object.setPrototypeOf(wrapper,null)}
function readWrapper(){const value=Symbol.prototype.valueOf.call(wrapper);if(typeof value!=='symbol'||value.description!=='payload')throw Error('lost wrapper payload')}
function dropWrapper(){wrapper=null}
"#;
    for program in [
        Engine::specialize_unspecialized(source, "boxed-primitive-roots.js").unwrap(),
        Engine::specialize(source, "boxed-primitive-roots.js").unwrap(),
    ] {
        let mut runtime = Runtime::new(boxed_primitive_host());
        runtime.execute(&program).unwrap();
        for (operation, alive) in [("createWrapper", true), ("readWrapper", true), ("dropWrapper", false)] {
            let global = runtime.global_root().unwrap();
            let key = runtime.string_rooted(operation);
            let function = runtime.get_property_rooted(global, key).unwrap();
            let result = runtime.call_rooted(function, global, &[]).unwrap();
            for root in [result, function, key, global] { assert!(runtime.release_root(root)); }
            runtime.collect(&program).unwrap();
            let payload = runtime.host_mut().state::<Remembered>().get().unwrap();
            assert_eq!(runtime.host_weak_root_is_live(payload), alive, "{operation}");
        }
    }
}

#[derive(Default)]
struct PreparedScriptState {
    script: Option<PreparedScript>,
    invalidations: usize,
}
fn prepared_script_host(state: Rc<RefCell<PreparedScriptState>>) -> TestHost {
    TestHost::default()
        .with_state(state)
        .with_natives(&[])
        .with_install(|context: &mut NativeContext<'_, TestHost>| {
            let state = context.host_mut().state::<RefCell<PreparedScriptState>>();
            let first = context.create_realm_rooted()?;
            let second = context.create_realm_rooted()?;
            let old = state.borrow_mut().script.take();
            if let Some(old) = old {
                assert!(!context.prepared_script_is_current(&old));
                assert!(context.evaluate_prepared_script_in_realm_rooted(first, &old).is_err());
                state.borrow_mut().invalidations += 1;
            }
            let prepared = context.prepare_specialized_script_in_realm_with_origin(
                first,
                "(() => { const secret = {}; return function(value) { return { value, secret, array: [], error: new Error('prepared') }; }; })()",
                "prepared-host.js", 0, 0,
            )?;
            assert!(context.prepared_script_is_current(&prepared));
            context.collect()?;
            let left_factory = context.evaluate_prepared_script_in_realm_rooted(first, &prepared)?;
            let right_factory = context.evaluate_prepared_script_in_realm_rooted(second, &prepared)?;
            context.collect()?;
            let undefined = context.undefined();
            let one = context.number(1.0);
            let two = context.number(2.0);
            let left = context.call_rooted(left_factory, undefined, &[one])?;
            let again = context.call_rooted(left_factory, undefined, &[two])?;
            let right = context.call_rooted(right_factory, undefined, &[two])?;
            context.collect()?;
            let property = |context: &mut NativeContext<'_, TestHost>, object, name| {
                let key = context.string_rooted(name);
                context.get_property_rooted(object, key)
            };
            let left_secret = property(context, left, "secret")?;
            let same_secret = property(context, again, "secret")?;
            let right_secret = property(context, right, "secret")?;
            assert_eq!(context.rooted_value(left_secret), context.rooted_value(same_secret));
            assert_ne!(context.rooted_value(left_secret), context.rooted_value(right_secret));
            for name in ["array", "error"] {
                let left = property(context, left, name)?;
                let right = property(context, right, name)?;
                let left = context.prototype_rooted(left)?;
                let right = context.prototype_rooted(right)?;
                assert_ne!(context.rooted_value(left), context.rooted_value(right));
            }
            for object in [left, right] {
                let error = property(context, object, "error")?;
                let stack = property(context, error, "stack")?;
                assert!(context.to_string(stack)?.contains("prepared-host.js"));
            }
            state.borrow_mut().script = Some(prepared);
            Ok(())
        })
}
#[test]
fn prepared_host_scripts_share_code_but_own_realm_state_and_reject_runtime_reset_handles() {
    for program in [
        Engine::specialize_unspecialized("void 0", "prepared-entry.js").unwrap(),
        Engine::specialize("void 0", "prepared-entry.js").unwrap(),
    ] {
        let state = Rc::new(RefCell::new(PreparedScriptState::default()));
        let mut runtime = Runtime::new(prepared_script_host(state.clone()));
        runtime.execute(&program).unwrap();
        runtime.execute(&program).unwrap();
        let mut other = Runtime::new(prepared_script_host(state.clone()));
        other.execute(&program).unwrap();
        assert_eq!(state.borrow().invalidations, 2);
    }
}

fn error_source_host() -> TestHost {
    TestHost::default()
        .with_natives(&[])
        .with_install(|context: &mut NativeContext<'_, TestHost>| {
            let first_source = "new Error('first')";
            let second_source = "\nnew Error('second')";
            let global = context.global_root()?;
            let first = context.evaluate_script_in_realm_rooted(global, first_source, "same-source-name.js")?;
            let second = context.evaluate_script_in_realm_rooted(global, second_source, "same-source-name.js")?;
            // The metadata owns immutable source text, not a lookup by a reusable filename
            // or a borrowed Script slot. Collection and later compilation must preserve it.
            context.collect()?;
            for (error, source, line) in [(first, first_source, 1), (second, second_source.trim_start(), 2)] {
                let position = context.error_source_position_rooted(error)?.expect("captured source");
                assert_eq!(position.source_line.as_deref(), Some(source));
                assert_eq!(position.script_name.as_deref(), Some("same-source-name.js"));
                assert_eq!(position.line_number, line);
                assert_eq!(position.column_number, 1);
            }
            let eval = context.evaluate_script_rooted("new Error('eval')", "named-eval.js")?;
            context.collect()?;
            let position = context.error_source_position_rooted(eval)?.expect("captured eval source");
            assert_eq!(position.source_line.as_deref(), Some("new Error('eval')"));
            assert_eq!(position.script_name, None);
            assert_eq!((position.line_number, position.column_number), (1, 1));
            Ok(())
        })
}

#[test]
fn error_source_metadata_owns_original_text_across_same_named_scripts_and_collection() {
    for program in [
        Engine::specialize_unspecialized("void 0", "error-source-entry.js").unwrap(),
        Engine::specialize("void 0", "error-source-entry.js").unwrap(),
    ] {
        let mut runtime = Runtime::new(error_source_host());
        runtime.execute(&program).unwrap();
    }
}

#[derive(Default)]
struct CompiledScriptStateForTest {
    script: Option<CompiledScript>,
    invalidations: usize,
}
fn compiled_script_host(state: Rc<RefCell<CompiledScriptStateForTest>>) -> TestHost {
    TestHost::default()
        .with_state(state)
        .with_natives(crate::host_functions![method "scriptIdentity" (0) => compiled_script_identity])
        .with_install(|context: &mut NativeContext<'_, TestHost>| {
            let state = context.host_mut().state::<RefCell<CompiledScriptStateForTest>>();
            let previous = state.borrow().script.clone();
            if let Some(script) = previous {
                assert!(!context.compiled_script_is_current(&script));
                let global = context.global_root()?;
                assert!(context.evaluate_compiled_script_in_realm_rooted(global, &script).is_err());
                state.borrow_mut().invalidations += 1;
                return Ok(());
            }
            let first = context.create_realm_rooted()?;
            let second = context.create_realm_rooted()?;
            for global in [first, second] {
                let function = context.host_function(HostFunctionId(0))?;
                let key = context.string_rooted("scriptIdentity");
                context.define_data_property_rooted(global, key, function, true, true, true)?;
            }
            let script = context.compile_script_in_realm_with_origin(first,
                "({identity:scriptIdentity(), array:[], error:new Error('owned'), template:((x)=>x)`site`, Class:class { #value=1; read(){return this.#value} }})",
                "owned-host-script.js", 0, 0)?;
            context.collect()?;
            context.collect()?;
            let left = context.evaluate_compiled_script_in_realm_rooted(first, &script)?;
            let again = context.evaluate_compiled_script_in_realm_rooted(first, &script)?;
            let right = context.evaluate_compiled_script_in_realm_rooted(second, &script)?;
            context.collect()?;
            let get = |context: &mut NativeContext<'_, TestHost>, object, name| {
                let key = context.string_rooted(name);
                context.get_property_rooted(object, key)
            };
            for name in ["identity", "template"] {
                let a = get(context, left, name)?;
                let b = get(context, again, name)?;
                assert!(context.strict_equal_rooted(a, b)?);
                let c = get(context, right, name)?;
                assert_eq!(context.strict_equal_rooted(a, c)?, name == "identity");
            }
            for name in ["array", "error"] {
                let a = get(context, left, name)?;
                let b = get(context, again, name)?;
                assert!(!context.strict_equal_rooted(a, b)?);
                let c = get(context, right, name)?;
                let a = context.prototype_rooted(a)?;
                let c = context.prototype_rooted(c)?;
                assert!(!context.strict_equal_rooted(a, c)?);
            }
            let first_class = get(context, left, "Class")?;
            let second_class = get(context, again, "Class")?;
            assert!(!context.strict_equal_rooted(first_class, second_class)?);
            let first_instance = context.construct_rooted(first_class, first_class, &[])?;
            let second_instance = context.construct_rooted(second_class, second_class, &[])?;
            let read = get(context, first_instance, "read")?;
            let one = context.call_rooted(read, first_instance, &[])?;
            assert_eq!(context.rooted_value(one), Some(Value::number(1.0)));
            assert!(context.call_rooted(read, second_instance, &[]).is_err());
            let after = context.evaluate_compiled_script_in_realm_rooted(first, &script)?;
            let old = get(context, left, "identity")?;
            let fresh = get(context, after, "identity")?;
            assert!(context.strict_equal_rooted(old, fresh)?);
            state.borrow_mut().script = Some(script);
            Ok(())
        })
}
fn compiled_script_identity(
    context: &mut NativeContext<'_, TestHost>, _: RootId, _: &[RootId],
) -> Result<RootId, RootedError> {
    let frame = context.source_stack_frames(1).pop().expect("actual Script activation");
    assert_eq!(frame.script_name, "owned-host-script.js");
    assert!(!frame.is_eval);
    Ok(context.number(frame.script_id as f64))
}
#[test]
fn compiled_host_scripts_keep_source_identity_and_own_separate_realm_execution_state() {
    for program in [
        Engine::specialize_unspecialized("void 0", "owned-entry.js").unwrap(),
        Engine::specialize("void 0", "owned-entry.js").unwrap(),
    ] {
        let state = Rc::new(RefCell::new(CompiledScriptStateForTest::default()));
        let mut runtime = Runtime::new(compiled_script_host(state.clone()));
        runtime.execute(&program).unwrap();
        runtime.execute(&program).unwrap();
        let mut foreign = Runtime::new(compiled_script_host(state.clone()));
        foreign.execute(&program).unwrap();
        assert_eq!(state.borrow().invalidations, 2);
    }
}

fn host_private_symbol_host() -> TestHost {
    TestHost::default()
        .with_natives(&[])
        .with_install(|context: &mut NativeContext<'_, TestHost>| {
            let global = context.global_root()?;
            for (name, private) in [("nativePrivate", true), ("nativePublic", false)] {
                let symbol = if private { context.private_symbol_rooted(Some("same")) }
                             else { context.symbol_rooted(Some("same")) };
                let name = context.string_rooted(name);
                context.define_data_property_rooted(global, name, symbol, true, true, true)?;
            }
            context.collect()?;
            Ok(())
        })
}
#[test]
fn host_private_symbols_preserve_explicit_access_and_bypass_public_object_protocols() {
    let source = r#"
      function check(value) { if (!value) throw new Error('private-symbol check'); }
      const s = nativePrivate, publicKey = nativePublic;
      check(typeof s === 'symbol' && s !== publicKey && s.description === 'same');
      check(String(s) === 'Symbol(same)' && Symbol.keyFor(s) === undefined);
      const o = {}; o[s] = 2; o[publicKey] = 8;
      check(o[s] === 2 && s in o && Object.hasOwn(o, s));
      let d = Object.getOwnPropertyDescriptor(o,s);
      check(d.value === 2 && d.writable && !d.enumerable && d.configurable);
      check(Reflect.ownKeys(o).length === 1 && Reflect.ownKeys(o)[0] === publicKey);
      check(Object.getOwnPropertySymbols(o).length === 1);
      check(!Object.hasOwn(Object.getOwnPropertyDescriptors(o),s));
      check(Object.assign({},o)[s] === undefined && ({...o})[s] === undefined);
      const child = Object.create(o);
      check(child[s] === undefined && !(s in child) && !Object.hasOwn(child,s));
      check(child[publicKey] === 8);
      child[s] = 3; check(child[s] === 3 && o[s] === 2);
      Object.defineProperty(o,s,{value:4,enumerable:true,writable:false,configurable:false});
      check(Object.getOwnPropertyDescriptor(o,s).enumerable);
      check(Reflect.ownKeys(o).length === 1 && !Reflect.deleteProperty(o,s));
      check(!Reflect.set(o,s,5) && o[s] === 4);
      let called = 0;
      const p = new Proxy(o,{
        get(){called++;throw 1}, set(){called++;throw 1}, has(){called++;throw 1},
        getOwnPropertyDescriptor(){called++;throw 1}, defineProperty(){called++;throw 1},
        deleteProperty(){called++;throw 1}, ownKeys(){return [s]}
      });
      check(p[s] === undefined && !(s in p) && !Object.hasOwn(p,s));
      check(!Reflect.set(p,s,6) && !Reflect.defineProperty(p,s,{value:7}));
      check(Reflect.deleteProperty(p,s) && called === 0);
      check(Reflect.ownKeys(p).length === 0);
      const rev = Proxy.revocable({},{});rev.revoke();
      check(rev.proxy[s] === undefined && !(s in rev.proxy));
      check(Reflect.deleteProperty(rev.proxy,s) && !Reflect.set(rev.proxy,s,8));
      const frozen = Object.freeze({});check(Reflect.set(frozen,s,9));
      check(frozen[s] === 9 && Reflect.defineProperty(frozen,s,{value:10}));
      check(Object.isFrozen(frozen) && Reflect.ownKeys(frozen).length === 0);
      let receiver;
      const access = {};
      Object.defineProperty(access,s,{get(){receiver=this;return 12},configurable:true});
      check(access[s] === 12 && receiver === access);
      check(!Reflect.set(access,s,4));
      check(Reflect.deleteProperty(access,s) && access[s] === undefined);
      for (const base of [null, undefined]) {
        let threw = false; try { base[s]; } catch(e) { threw = e instanceof TypeError; }
        check(threw);
      }
      for (const base of [1, true, 'x', Symbol('x')]) {
        check(base[s] === undefined);
        let threw = false;
        try { (function(){ 'use strict'; base[s] = 1; })(); }
        catch(e) { threw = e instanceof TypeError; }
        check(threw);
      }
      let setterReceiver, setterValue;
      const a = {}, receiverObject = {};
      Object.defineProperty(a,s,{get(){return this},set(v){setterReceiver=this;setterValue=v},configurable:true});
      check(Reflect.get(a,s,receiverObject) === receiverObject);
      check(Reflect.set(a,s,33,receiverObject));
      check(setterReceiver === receiverObject && setterValue === 33);
      const marker = {};
      Object.defineProperty(a,s,{get(){throw marker},set(){throw marker}});
      for(const operation of [()=>a[s],()=>Reflect.set(a,s,2)]) {
        let caught; try { operation(); } catch(e) { caught=e; } check(caught===marker);
      }
      for(const target of [{},Object.preventExtensions({})]) {
        let threw=false;try{Reflect.ownKeys(new Proxy(target,{ownKeys(){return [s,s]}}))}catch(e){threw=e instanceof TypeError}
        check(threw);
      }
      let invariantThrew=false;
      try{Reflect.ownKeys(new Proxy(Object.preventExtensions({}),{ownKeys(){return [s]}}))}catch(e){invariantThrew=e instanceof TypeError}
      check(invariantThrew);
      globalThis.privateSurvivor = child;
    "#;
    for program in [Engine::specialize_unspecialized(source, "native-private-symbol.js").unwrap(),
                    Engine::specialize(source, "native-private-symbol.js").unwrap()] {
        let mut runtime = Runtime::new(host_private_symbol_host());
        runtime.execute(&program).unwrap();
    }
}


fn identity_host() -> TestHost {
    TestHost::default()
        .with_natives(crate::host_functions![method "identity" (1) => identity])
        .with_install(|context: &mut NativeContext<'_, TestHost>| {
            let operation = context.host_function(HostFunctionId(0))?;
            let global = context.global_root()?;
            let key = context.string_rooted("identity");
            assert!(context.set_property_rooted(global, key, operation, global)?);
            Ok(())
        })
}

fn identity(
    context: &mut NativeContext<'_, TestHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    Ok(args.first().copied().unwrap_or_else(|| context.undefined()))
}

#[test]
fn native_calls_reuse_root_lists_instead_of_accumulating_them() {
    let mut runtime = Runtime::new(identity_host());
    let program = Engine::specialize(
        "for (let i = 0; i < 1000; i++) identity(i, [i]);",
        "native-root-lists.js",
    )
    .unwrap();
    runtime.execute(&program).unwrap();
    let pooled = runtime.vm.native_root_lists.len();
    assert!(pooled <= 1, "{pooled} root lists pooled after sequential native calls");
}


fn lane_host() -> TestHost {
    TestHost::default()
        .with_natives(crate::host_functions![
            global "callBack" (2) => lane_call_back,
            global "raise" (1) => lane_raise,
            global "collected" (1) => lane_collected,
        ])
}

fn lane_call_back(
    ctx: &mut NativeContext<'_, TestHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    ctx.call_rooted(args[0], receiver, &args[1..])
}

fn lane_raise(
    ctx: &mut NativeContext<'_, TestHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    Err(ctx.throw(args[0]))
}

fn lane_collected(
    ctx: &mut NativeContext<'_, TestHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    ctx.collect()?;
    Ok(args[0])
}

#[test]
fn throws_through_lane_host_calls_of_another_program_reach_the_entry_programs_catch() {
    // A wrapper compiled as another program calls a host function from its lane run; the
    // guest callback's throw unwinds through the wrapper into a catch of the loop's entry
    // program, whose handlers the unwind reads from that program, not the wrapper's.
    let mut runtime = Runtime::new(lane_host());
    let program = Engine::specialize(
        "var wrap = new Function('f', 'let s = 0; for (let k = 0; k < 3; k++) s += k; return callBack(f, s);');
         function run(i) {
           let caught = 'none';
           try { wrap((v) => { throw { code: i + v }; }); } catch (e) { caught = e.code; }
           const passed = wrap((v) => v * 10);
           return caught + ':' + passed;
         }
         var out = [];
         for (let i = 0; i < 4; i++) out.push(run(i));
         var result = out.join(',');",
        "lane-host-cross-program.js",
    )
    .unwrap();
    runtime.execute(&program).unwrap();
    let global = runtime.global_root().unwrap();
    let key = runtime.string_rooted("result");
    let result = runtime.get_property_rooted(global, key).unwrap();
    assert_eq!(
        runtime.string_text_rooted(result).unwrap().as_deref(),
        Some("3:30,4:30,5:30,6:30")
    );
}

#[test]
fn host_functions_called_from_lane_frames_keep_reentry_errors_and_roots() {
    // Host functions run in place from a frame's lane run: a guest callback they call
    // sees its arguments, a thrown value reaches the guest's catch, and a collection
    // inside the host keeps the frame's live registers.
    let mut runtime = Runtime::new(lane_host());
    let program = Engine::specialize(
        "function run(i) {
           const o = { i };
           const back = callBack((v) => v.i * 2, o);
           let caught;
           try { raise({ code: i }); } catch (e) { caught = e.code; }
           const same = collected(o) === o;
           return [back, caught, same, o.i].join(':');
         }
         var out = [];
         for (let i = 0; i < 4; i++) out.push(run(i));
         var result = out.join(',');",
        "lane-host.js",
    )
    .unwrap();
    runtime.execute(&program).unwrap();
    let global = runtime.global_root().unwrap();
    let key = runtime.string_rooted("result");
    let result = runtime.get_property_rooted(global, key).unwrap();
    assert_eq!(
        runtime.string_text_rooted(result).unwrap().as_deref(),
        Some("0:0:true:0,2:1:true:1,4:2:true:2,6:3:true:3")
    );
}

fn descriptor_fields_host() -> TestHost {
    TestHost::default()
        .with_natives(crate::host_functions![
            global "nativeCopyProperty" (4) => native_copy_property,
            global "nativeDefineValue" (3) => native_define_value,
            global "nativeDefineMixed" (2) => native_define_mixed
        ])
}
/// `nativeCopyProperty(source, key, target, targetKey)`: whether [[GetOwnProperty]] found the
/// property, after defining it on `target` from the fields it reported.
fn native_copy_property(context: &mut NativeContext<'_, TestHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let [source, key, target, target_key] = args else { unreachable!("four arguments") };
    let Some(fields) = context.own_property_fields_rooted(*source, *key)? else {
        return Ok(context.boolean(false));
    };
    context.collect()?;
    let defined = context.define_property_fields_rooted(*target, *target_key, fields)?;
    Ok(context.boolean(defined))
}
/// `nativeDefineValue(target, key, value)`: `Reflect.defineProperty(target, key, { value })`.
fn native_define_value(context: &mut NativeContext<'_, TestHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let [target, key, value] = args else { unreachable!("three arguments") };
    let fields = DescriptorFields { value: Some(*value), ..DescriptorFields::default() };
    let defined = context.define_property_fields_rooted(*target, *key, fields)?;
    Ok(context.boolean(defined))
}
/// `nativeDefineMixed(target, key)`: a descriptor with both a value and a getter.
fn native_define_mixed(context: &mut NativeContext<'_, TestHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let [target, key] = args else { unreachable!("two arguments") };
    let undefined = context.undefined();
    let fields = DescriptorFields { value: Some(undefined), get: Some(undefined), ..DescriptorFields::default() };
    let defined = context.define_property_fields_rooted(*target, *key, fields)?;
    Ok(context.boolean(defined))
}
#[test]
fn native_descriptor_fields_match_get_own_property_and_define_property() {
    let source = r#"
      const same=(a,b)=>JSON.stringify(a)===JSON.stringify(b)&&(a===undefined||(a.get===b.get&&a.set===b.set&&Object.is(a.value,b.value)));
      const symbol=Symbol('s');
      const source={plain:1,[symbol]:2,get g(){return 3},set s(v){}};
      Object.defineProperty(source,'fixed',{value:4,writable:false,enumerable:false,configurable:false});
      const target={};
      for(const key of Reflect.ownKeys(source)){
        if(!nativeCopyProperty(source,key,target,key))throw Error('copy failed');
        if(!same(Object.getOwnPropertyDescriptor(source,key),Object.getOwnPropertyDescriptor(target,key)))throw Error('descriptor differs');
      }
      if(nativeCopyProperty(source,'missing',target,'missing')||'missing' in target)throw Error('missing property');
      const traps=[];
      const proxy=new Proxy({p:5},{getOwnPropertyDescriptor(t,k){traps.push(k);return Reflect.getOwnPropertyDescriptor(t,k)}});
      if(!nativeCopyProperty(proxy,'p',target,'fromProxy')||traps.join()!=='p'||target.fromProxy!==5)throw Error('proxy trap');
      if(!nativeDefineValue(target,'fresh',6))throw Error('fresh value');
      if(!same(Object.getOwnPropertyDescriptor(target,'fresh'),{value:6,writable:false,enumerable:false,configurable:false}))throw Error('absent fields default');
      Object.defineProperty(target,'kept',{value:7,writable:true,enumerable:true,configurable:true});
      nativeDefineValue(target,'kept',8);
      if(!same(Object.getOwnPropertyDescriptor(target,'kept'),{value:8,writable:true,enumerable:true,configurable:true}))throw Error('absent fields keep');
      if(nativeDefineValue(target,'fixed',9)||target.fixed!==4)throw Error('rejected redefinition');
      let caught;try{nativeDefineMixed(target,'mixed')}catch(error){caught=error}
      if(!(caught instanceof TypeError))throw Error('mixed fields');
    "#;
    for program in [Engine::compile(crate::ExecutionRequest{source,name:"native-descriptor-fields.js",kind:crate::SourceKind::Script}).unwrap(),Engine::specialize(source,"native-descriptor-fields.js").unwrap()] {
        Runtime::new(descriptor_fields_host()).execute(&program).unwrap();
    }
}

/// Keeps compiled programs in memory, shared by the runtimes of one test.
struct EntryCacheHost {
    kept: Rc<RefCell<std::collections::HashMap<(String, Vec<String>), Vec<u8>>>>,
    lines: Rc<RefCell<Vec<String>>>,
}
impl Host for EntryCacheHost {
    fn write_line(&mut self, line: &str) {
        self.lines.borrow_mut().push(line.to_owned());
    }
    fn clock_millis(&mut self) -> f64 { 0.0 }
    fn kept_compiled_program(&mut self, name: &str, parts: &[&str]) -> Option<crate::KeptBytes> {
        let key = (name.to_owned(), parts.iter().map(|part| (*part).to_owned()).collect());
        self.kept.borrow().get(&key).cloned().map(|bytes: Vec<u8>| Rc::new(bytes) as crate::KeptBytes)
    }
    fn keep_compiled_program(&mut self, name: &str, parts: &[&str], bytes: &[u8]) {
        let key = (name.to_owned(), parts.iter().map(|part| (*part).to_owned()).collect());
        self.kept.borrow_mut().insert(key, bytes.to_vec());
    }
}
#[test]
fn compiled_entry_is_kept_and_decodes_to_the_same_program() {
    let kept = Rc::new(RefCell::new(std::collections::HashMap::new()));
    let source = "function outer(n){const inner=k=>k*n;return [1,2,3].map(inner).join()}\nprint(`${outer(2)} ${typeof outer} ${outer.toString().length}`);";
    let mut outputs = Vec::new();
    for _ in 0..2 {
        let lines = Rc::new(RefCell::new(Vec::new()));
        let mut runtime = Runtime::new(EntryCacheHost { kept: kept.clone(), lines: lines.clone() });
        let program = runtime
            .compile_entry(crate::ExecutionRequest { source, name: "entry.js", kind: crate::SourceKind::Script })
            .unwrap();
        assert!(program.validated.holds(), "an entry compiled or decoded here is proven valid");
        runtime.execute(&program).unwrap();
        outputs.push(lines.borrow().clone());
    }
    assert_eq!(kept.borrow().len(), 1, "the second runtime decodes the kept entry");
    assert_eq!(outputs[0], outputs[1]);
    assert_eq!(outputs[0], ["2,4,6 function 70"]);
    // A different goal or text is a different entry.
    let lines = Rc::new(RefCell::new(Vec::new()));
    let mut runtime = Runtime::new(EntryCacheHost { kept: kept.clone(), lines });
    runtime.compile_entry(crate::ExecutionRequest { source, name: "entry.js", kind: crate::SourceKind::Module }).unwrap();
    assert_eq!(kept.borrow().len(), 2);
}
