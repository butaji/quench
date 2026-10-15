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
