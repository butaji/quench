//! $262 host API object for test262

use crate::context::CURRENT_CONTEXT;
use crate::test262::harness::make_native;
use crate::value::{Object, ObjectKind};
use crate::{Context, JsError, Value};
use std::rc::Rc;

/// $262.gc - trigger garbage collection (not supported, throws ReferenceError)
pub fn host_262_gc(_args: Vec<Value>) -> Result<Value, JsError> {
    let msg = "ReferenceError: $262.gc is not supported".to_string();
    let (err_val, js_err) = crate::value::error::create_js_error(&msg);
    crate::value::set_thrown_value(err_val);
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
        let (err_val, js_err) = crate::value::error::create_js_error(&msg);
        crate::value::set_thrown_value(err_val);
        Err(js_err)
    }
}

/// Realm evalScript - evaluates code in a new context
fn realm_eval_script(args: Vec<Value>) -> Result<Value, JsError> {
    let code = args
        .first()
        .map(crate::value::to_js_string)
        .unwrap_or_default();
    let mut realm_ctx = Context::new()?;
    crate::test262::harness::inject_harness(&mut realm_ctx);
    realm_ctx.eval(&code)
}

/// $262.createRealm - creates a realm-like global facade.
fn host_262_create_realm(_args: Vec<Value>) -> Result<Value, JsError> {
    let global = create_realm_global()?;
    let mut realm = Object::new(ObjectKind::Ordinary);
    realm.set("global", global);
    realm.set("evalScript", make_native(realm_eval_script));
    realm.set("gc", make_native(host_262_gc));
    realm.set("detachArrayBuffer", make_native(host_262_detach_buffer));
    Ok(Value::Object(Rc::new(std::cell::RefCell::new(realm))))
}

fn create_realm_global() -> Result<Value, JsError> {
    let mut ctx = Context::new()?;
    mirror_realm_bindings(&mut ctx);
    let Value::Object(global) = ctx.get_global("globalThis").unwrap_or(Value::Undefined) else {
        return Err(JsError("createRealm: globalThis missing".to_string()));
    };
    let eval_global = Rc::clone(&global);
    global.borrow_mut().set(
        "eval",
        make_native(move |args| eval_realm_global(&eval_global, args)),
    );
    Ok(Value::Object(global))
}

fn mirror_realm_bindings(ctx: &mut Context) {
    let names = ["Number", "String", "Boolean", "Symbol"];
    let Some(Value::Object(global)) = ctx.get_global("globalThis") else {
        return;
    };
    for name in names {
        if let Some(value) = ctx.get_global(name) {
            global.borrow_mut().set(name, value);
        }
    }
}

fn eval_realm_global(
    global: &Rc<std::cell::RefCell<Object>>,
    args: Vec<Value>,
) -> Result<Value, JsError> {
    let code = args
        .first()
        .map(crate::value::to_js_string)
        .unwrap_or_default();
    let mut ctx = Context::new()?;
    copy_global_properties(global, &mut ctx);
    let result = ctx.eval(&code)?;
    sync_global_properties(&ctx, global);
    Ok(result)
}

fn copy_global_properties(global: &Rc<std::cell::RefCell<Object>>, ctx: &mut Context) {
    for key in global.borrow().own_keys() {
        if let Some(value) = global.borrow().get(&key) {
            ctx.set_global(key, value);
        }
    }
}

fn sync_global_properties(ctx: &Context, global: &Rc<std::cell::RefCell<Object>>) {
    if let Some(Value::Object(evaluated)) = ctx.get_global("globalThis") {
        for key in evaluated.borrow().own_keys() {
            if let Some(value) = evaluated.borrow().get(&key) {
                global.borrow_mut().set(&key, value);
            }
        }
    }
}

/// $262.evalScript - evaluates code in the current context
fn host_262_eval_script(args: Vec<Value>) -> Result<Value, JsError> {
    let code = args
        .first()
        .map(crate::value::to_js_string)
        .unwrap_or_default();
    let ctx_ptr: *mut Context = CURRENT_CONTEXT.with(|cell| {
        cell.borrow()
            .map_or_else(std::ptr::null_mut, |ctx| ctx as *mut _)
    });
    if ctx_ptr.is_null() {
        let msg = "$262.evalScript: no active context".to_string();
        let (err_val, js_err) = crate::value::error::create_js_error(&msg);
        crate::value::set_thrown_value(err_val);
        return Err(js_err);
    }
    let ctx = unsafe { &mut *ctx_ptr };
    ctx.eval(&code)
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

/// Inject full $262 host API (createRealm, evalScript, gc, detachArrayBuffer).
/// Call this AFTER harness files are loaded.
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
}
