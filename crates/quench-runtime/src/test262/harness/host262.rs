//! $262 host API object for test262

use crate::context::CURRENT_CONTEXT;
use crate::test262::harness::make_native;
use crate::value::{Object, ObjectKind};
use crate::{Context, JsError, Value};
use std::cell::RefCell;
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

/// Realm evalScript - reuses the realm's stored context so that modifications
/// to the realm's builtins (e.g. Object.setPrototypeOf(other.Number.prototype, ...))
/// persist across eval calls.
fn realm_eval_script(
    realm_ctx: &RefCell<Option<Context>>,
    args: Vec<Value>,
) -> Result<Value, JsError> {
    let code = args
        .first()
        .map(crate::value::to_js_string)
        .unwrap_or_default();
    let mut ctx = realm_ctx
        .borrow_mut()
        .take()
        .expect("realm context missing");
    let result = ctx.eval(&code);
    // Put the context back for the next call
    *realm_ctx.borrow_mut() = Some(ctx);
    result
}

/// $262.createRealm - creates a realm-like global facade.
/// The realm stores its own Context so that builtin modifications persist across
/// eval calls (e.g., Object.setPrototypeOf(other.Number.prototype, proxy)).
fn host_262_create_realm(_args: Vec<Value>) -> Result<Value, JsError> {
    let mut ctx = Context::new()?;
    crate::test262::harness::inject_harness(&mut ctx);
    let Value::Object(global) = ctx.get_global("globalThis").unwrap_or(Value::Undefined) else {
        return Err(JsError("createRealm: globalThis missing".to_string()));
    };

    // Create a shared context storage; we need interior mutability so the
    // realm_eval_script closure (which must be 'static) can mutate it.
    let realm_ctx = Rc::new(RefCell::new(Some(ctx)));

    // Set realm's eval to use the shared context
    let eval_ctx = Rc::clone(&realm_ctx);
    global.borrow_mut().set(
        "eval",
        make_native(move |args| realm_eval_script(&eval_ctx, args)),
    );

    // Create the realm facade object
    let mut realm = Object::new(ObjectKind::Ordinary);
    realm.set("global", Value::Object(Rc::clone(&global)));
    realm.set(
        "evalScript",
        make_native(move |args| realm_eval_script(&Rc::clone(&realm_ctx), args)),
    );
    realm.set("gc", make_native(host_262_gc));
    realm.set("detachArrayBuffer", make_native(host_262_detach_buffer));

    Ok(Value::Object(Rc::new(RefCell::new(realm))))
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
