//! Timer globals polyfill for rquickjs.
//!
//! Provides `setInterval`, `clearInterval`, `setTimeout`, and `clearTimeout`
//! for the rquickjs runtime. In --once mode (static rendering), timers are
//! registered but never fire.

use rquickjs::{Ctx, Function, Object, Result as JsResult};

/// Install timer globals into the given rquickjs Context.
pub fn install(ctx: &Ctx<'_>) -> JsResult<()> {
    init_storage(ctx)?;
    install_set_interval(ctx)?;
    install_clear_interval(ctx)?;
    install_set_timeout(ctx)?;
    install_clear_timeout(ctx)
}

fn init_storage<'js>(ctx: &Ctx<'js>) -> JsResult<()> {
    let globals = ctx.globals();
    let storage = Object::new(ctx.clone())?;
    storage.set("next_id", 1)?;
    storage.set("timers", Object::new(ctx.clone())?)?;
    globals.set("__runts_timer_storage", storage)
}

fn install_set_interval<'js>(ctx: &Ctx<'js>) -> JsResult<()> {
    let globals = ctx.globals();
    let f = Function::new(ctx.clone(),
        |ctx: Ctx<'js>, cb: Function<'js>, ms: i32| -> JsResult<i32> {
            reg_timer(ctx, cb, ms, false)
        },
    )?;
    globals.set("setInterval", f)
}

fn install_set_timeout<'js>(ctx: &Ctx<'js>) -> JsResult<()> {
    let globals = ctx.globals();
    let f = Function::new(ctx.clone(),
        |ctx: Ctx<'js>, cb: Function<'js>, ms: i32| -> JsResult<i32> {
            reg_timer(ctx, cb, ms, true)
        },
    )?;
    globals.set("setTimeout", f)
}

fn reg_timer<'js>(
    ctx: Ctx<'js>,
    cb: Function<'js>,
    ms: i32,
    is_timeout: bool,
) -> JsResult<i32> {
    let storage: Object<'js> = ctx.globals().get("__runts_timer_storage")?;
    let next_id: i32 = storage.get("next_id")?;
    let timers: Object<'js> = storage.get("timers")?;
    let timer = Object::new(ctx.clone())?;
    timer.set("cb", cb)?;
    timer.set("ms", ms)?;
    timer.set("active", true)?;
    timer.set("is_timeout", is_timeout)?;
    timers.set(next_id.to_string(), timer)?;
    storage.set("next_id", next_id + 1)?;
    Ok(next_id)
}

fn install_clear_interval<'js>(ctx: &Ctx<'js>) -> JsResult<()> {
    let globals = ctx.globals();
    let f = Function::new(ctx.clone(),
        |ctx: Ctx<'js>, id: i32| -> JsResult<()> {
            clear_timer(ctx, id)
        },
    )?;
    globals.set("clearInterval", f)
}

fn install_clear_timeout<'js>(ctx: &Ctx<'js>) -> JsResult<()> {
    let globals = ctx.globals();
    let f = Function::new(ctx.clone(),
        |ctx: Ctx<'js>, id: i32| -> JsResult<()> {
            clear_timer(ctx, id)
        },
    )?;
    globals.set("clearTimeout", f)
}

fn clear_timer<'js>(ctx: Ctx<'js>, id: i32) -> JsResult<()> {
    let storage: Object<'js> = ctx.globals().get("__runts_timer_storage")?;
    let timers: Object<'js> = storage.get("timers")?;
    if let Ok(t) = timers.get::<_, Object>(id.to_string()) {
        let _ = t.set("active", false);
    }
    Ok(())
}
