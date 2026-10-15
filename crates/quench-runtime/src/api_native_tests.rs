use super::*;
use crate::{Engine, ResidualProgram};
use std::{cell::RefCell, rc::Rc};

#[derive(Default)]
struct CapturingHost;

impl Host for CapturingHost {
    fn write_line(&mut self, _: &str) {}
    fn clock_millis(&mut self) -> f64 {
        0.0
    }
    fn functions(&self) -> &[HostFunction<Self>] {
        crate::host_functions![method "captured" (0) => captured]
    }
    fn initialize(context: &mut NativeContext<'_, Self>) -> Result<(), RootedError> {
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
    }
}

fn captured(
    context: &mut NativeContext<'_, CapturingHost>,
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
    let mut runtime = Runtime::new(CapturingHost);
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

#[derive(Clone, Default)]
struct InitializingHost(Rc<RefCell<InitializerState>>);

impl Host for InitializingHost {
    fn write_line(&mut self, _: &str) {}
    fn clock_millis(&mut self) -> f64 {
        0.0
    }

    fn initialize(context: &mut NativeContext<'_, Self>) -> Result<(), RootedError> {
        let object = context.object_rooted()?;
        let global = context.global_root()?;
        let key = context.string_rooted("installed");
        let state = context.host_mut().0.clone();
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
    }
}

#[test]
fn host_initialization_scopes_objects_and_validates_exceptions_before_guest_execution() {
    let host = InitializingHost::default();
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
        for root in &host.0.borrow().scoped {
            assert!(!runtime.root_is_live(*root));
        }
    }
    host.0.borrow_mut().mode = Initialization::Throw;
    let error = runtime.execute(&program).unwrap_err();
    let exception = runtime.root(error.thrown_value().unwrap());
    runtime.collect(&program).unwrap();
    assert!(runtime.root_is_live(exception));
    for root in &host.0.borrow().scoped {
        assert!(!runtime.root_is_live(*root));
    }
    let global = runtime.global_root().unwrap();
    let key = runtime.string_rooted("reached");
    let result = runtime.get_property_rooted(global, key).unwrap();
    assert!(runtime.rooted_value(result).unwrap().is_undefined());
    let mut other = Runtime::new(TestHost::default());
    let foreign = other.root(Value::number(1.0));
    host.0.borrow_mut().mode = Initialization::Foreign(foreign);
    let error = runtime.execute(&program).unwrap_err();
    assert_eq!(error.to_string(), "released or foreign embedding root");
    assert!(error.thrown_value().is_none());
    for root in &host.0.borrow().scoped {
        assert!(!runtime.root_is_live(*root));
    }
    host.0.borrow_mut().mode = Initialization::Trap;
    let error = runtime.execute(&program).unwrap_err();
    assert_eq!(error.wasm_trap(), Some(crate::WasmTrap::Unreachable));
    host.0.borrow_mut().mode = Initialization::Unrooted(other.rooted_value(foreign).unwrap());
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

#[derive(Clone, Default)]
struct TestHost(Rc<RefCell<State>>);

impl Host for TestHost {
    fn write_line(&mut self, _: &str) {}
    fn clock_millis(&mut self) -> f64 {
        0.0
    }
    fn functions(&self) -> &[HostFunction<Self>] {
        crate::host_functions![
            global "echo" (1) => echo,
            method "reenter" (1) => reenter,
            method "reject" (1) => reject,
            method "escaped" (0) => escaped,
            method "trap" (0) => typed_trap,
            method "construct" (2) => construct,
            method "compare" (2) => compare,
            global "collectJSON" (0) => collect_json,
        ]
    }
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
    let state = ctx.host_mut().0.clone();
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
    ctx.host_mut().0.borrow_mut().borrowed.extend(root);
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
    let state = ctx.host_mut().0.clone();
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
    for root in &host.0.borrow().borrowed {
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
    let state = ctx.host_mut().0.clone();
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
    let host = TestHost::default();
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
    assert_eq!(host.0.borrow().calls, 1);
    for root in &host.0.borrow().borrowed {
        assert!(!runtime.root_is_live(*root));
    }
    let retained = host.0.borrow().retained[0];
    runtime.collect(&program).unwrap();
    assert_eq!(runtime.rooted_value(retained), runtime.rooted_value(result));
    assert!(runtime.release_root(retained));
    let callback = runtime.host_function(ECHO).unwrap();
    let receiver = runtime.root(Value::UNDEFINED);
    let result = runtime.call_rooted(callback, receiver, &[value]).unwrap();
    assert_eq!(runtime.rooted_value(result), runtime.rooted_value(value));
    for root in &host.0.borrow().borrowed {
        assert!(!runtime.root_is_live(*root));
    }
    let retained = host.0.borrow().retained[1];
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
    for root in &host.0.borrow().borrowed {
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
    for root in &host.0.borrow().borrowed {
        assert!(!runtime.root_is_live(*root));
    }
    assert!(runtime.release_root(exception));
}

#[test]
fn callbacks_cannot_return_foreign_or_released_handles() {
    let (mut runtime, _, host) = initialized("");
    let mut other = Runtime::new(TestHost::default());
    let foreign = other.root(Value::number(1.0));
    let stale = runtime.root(Value::number(2.0));
    assert!(runtime.release_root(stale));
    let receiver = runtime.root(Value::UNDEFINED);
    let callback = runtime.host_function(ESCAPED).unwrap();
    for exception in [false, true] {
        host.0.borrow_mut().invalid_exception = exception;
        for invalid in [foreign, stale] {
            host.0.borrow_mut().invalid_result = Some(invalid);
            let error = runtime.call_rooted(callback, receiver, &[]).unwrap_err();
            assert!(error.exception.is_none());
            assert_eq!(error.to_string(), "released or foreign embedding root");
            for root in &host.0.borrow().borrowed {
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
            .0
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
        for root in &host.0.borrow().borrowed {
            assert!(!runtime.root_is_live(*root));
        }
    }
}

#[test]
fn native_value_operations_reject_foreign_and_released_roots() {
    let (mut runtime, _, _) = initialized("");
    let mut other = Runtime::new(TestHost::default());
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

#[derive(Default)]
struct LazyHost;

const LAZY_GROUP_SCRIPT: &str = "globalThis.inits = (globalThis.inits || 0) + 1; \
    globalThis.A = 'a'; globalThis.B = 'b'; \
    Object.defineProperty(globalThis, 'C', { value: 'c', writable: true, configurable: true });";

impl Host for LazyHost {
    fn write_line(&mut self, _: &str) {}
    fn clock_millis(&mut self) -> f64 {
        0.0
    }
    fn functions(&self) -> &[HostFunction<Self>] {
        crate::host_functions![
            method "initializeGroup" (0) => initialize_lazy_group,
            method "initializeSingle" (0) => initialize_lazy_single,
        ]
    }
    fn initialize(context: &mut NativeContext<'_, Self>) -> Result<(), RootedError> {
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
        context.seal_startup_intrinsics();
        Ok(())
    }
}

fn initialize_lazy_group(
    context: &mut NativeContext<'_, LazyHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    context.evaluate_script_rooted(LAZY_GROUP_SCRIPT, "lazy-group.js")
}

fn initialize_lazy_single(
    context: &mut NativeContext<'_, LazyHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    context.evaluate_script_rooted("globalThis.D = 'd';", "lazy-single.js")
}

#[test]
fn lazy_data_properties_materialize_once_as_data_in_place() {
    let mut runtime = Runtime::new(LazyHost);
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

#[derive(Default)]
struct NumberCoercionHost;
impl Host for NumberCoercionHost {
    fn write_line(&mut self, _: &str) {}
    fn clock_millis(&mut self) -> f64 { 0.0 }
    fn functions(&self) -> &[HostFunction<Self>] {
        crate::host_functions![global "nativeNumber" (1) => native_number_coercion]
    }
}
fn native_number_coercion(context: &mut NativeContext<'_, NumberCoercionHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
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
    ] { Runtime::new(NumberCoercionHost).execute(&program).unwrap(); }
}

#[derive(Default)]
struct InstanceOfHost;
impl Host for InstanceOfHost {
    fn write_line(&mut self, _: &str) {}
    fn clock_millis(&mut self) -> f64 {
        0.0
    }
    fn functions(&self) -> &[HostFunction<Self>] {
        crate::host_functions![global "nativeInstanceOf" (2) => native_instanceof]
    }
}
fn native_instanceof(
    context: &mut NativeContext<'_, InstanceOfHost>,
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
        Runtime::new(InstanceOfHost).execute(&program).unwrap();
    }
}

#[derive(Default)]
struct CallerSourcesHost;
impl Host for CallerSourcesHost {
    fn write_line(&mut self, _: &str) {}
    fn clock_millis(&mut self) -> f64 { 0.0 }
    fn functions(&self) -> &[HostFunction<Self>] {
        crate::host_functions![global "nativeCallerSources" (0) => native_caller_sources, global "nativeDynamicCaller" (0) => native_dynamic_caller]
    }
}
fn native_caller_sources(context: &mut NativeContext<'_, CallerSourcesHost>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    context.collect()?;
    let sources=context.call_stack_source_names();
    let sources=sources.iter().map(|source|context.string_rooted(source)).collect::<Vec<_>>();
    context.array_rooted(&sources)
}
fn native_dynamic_caller(context: &mut NativeContext<'_, CallerSourcesHost>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
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
        Runtime::new(CallerSourcesHost).execute(&program).unwrap();
    }
}

struct SnapshotMetadataHost;
impl Host for SnapshotMetadataHost {
    fn write_line(&mut self, _: &str) {}
    fn clock_millis(&mut self) -> f64 { 0.0 }
    fn initialize(context: &mut NativeContext<'_, Self>) -> Result<(), RootedError> { context.seal_startup_intrinsics(); Ok(()) }
    fn functions(&self) -> &[HostFunction<Self>] { crate::host_functions![global "nativeSnapshotMetadata" (1) => native_snapshot_metadata] }
}
fn native_snapshot_metadata(context: &mut NativeContext<'_, SnapshotMetadataHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    let target = context.proxy_target_rooted(value)?.unwrap_or_else(|| context.undefined());
    context.collect()?;
    let object = context.boolean(context.has_startup_global_name("Object"));
    let added = context.boolean(context.has_startup_global_name("AddedAfterBootstrap"));
    context.array_rooted(&[object, added, target])
}
#[test]
fn native_bootstrap_names_and_proxy_targets_survive_mutation_revocation_and_collection() {
    let source=r#"
      delete globalThis.Object;globalThis.AddedAfterBootstrap=function AddedAfterBootstrap(){};
      const target={marker:42};const pair=Proxy.revocable(target,{});
      const info=nativeSnapshotMetadata(pair.proxy);
      if(info[0]!==true||info[1]!==false||info[2]!==target)throw Error('snapshot or original target changed');
      pair.revoke();if(nativeSnapshotMetadata(pair.proxy)[2]!==null)throw Error('revoked target changed');
      if(nativeSnapshotMetadata(target)[2]!==undefined)throw Error('ordinary object acquired proxy slots');
    "#;
    for program in [Engine::compile(crate::ExecutionRequest{source,name:"native-snapshot-metadata.js",kind:crate::SourceKind::Script}).unwrap(),Engine::specialize(source,"native-snapshot-metadata.js").unwrap()] {
        Runtime::new(SnapshotMetadataHost).execute(&program).unwrap();
    }
}

struct BoundedViewHost;
impl Host for BoundedViewHost {
    fn write_line(&mut self,_:&str){}
    fn clock_millis(&mut self)->f64{0.0}
    fn functions(&self)->&[HostFunction<Self>]{crate::host_functions![global "nativeBoundedView" (1) => native_bounded_view]}
}
fn native_bounded_view(context:&mut NativeContext<'_,BoundedViewHost>,_:RootId,args:&[RootId])->Result<RootId,RootedError>{
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
    for program in [Engine::compile(crate::ExecutionRequest{source,name:"native-bounded-view.js",kind:crate::SourceKind::Script}).unwrap(),Engine::specialize(source,"native-bounded-view.js").unwrap()]{Runtime::new(BoundedViewHost).execute(&program).unwrap();}
}

struct NonIndexKeyHost;
impl Host for NonIndexKeyHost {
    fn write_line(&mut self, _: &str) {}
    fn clock_millis(&mut self) -> f64 { 0.0 }
    fn functions(&self) -> &[HostFunction<Self>] {
        crate::host_functions![global "nativeNonIndexKeys" (2) => native_non_index_keys]
    }
}
fn native_non_index_keys(context: &mut NativeContext<'_, NonIndexKeyHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
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
        Runtime::new(NonIndexKeyHost).execute(&program).unwrap();
    }
}

struct BootstrapRegExpHost;
impl Host for BootstrapRegExpHost {
    fn write_line(&mut self, _: &str) {}
    fn clock_millis(&mut self) -> f64 { 0.0 }
    fn initialize(context: &mut NativeContext<'_, Self>) -> Result<(), RootedError> {
        context.seal_startup_intrinsics(); Ok(())
    }
    fn functions(&self) -> &[HostFunction<Self>] {
        crate::host_functions![global "nativeBootstrapRegExp" (1) => native_bootstrap_regexp]
    }
}
fn native_bootstrap_regexp(context: &mut NativeContext<'_, BootstrapRegExpHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
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
        Runtime::new(BootstrapRegExpHost).execute(&program).unwrap();
    }
}

struct PromiseSlotsHost;
impl Host for PromiseSlotsHost {
    fn write_line(&mut self, _: &str) {}
    fn clock_millis(&mut self) -> f64 { 0.0 }
    fn functions(&self) -> &[HostFunction<Self>] {
        crate::host_functions![global "promiseSlots" (1) => promise_slots]
    }
}
fn promise_slots(context: &mut NativeContext<'_, PromiseSlotsHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
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
        let mut runtime=Runtime::new(PromiseSlotsHost);
        runtime.execute(&program).unwrap();
        assert!(runtime.take_promise_rejection_unhandled_events().is_empty());
    }
}
