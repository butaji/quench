//! Ink hook bridge — exposes `useInput`, `useApp`, etc.
//! to the rquickjs runtime.
//!
//! Each hook stores its state in JS globals (e.g.
//! `__runts_input_handlers`) so that the Rust event loop
//! can route crossterm events to JS callbacks without
//! keeping JS function references in Rust.

use rquickjs::{Ctx, Function, Object, Result as JsResult};

/// Install `runts_ink_hooks` global with all Ink hooks.
pub fn install<'js>(ctx: &Ctx<'js>) -> JsResult<()> {
    let hooks = Object::new(ctx.clone())?;
    add_input_hooks(ctx, &hooks)?;
    add_misc_hooks(ctx, &hooks)?;
    ctx.globals().set("runts_ink_hooks", hooks)
}

fn add_input_hooks<'js>(ctx: &Ctx<'js>, hooks: &Object<'js>) -> JsResult<()> {
    add_input_hook(ctx, hooks)?;
    add_app_hook(ctx, hooks)?;
    add_stdin_hook(ctx, hooks)?;
    add_stdout_hook(ctx, hooks)?;
    add_stderr_hook(ctx, hooks)?;
    Ok(())
}

fn add_misc_hooks<'js>(ctx: &Ctx<'js>, hooks: &Object<'js>) -> JsResult<()> {
    add_window_size_hook(ctx, hooks)?;
    add_focus_hook(ctx, hooks)?;
    add_focus_manager_hook(ctx, hooks)?;
    add_cursor_hook(ctx, hooks)?;
    add_animation_hook(ctx, hooks)?;
    Ok(())
}

fn add_input_hook<'js>(ctx: &Ctx<'js>, hooks: &Object<'js>) -> JsResult<()> {
    let f = Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>, handler: Function<'js>| -> JsResult<()> {
            let globals = ctx.globals();
            let arr: Object = globals
                .get("__runts_input_handlers")
                .unwrap_or_else(|_| Object::new(ctx.clone()).unwrap());
            let len = arr.len();
            arr.set(len.to_string(), handler)?;
            globals.set("__runts_input_handlers", arr)?;
            Ok(())
        },
    )?;
    hooks.set("useInput", f)
}

fn add_app_hook<'js>(ctx: &Ctx<'js>, hooks: &Object<'js>) -> JsResult<()> {
    let f = Function::new(ctx.clone(), |ctx: Ctx<'js>| -> JsResult<Object<'js>> {
        let obj = Object::new(ctx.clone())?;
        let exit = Function::new(
            ctx.clone(),
            |ctx: Ctx<'js>, code: i32| -> JsResult<()> {
                ctx.globals().set("__runts_exit", true)?;
                ctx.globals().set("__runts_exit_code", code)?;
                Ok(())
            },
        )?;
        obj.set("exit", exit)?;
        Ok(obj)
    })?;
    hooks.set("useApp", f)
}

fn add_stdin_hook<'js>(ctx: &Ctx<'js>, hooks: &Object<'js>) -> JsResult<()> {
    let f = Function::new(ctx.clone(), |ctx: Ctx<'js>| -> JsResult<Object<'js>> {
        let obj = Object::new(ctx.clone())?;
        obj.set("isRawModeSupported", true)?;
        let set_raw = Function::new(ctx.clone(), |_ctx: Ctx<'js>| -> JsResult<()> {
            Ok(())
        })?;
        obj.set("setRawMode", set_raw)?;
        Ok(obj)
    })?;
    hooks.set("useStdin", f)
}

fn add_stdout_hook<'js>(ctx: &Ctx<'js>, hooks: &Object<'js>) -> JsResult<()> {
    let f = Function::new(ctx.clone(), |ctx: Ctx<'js>| -> JsResult<Object<'js>> {
        let obj = Object::new(ctx.clone())?;
        let write = Function::new(
            ctx.clone(),
            |ctx: Ctx<'js>, msg: String| -> JsResult<()> {
                let host: Function = ctx.globals().get("__runts_stderr__")?;
                let _: rquickjs::Value = host.call((msg,))?;
                Ok(())
            },
        )?;
        obj.set("write", write)?;
        Ok(obj)
    })?;
    hooks.set("useStdout", f)
}

fn add_stderr_hook<'js>(ctx: &Ctx<'js>, hooks: &Object<'js>) -> JsResult<()> {
    let f = Function::new(ctx.clone(), |ctx: Ctx<'js>| -> JsResult<Object<'js>> {
        let obj = Object::new(ctx.clone())?;
        let write = Function::new(
            ctx.clone(),
            |ctx: Ctx<'js>, msg: String| -> JsResult<()> {
                let host: Function = ctx.globals().get("__runts_stderr__")?;
                let _: rquickjs::Value = host.call((msg,))?;
                Ok(())
            },
        )?;
        obj.set("write", write)?;
        Ok(obj)
    })?;
    hooks.set("useStderr", f)
}

fn add_window_size_hook<'js>(ctx: &Ctx<'js>, hooks: &Object<'js>) -> JsResult<()> {
    let f = Function::new(ctx.clone(), |ctx: Ctx<'js>| -> JsResult<Object<'js>> {
        let obj = Object::new(ctx.clone())?;
        let cols: i32 = ctx.globals().get("__runts_cols").unwrap_or(80);
        let rows: i32 = ctx.globals().get("__runts_rows").unwrap_or(24);
        obj.set("width", cols)?;
        obj.set("height", rows)?;
        Ok(obj)
    })?;
    hooks.set("useWindowSize", f)
}

fn add_focus_hook<'js>(ctx: &Ctx<'js>, hooks: &Object<'js>) -> JsResult<()> {
    let f = Function::new(ctx.clone(), |ctx: Ctx<'js>| -> JsResult<Object<'js>> {
        let obj = Object::new(ctx.clone())?;
        obj.set("isFocused", true)?;
        Ok(obj)
    })?;
    hooks.set("useFocus", f)
}

fn add_focus_manager_hook<'js>(ctx: &Ctx<'js>, hooks: &Object<'js>) -> JsResult<()> {
    let f = Function::new(ctx.clone(), |ctx: Ctx<'js>| -> JsResult<Object<'js>> {
        let obj = Object::new(ctx.clone())?;
        let noop = Function::new(ctx.clone(), |_ctx: Ctx<'js>| -> JsResult<()> {
            Ok(())
        })?;
        obj.set("focusNext", noop.clone())?;
        obj.set("focusPrevious", noop.clone())?;
        obj.set("focusNextViaTab", noop)?;
        Ok(obj)
    })?;
    hooks.set("useFocusManager", f)
}

fn add_cursor_hook<'js>(ctx: &Ctx<'js>, hooks: &Object<'js>) -> JsResult<()> {
    let f = Function::new(ctx.clone(), |ctx: Ctx<'js>| -> JsResult<Object<'js>> {
        let obj = Object::new(ctx.clone())?;
        let noop = Function::new(ctx.clone(), |_ctx: Ctx<'js>| -> JsResult<()> {
            Ok(())
        })?;
        obj.set("hideCursor", noop.clone())?;
        obj.set("showCursor", noop.clone())?;
        obj.set("isCursorVisible", true)?;
        Ok(obj)
    })?;
    hooks.set("useCursor", f)
}

fn add_animation_hook<'js>(ctx: &Ctx<'js>, hooks: &Object<'js>) -> JsResult<()> {
    let f = Function::new(ctx.clone(), |ctx: Ctx<'js>| -> JsResult<Object<'js>> {
        let obj = Object::new(ctx.clone())?;
        let noop = Function::new(ctx.clone(), |_ctx: Ctx<'js>| -> JsResult<()> {
            Ok(())
        })?;
        obj.set("start", noop.clone())?;
        obj.set("stop", noop)?;
        Ok(obj)
    })?;
    hooks.set("useAnimation", f)
}
