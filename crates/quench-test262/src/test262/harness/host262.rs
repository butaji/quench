//! $262 host API object for test262

use crate::harness::make_native;
use quench_runtime::ast::Program;
use quench_runtime::context::CURRENT_CONTEXT;
use quench_runtime::value::{Object, ObjectKind};
use quench_runtime::{Context, JsError, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// $262.gc - trigger garbage collection (not supported, throws ReferenceError)
pub fn host_262_gc(_args: Vec<Value>) -> Result<Value, JsError> {
    let msg = "ReferenceError: $262.gc is not supported".to_string();
    let (err_val, js_err) = quench_runtime::value::error::create_js_error(&msg);
    quench_runtime::value::set_thrown_value(err_val);
    Err(js_err)
}

/// $262.detachArrayBuffer - detaches an ArrayBuffer
pub fn host_262_detach_buffer(args: Vec<Value>) -> Result<Value, JsError> {
    let buffer = args.first().cloned().unwrap_or(Value::Undefined);
    if let Value::Object(obj) = buffer {
        let mut obj_mut = obj.borrow_mut();
        obj_mut.set("detached", Value::Boolean(true));
        obj_mut.set("byteLength", Value::Number(0.0));
        Ok(Value::Undefined)
    } else {
        let msg = "$262.detachArrayBuffer: buffer object required".to_string();
        let (err_val, js_err) = quench_runtime::value::error::create_js_error(&msg);
        quench_runtime::value::set_thrown_value(err_val);
        Err(js_err)
    }
}

/// Realm evalScript - reuses the realm's stored context so that modifications
/// to the realm's builtins (e.g. Object.setPrototypeOf(other.Number.prototype, ...))
/// persist across eval calls.
fn realm_eval_script(
    realm_ctx: &RefCell<Option<Context>>,
    realm_intrinsics: &RefCell<Option<quench_runtime::RealmSnapshot>>,
    args: Vec<Value>,
) -> Result<Value, JsError> {
    let code = args
        .first()
        .map(quench_runtime::value::to_js_string)
        .unwrap_or_default();
    let taken = realm_ctx.borrow_mut().take();
    let Some(mut ctx) = taken else {
        let msg = "realm.evalScript: realm context missing (reentrant call)".to_string();
        let (err_val, js_err) = quench_runtime::value::error::create_js_error(&msg);
        quench_runtime::value::set_thrown_value(err_val);
        return Err(js_err);
    };
    let caller_intrinsics = quench_runtime::RealmSnapshot::capture();
    let realm_snapshot = realm_intrinsics.borrow_mut().take();
    if let Some(snapshot) = realm_snapshot {
        snapshot.restore();
    }
    // evalScript runs a NEW script: non-strict unless the source itself
    // declares 'use strict' — never inherit the caller's strictness.
    let was_strict = quench_runtime::api::strict_mode();
    let was_direct_eval = quench_runtime::api::direct_eval();
    quench_runtime::api::set_strict_mode(false);
    quench_runtime::api::set_direct_eval(false);
    let result = ctx.eval(&code);
    quench_runtime::api::set_strict_mode(was_strict);
    quench_runtime::api::set_direct_eval(was_direct_eval);
    let updated_realm = quench_runtime::RealmSnapshot::capture();
    *realm_intrinsics.borrow_mut() = Some(updated_realm);
    caller_intrinsics.restore();
    // Put the context back for the next call
    *realm_ctx.borrow_mut() = Some(ctx);
    result
}

/// $262.createRealm - creates a realm-like global facade.
/// The realm stores its own Context so that builtin modifications persist across
/// eval calls (e.g., Object.setPrototypeOf(other.Number.prototype, proxy)).
fn host_262_create_realm(_args: Vec<Value>) -> Result<Value, JsError> {
    // Building the sub-realm overwrites the shared thread-local intrinsic
    // caches; snapshot them first and restore afterwards so the main realm
    // keeps its own intrinsics.
    let snapshot = quench_runtime::RealmSnapshot::capture();
    let mut ctx = Context::new()?;
    crate::harness::inject_harness(&mut ctx);
    snapshot.restore();
    let Value::Object(global) = ctx.get_global("globalThis").unwrap_or(Value::Undefined) else {
        return Err(JsError("createRealm: globalThis missing".to_string()));
    };

    // Create a shared context storage; we need interior mutability so the
    // realm_eval_script closure (which must be 'static) can mutate it.
    let realm_ctx = Rc::new(RefCell::new(Some(ctx)));
    let realm_intrinsics = Rc::new(RefCell::new(Some(quench_runtime::RealmSnapshot::capture())));

    // Set realm's eval to use the shared context
    let eval_ctx = Rc::clone(&realm_ctx);
    let eval_intrinsics = Rc::clone(&realm_intrinsics);
    global.borrow_mut().set(
        "eval",
        make_native(move |args| realm_eval_script(&eval_ctx, &eval_intrinsics, args)),
    );

    // Create the realm facade object
    let mut realm = Object::new(ObjectKind::Ordinary);
    realm.set("global", Value::Object(Rc::clone(&global)));
    realm.set(
        "evalScript",
        make_native(move |args| {
            realm_eval_script(&Rc::clone(&realm_ctx), &Rc::clone(&realm_intrinsics), args)
        }),
    );
    realm.set("gc", make_native(host_262_gc));
    realm.set("detachArrayBuffer", make_native(host_262_detach_buffer));

    Ok(Value::Object(Rc::new(RefCell::new(realm))))
}

/// $262.evalScript - evaluates code in the current context
fn host_262_eval_script(args: Vec<Value>) -> Result<Value, JsError> {
    let code = args
        .first()
        .map(quench_runtime::value::to_js_string)
        .unwrap_or_default();
    let ctx_ptr: *mut Context = CURRENT_CONTEXT.with(|cell| {
        cell.borrow()
            .map_or_else(std::ptr::null_mut, |ctx| ctx as *mut _)
    });
    if ctx_ptr.is_null() {
        let msg = "$262.evalScript: no active context".to_string();
        let (err_val, js_err) = quench_runtime::value::error::create_js_error(&msg);
        quench_runtime::value::set_thrown_value(err_val);
        return Err(js_err);
    }
    let ctx = unsafe { &mut *ctx_ptr };
    // evalScript runs a NEW script: non-strict unless the source itself
    // declares 'use strict' — never inherit the caller's strictness.
    let was_strict = quench_runtime::api::strict_mode();
    let was_direct_eval = quench_runtime::api::direct_eval();
    quench_runtime::api::set_strict_mode(false);
    quench_runtime::api::set_direct_eval(false);
    let result = (|| {
        reject_restricted_global_lexicals(ctx, &code)?;
        let function_names = global_function_names(ctx, &code)?;
        let value = ctx.eval(&code)?;
        set_global_function_descriptors(ctx, &function_names);
        Ok(value)
    })();
    quench_runtime::api::set_strict_mode(was_strict);
    quench_runtime::api::set_direct_eval(was_direct_eval);
    result
}

fn global_function_names(ctx: &Context, source: &str) -> Result<Vec<String>, JsError> {
    let Program::Script(body) = ctx.parse(source)?;
    Ok(body
        .iter()
        .filter_map(|statement| match statement {
            quench_runtime::ast::Statement::FunctionDeclaration { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect())
}

fn set_global_function_descriptors(ctx: &Context, names: &[String]) {
    let Some(Value::Object(global)) = ctx.get_global("globalThis") else {
        return;
    };
    for name in names {
        let Some(value) = global.borrow().get(name) else {
            continue;
        };
        global.borrow_mut().define(
            name,
            value.clone(),
            quench_runtime::value::PropertyFlags {
                value: Some(value),
                writable: true,
                enumerable: true,
                configurable: false,
            },
        );
    }
}

fn reject_restricted_global_lexicals(ctx: &Context, source: &str) -> Result<(), JsError> {
    let Program::Script(body) = ctx.parse(source)?;
    let names = quench_runtime::interpreter::collect_let_const_declarations(&body);
    let Some(Value::Object(global)) = ctx.get_global("globalThis") else {
        return Ok(());
    };
    let current_env = quench_runtime::context::get_current_env();
    let has_lexical = |name: &str| {
        current_env
            .as_ref()
            .is_some_and(|env| match env.borrow().get_kind(name) {
                Some(quench_runtime::ast::VarKind::Let | quench_runtime::ast::VarKind::Const) => {
                    true
                }
                Some(quench_runtime::ast::VarKind::Var) => global
                    .borrow()
                    .get_own_property(name)
                    .is_some_and(|descriptor| descriptor.configurable == Some(false)),
                None => false,
            })
    };
    let has_declarative_lexical = |name: &str| {
        current_env.as_ref().is_some_and(|env| {
            matches!(
                env.borrow().get_kind(name),
                Some(quench_runtime::ast::VarKind::Let | quench_runtime::ast::VarKind::Const)
            )
        })
    };
    for (name, _) in &names {
        if has_lexical(name) {
            let (error, js_error) = quench_runtime::value::error::create_js_error_with_type(
                "Identifier has already been declared",
                "SyntaxError",
            );
            quench_runtime::value::set_thrown_value(error);
            return Err(js_error);
        }
    }
    let mut var_names = Vec::new();
    quench_runtime::interpreter::collect_var_names_recursive(&body, &mut var_names);
    for name in &var_names {
        if has_declarative_lexical(name) {
            let (error, js_error) = quench_runtime::value::error::create_js_error_with_type(
                "Identifier has already been declared",
                "SyntaxError",
            );
            quench_runtime::value::set_thrown_value(error);
            return Err(js_error);
        }
    }
    for statement in &body {
        let Some(name) = (match statement {
            quench_runtime::ast::Statement::FunctionDeclaration { name, .. } => Some(name),
            _ => None,
        }) else {
            continue;
        };
        if has_declarative_lexical(name) {
            let (error, js_error) = quench_runtime::value::error::create_js_error_with_type(
                "Identifier has already been declared",
                "SyntaxError",
            );
            quench_runtime::value::set_thrown_value(error);
            return Err(js_error);
        }
    }
    if !global.borrow().extensible {
        for name in var_names {
            if !global.borrow().has_own(&name) {
                let (error, js_error) = quench_runtime::value::error::create_js_error_with_type(
                    "Cannot declare global variable on a non-extensible object",
                    "TypeError",
                );
                quench_runtime::value::set_thrown_value(error);
                return Err(js_error);
            }
        }
    }
    for statement in &body {
        let Some(name) = (match statement {
            quench_runtime::ast::Statement::FunctionDeclaration { name, .. } => Some(name),
            _ => None,
        }) else {
            continue;
        };
        let descriptor = global.borrow().get_own_property(name);
        let allowed = match descriptor {
            None => global.borrow().extensible,
            Some(descriptor) if descriptor.configurable == Some(true) => true,
            Some(descriptor) => {
                descriptor.is_data()
                    && descriptor.writable == Some(true)
                    && descriptor.enumerable == Some(true)
            }
        };
        if !allowed {
            let (error, js_error) = quench_runtime::value::error::create_js_error_with_type(
                "Cannot declare global function",
                "TypeError",
            );
            quench_runtime::value::set_thrown_value(error);
            return Err(js_error);
        }
    }
    for (name, _) in names {
        if global
            .borrow()
            .get_own_property(&name)
            .is_some_and(|descriptor| descriptor.configurable == Some(false))
            && quench_runtime::context::get_current_env()
                .and_then(|env| env.borrow().get_kind(&name))
                != Some(quench_runtime::ast::VarKind::Var)
        {
            let (error, js_error) = quench_runtime::value::error::create_js_error_with_type(
                "Identifier conflicts with a restricted global property",
                "SyntaxError",
            );
            quench_runtime::value::set_thrown_value(error);
            return Err(js_error);
        }
    }
    Ok(())
}

/// Inject $262.agent stub BEFORE loading harness files.
/// atomicsHelper.js references $262.agent.getReport.bind.
pub fn inject_stub_agent(ctx: &mut Context) {
    let mut agent = Object::new(ObjectKind::Ordinary);
    agent.set("sleep", make_native(|_| Ok(Value::Undefined)));
    agent.set("getReport", make_native(|_| Ok(Value::Undefined)));
    agent.set("report", make_native(|_| Ok(Value::Undefined)));
    agent.set("broadcast", make_native(|_| Ok(Value::Undefined)));
    agent.set("start", make_native(|_| Ok(Value::Undefined)));
    agent.set("leave", make_native(|_| Ok(Value::Undefined)));
    agent.set("leaving", make_native(|_| Ok(Value::Undefined)));
    agent.set("receiveBroadcast", make_native(|_| Ok(Value::Undefined)));
    agent.set("waitUntil", make_native(|_| Ok(Value::Undefined)));
    let mut timeouts = Object::new(ObjectKind::Ordinary);
    timeouts.set("yield", Value::Number(100.0));
    timeouts.set("small", Value::Number(200.0));
    timeouts.set("long", Value::Number(1000.0));
    timeouts.set("huge", Value::Number(10000.0));
    agent.set(
        "timeouts",
        Value::Object(Rc::new(std::cell::RefCell::new(timeouts))),
    );

    let mut obj = Object::new(ObjectKind::Ordinary);
    obj.set(
        "agent",
        Value::Object(Rc::new(std::cell::RefCell::new(agent))),
    );
    ctx.set_global(
        "$262".to_string(),
        Value::Object(Rc::new(std::cell::RefCell::new(obj))),
    );
}

/// $262.AbstractModuleSource constructor — throws TypeError when invoked.
fn host_262_abstract_module_source(_args: Vec<Value>) -> Result<Value, JsError> {
    let msg = "TypeError: AbstractModuleSource cannot be called directly".to_string();
    let (err_val, js_err) =
        quench_runtime::value::error::create_js_error_with_type(&msg, "TypeError");
    quench_runtime::value::set_thrown_value(err_val);
    Err(js_err)
}

/// Inject $262.AbstractModuleSource per ECMA-262 §28.1.1.1.
fn inject_abstract_module_source(ctx: &mut Context) {
    use quench_runtime::value::NativeConstructor;
    let proto = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
    if let Some(object_proto) = quench_runtime::builtins::get_object_prototype() {
        proto.borrow_mut().prototype = Some(object_proto);
    }
    if let Some(Value::Symbol(tag_key)) =
        quench_runtime::builtins::symbol::get_well_known_symbol_no_ctx("toStringTag")
    {
        let getter = make_native(|_args| Ok(Value::Undefined));
        proto.borrow_mut().define_accessor(
            &tag_key.property_key(),
            Some(getter),
            None,
            quench_runtime::value::PropertyFlags {
                writable: false,
                enumerable: false,
                configurable: true,
                ..Default::default()
            },
        );
        if let Some(flags) = proto
            .borrow_mut()
            .descriptors
            .get_mut(&tag_key.property_key())
        {
            flags.enumerable = false;
            flags.configurable = true;
        }
    }

    let constructor = NativeConstructor::new(host_262_abstract_module_source, Rc::clone(&proto));
    constructor.set_name("AbstractModuleSource");
    let ctor_val = Value::NativeConstructor(Rc::new(constructor));
    proto.borrow_mut().set("constructor", ctor_val.clone());
    if let Some(flags) = proto.borrow_mut().descriptors.get_mut("constructor") {
        flags.writable = true;
        flags.enumerable = false;
        flags.configurable = true;
    }
    if let Some(Value::Object(obj)) = ctx.get_global("$262") {
        obj.borrow_mut().set("AbstractModuleSource", ctor_val);
        if let Some(flags) = obj.borrow_mut().descriptors.get_mut("AbstractModuleSource") {
            flags.writable = false;
            flags.enumerable = false;
            flags.configurable = true;
        }
    }
}

/// Inject full $262 host API (createRealm, evalScript, gc, detachArrayBuffer,
/// AbstractModuleSource). Call this AFTER harness files are loaded.
pub fn inject(ctx: &mut Context) {
    // Inject stub first if $262 doesn't exist yet
    if ctx.get_global("$262").is_none() {
        inject_stub_agent(ctx);
    }
    // Now add the non-stub methods
    if let Some(Value::Object(obj)) = ctx.get_global("$262") {
        let mut o = obj.borrow_mut();
        o.set("createRealm", make_native(host_262_create_realm));
        o.set("evalScript", make_native(host_262_eval_script));
        o.set("gc", make_native(host_262_gc));
        o.set("detachArrayBuffer", make_native(host_262_detach_buffer));
    }
    inject_abstract_module_source(ctx);
}

#[cfg(test)]
mod tests {
    use crate::harness::try_inject_harness;

    fn harness_ctx() -> quench_runtime::Context {
        let mut ctx = quench_runtime::Context::new().unwrap();
        try_inject_harness(&mut ctx).unwrap();
        ctx
    }

    #[test]
    fn test_create_realm_returns_object() {
        let mut ctx = harness_ctx();
        let result = ctx.eval("typeof $262.createRealm() === 'object'");
        assert!(
            result.is_ok(),
            "$262.createRealm should return object: {:?}",
            result
        );
    }

    #[test]
    fn test_create_realm_has_global() {
        let mut ctx = harness_ctx();
        let result = ctx.eval("typeof $262.createRealm().global === 'object'");
        assert!(
            result.is_ok(),
            "realm.global should be object: {:?}",
            result
        );
    }

    #[test]
    fn test_create_realm_has_eval_script() {
        let mut ctx = harness_ctx();
        let result = ctx.eval("typeof $262.createRealm().evalScript === 'function'");
        assert!(
            result.is_ok(),
            "realm.evalScript should be function: {:?}",
            result
        );
    }

    #[test]
    fn test_create_realm_eval_script_runs() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "var realm = $262.createRealm(); realm.evalScript('var x = 42'); realm.global.x === 42",
        );
        assert!(
            result.is_ok(),
            "realm.evalScript should run code: {:?}",
            result
        );
    }

    #[test]
    fn test_create_realm_separate_globals() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "var realm1 = $262.createRealm(); var realm2 = $262.createRealm(); realm1.global.x = 1; realm2.global.x = 2; (realm1.global.x === 1 && realm2.global.x === 2)",
        );
        assert!(
            result.is_ok(),
            "realms should have separate globals: {:?}",
            result
        );
    }

    #[test]
    fn test_create_realm_preserves_modifications() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "var realm = $262.createRealm(); realm.evalScript('Object.prototype.customProp = 42'); realm.global.Object.prototype.customProp === 42",
        );
        assert!(
            result.is_ok(),
            "realm modifications should persist: {:?}",
            result
        );
    }

    #[test]
    fn test_create_realm_has_error_constructors() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "var realm = $262.createRealm(); var err = new realm.global.TypeError('test'); err.constructor === realm.global.TypeError",
        );
        assert!(result.is_ok(), "realm should have TypeError: {:?}", result);
    }

    #[test]
    fn test_create_realm_does_not_contaminate_main_realm_intrinsics() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "var realm = $262.createRealm(); \
             Object.getPrototypeOf([]) === Array.prototype && \
             Object.getPrototypeOf({}) === Object.prototype && \
             Object.getPrototypeOf(function(){}) === Function.prototype && \
             Object.getPrototypeOf(/x/) === RegExp.prototype",
        );
        assert_eq!(
            result,
            Ok(quench_runtime::Value::Boolean(true)),
            "createRealm must not repoint main-realm intrinsic caches"
        );
    }

    #[test]
    fn test_create_realm_restores_harness_caches() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "var before = fnGlobalObject(); var realm = $262.createRealm(); \
             fnGlobalObject() === before && fnGlobalObject() === globalThis",
        );
        assert_eq!(
            result,
            Ok(quench_runtime::Value::Boolean(true)),
            "createRealm must restore harness GLOBAL_OBJECT cache"
        );
    }

    #[test]
    fn test_eval_script_runs() {
        let mut ctx = harness_ctx();
        let result = ctx.eval("$262.evalScript('var y = 123'); y === 123");
        assert!(result.is_ok(), "$262.evalScript should run: {:?}", result);
    }

    #[test]
    fn test_eval_script_runs_sloppy_in_strict_context() {
        // Official evalScript runs a NEW script: always non-strict unless the
        // source itself declares 'use strict'. Inside a strict context,
        // sloppy-only syntax (strict reserved word as binding) must parse.
        let mut ctx = harness_ctx();
        let prev = quench_runtime::api::strict_mode();
        quench_runtime::api::set_strict_mode(true);
        let result = ctx.eval("$262.evalScript('var public = 1;')");
        quench_runtime::api::set_strict_mode(prev);
        assert!(
            result.is_ok(),
            "$262.evalScript must run as a new sloppy script: {:?}",
            result
        );
    }

    #[test]
    fn test_realm_eval_script_runs_sloppy_in_strict_context() {
        let mut ctx = harness_ctx();
        let prev = quench_runtime::api::strict_mode();
        quench_runtime::api::set_strict_mode(true);
        let result =
            ctx.eval("var realm = $262.createRealm(); realm.evalScript('var public = 1;')");
        quench_runtime::api::set_strict_mode(prev);
        assert!(
            result.is_ok(),
            "realm.evalScript must run as a new sloppy script: {:?}",
            result
        );
    }

    #[test]
    fn test_realm_eval_script_reentrant_returns_error_not_panic() {
        // Nested realm.evalScript must return a JsError, never panic.
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "var realm = $262.createRealm(); realm.global.trigger = function() { realm.evalScript('1'); }; realm.evalScript('trigger()');",
        );
        assert!(
            result.is_err(),
            "reentrant realm.evalScript should return an error, not panic: {:?}",
            result
        );
    }

    #[test]
    fn test_eval_script_returns_value() {
        let mut ctx = harness_ctx();
        let result = ctx.eval("$262.evalScript('42') === 42");
        assert!(
            result.is_ok(),
            "$262.evalScript should return value: {:?}",
            result
        );
    }

    #[test]
    fn test_eval_script_survives_nested_context_eval() {
        let mut ctx = harness_ctx();
        let result = ctx.eval("eval('1'); $262.evalScript('var nested = 1'); nested === 1");
        assert!(
            result.is_ok(),
            "$262.evalScript lost its context: {result:?}"
        );
    }

    #[test]
    fn test_eval_script_rejects_restricted_lexical_global() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "Object.defineProperty(this, 'restricted', { configurable: false }); \
             try { $262.evalScript('let x; let restricted;'); false } \
             catch (e) { e instanceof SyntaxError }",
        );
        assert_eq!(result, Ok(quench_runtime::Value::Boolean(true)));
    }

    #[test]
    fn test_eval_script_rejects_new_var_on_non_extensible_global() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "Object.preventExtensions(this); \
             try { $262.evalScript('var freshGlobal;'); false } \
             catch (e) { e instanceof TypeError }",
        );
        assert_eq!(result, Ok(quench_runtime::Value::Boolean(true)));
    }

    #[test]
    fn test_eval_script_rejects_incompatible_global_function() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "Object.defineProperty(this, 'fixed', { configurable: false, writable: false }); \
             try { $262.evalScript('function fixed() {}'); false } \
             catch (e) { e instanceof TypeError }",
        );
        assert_eq!(result, Ok(quench_runtime::Value::Boolean(true)));
    }

    #[test]
    fn test_eval_script_rejects_existing_lexical_binding() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "let existing; try { $262.evalScript('var x; let existing;'); false } \
             catch (e) { e instanceof SyntaxError }",
        );
        assert_eq!(result, Ok(quench_runtime::Value::Boolean(true)));
    }

    #[test]
    fn test_eval_script_function_binding_is_non_configurable() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "$262.evalScript('function freshFunction() {}'); \
             Object.getOwnPropertyDescriptor(this, 'freshFunction').configurable",
        );
        assert_eq!(result, Ok(quench_runtime::Value::Boolean(false)));
    }

    #[test]
    fn test_direct_eval_var_does_not_block_eval_script_lexical_binding() {
        let mut ctx = harness_ctx();
        ctx.eval("eval('var directEvalVar');").unwrap();
        ctx.eval("$262.evalScript('var evalVar; let directEvalVar;');")
            .unwrap();
    }

    #[test]
    fn test_source_var_blocks_eval_script_lexical_binding() {
        let mut ctx = harness_ctx();
        ctx.eval("var sourceVar;").unwrap();
        let error = ctx.eval("$262.evalScript('var evalVar; let sourceVar;');");
        assert!(error.is_err());
    }

    #[test]
    fn test_gc_throws_reference_error() {
        let mut ctx = harness_ctx();
        let result =
            ctx.eval("var threw = false; try { $262.gc(); } catch(e) { threw = true; } threw");
        assert!(result.is_ok(), "$262.gc should throw: {:?}", result);
        assert_eq!(result.unwrap(), quench_runtime::Value::Boolean(true));
    }

    #[test]
    fn test_detach_array_buffer() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "var buf = new ArrayBuffer(8); $262.detachArrayBuffer(buf); buf.byteLength === 0 && buf.detached === true",
        );
        assert!(
            result.is_ok(),
            "detachArrayBuffer should work: {:?}",
            result
        );
    }

    #[test]
    fn test_detach_array_buffer_wrong_type() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "var threw = false; try { $262.detachArrayBuffer({}); } catch(e) { threw = true; } threw",
        );
        assert!(
            result.is_ok(),
            "detachArrayBuffer wrong type should throw: {:?}",
            result
        );
    }

    #[test]
    fn test_agent_stub_methods_exist() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "$262.agent.sleep !== undefined && $262.agent.getReport !== undefined && $262.agent.report !== undefined",
        );
        assert!(
            result.is_ok(),
            "$262.agent stubs should exist: {:?}",
            result
        );
    }

    #[test]
    fn test_agent_timeouts() {
        let mut ctx = harness_ctx();
        let result = ctx.eval("typeof $262.agent.timeouts === 'object'");
        assert!(
            result.is_ok(),
            "$262.agent.timeouts should exist: {:?}",
            result
        );
    }

    #[test]
    fn test_cross_realm_typeerror_identity() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "var realm = $262.createRealm(); var localTE = TypeError; var realmTE = realm.global.TypeError; localTE !== realmTE",
        );
        assert!(
            result.is_ok(),
            "cross-realm constructors should differ: {:?}",
            result
        );
    }

    #[test]
    fn test_cross_realm_error_throws_type_mismatch() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "var realm = $262.createRealm(); var threw = false; try { assert.throws(TypeError, function() { throw new realm.global.TypeError(); }); } catch(e) { threw = true; } threw",
        );
        assert!(
            result.is_ok(),
            "cross-realm TypeError should not match local: {:?}",
            result
        );
        assert_eq!(result.unwrap(), quench_runtime::Value::Boolean(true));
    }

    #[test]
    fn test_abstract_module_source_is_function() {
        let mut ctx = harness_ctx();
        let result = ctx.eval("typeof $262.AbstractModuleSource");
        assert_eq!(
            result.unwrap(),
            quench_runtime::Value::String("function".into())
        );
    }

    #[test]
    fn test_abstract_module_source_length_is_zero() {
        let mut ctx = harness_ctx();
        let result = ctx.eval("$262.AbstractModuleSource.length");
        assert_eq!(result.unwrap(), quench_runtime::Value::Number(0.0));
    }

    #[test]
    fn test_abstract_module_source_name_is_constructor_name() {
        let mut ctx = harness_ctx();
        let result = ctx.eval("$262.AbstractModuleSource.name");
        assert_eq!(
            result.unwrap(),
            quench_runtime::Value::String("AbstractModuleSource".into())
        );
    }

    #[test]
    fn test_abstract_module_source_throws_typeerror() {
        let mut ctx = harness_ctx();
        let result = ctx.eval("try { new $262.AbstractModuleSource(); 'no' } catch(e) { e instanceof TypeError ? 'yes' : 'no' }");
        assert_eq!(result.unwrap(), quench_runtime::Value::String("yes".into()));
    }

    #[test]
    fn test_abstract_module_source_proto_is_function_prototype() {
        let mut ctx = harness_ctx();
        let result =
            ctx.eval("Object.getPrototypeOf($262.AbstractModuleSource) === Function.prototype");
        assert_eq!(result.unwrap(), quench_runtime::Value::Boolean(true));
    }

    #[test]
    fn test_abstract_module_source_prototype_inherits_object_prototype() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "Object.getPrototypeOf($262.AbstractModuleSource.prototype) === Object.prototype",
        );
        assert_eq!(result.unwrap(), quench_runtime::Value::Boolean(true));
    }

    #[test]
    fn test_abstract_module_source_prototype_descriptor_is_locked() {
        let mut ctx = harness_ctx();
        let result = ctx.eval("[Object.getOwnPropertyDescriptor($262.AbstractModuleSource, 'prototype').writable, Object.getOwnPropertyDescriptor($262.AbstractModuleSource, 'prototype').configurable].join('|')");
        assert_eq!(
            result.unwrap(),
            quench_runtime::Value::String("false|false".into())
        );
    }

    #[test]
    fn test_abstract_module_source_verify_property_passes() {
        let mut ctx = harness_ctx();
        ctx.eval("function verifyProperty(obj, name, desc) { var originalDesc = Object.getOwnPropertyDescriptor(obj, name); return originalDesc.writable === desc.writable && originalDesc.configurable === desc.configurable; }").unwrap();
        let result = ctx.eval("verifyProperty($262.AbstractModuleSource, 'prototype', { value: $262.AbstractModuleSource.prototype, writable: false, enumerable: false, configurable: false })");
        assert_eq!(result.unwrap(), quench_runtime::Value::Boolean(true));
    }
}
