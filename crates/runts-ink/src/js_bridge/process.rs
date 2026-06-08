//! Node.js `process` global polyfill for rquickjs.

use rquickjs::{Ctx, Function, Object, Result as JsResult};

/// Install the `process` global into the given rquickjs Context.
pub fn install(ctx: &Ctx<'_>) -> JsResult<()> {
    let process = Object::new(ctx.clone())?;
    install_exit(ctx, &process)?;
    install_platform_info(&process)?;
    install_env(ctx, &process)?;
    install_stdin(&process)?;
    install_stdout(ctx, &process)?;
    install_stderr(ctx, &process)?;
    install_event_handlers(ctx, &process)?;
    ctx.globals().set("process", process)
}

fn install_exit<'js>(ctx: &Ctx<'js>, process: &Object<'js>) -> JsResult<()> {
    let exit_fn = Function::new(ctx.clone(), |ctx: Ctx<'js>, code: i32| -> JsResult<()> {
        ctx.globals().set("__runts_exit", true)?;
        ctx.globals().set("__runts_exit_code", code)
    })?;
    process.set("exit", exit_fn)
}

fn install_platform_info(process: &Object<'_>) -> JsResult<()> {
    process.set("version", "v20.0.0")?;
    #[cfg(target_os = "macos")]
    process.set("platform", "darwin")?;
    #[cfg(target_os = "linux")]
    process.set("platform", "linux")?;
    #[cfg(target_os = "windows")]
    process.set("platform", "win32")?;
    #[cfg(target_arch = "x86_64")]
    process.set("arch", "x64")?;
    #[cfg(target_arch = "aarch64")]
    process.set("arch", "arm64")?;
    Ok(())
}

fn install_env<'js>(ctx: &Ctx<'js>, process: &Object<'js>) -> JsResult<()> {
    let env = Object::new(ctx.clone())?;
    env.set("NODE_ENV", "development")?;
    env.set("TERM", "xterm-256color")?;
    process.set("env", env)
}

fn install_stdin(process: &Object<'_>) -> JsResult<()> {
    let stdin = Object::new(process.ctx().clone())?;
    stdin.set("isTTY", true)?;
    process.set("stdin", stdin)
}

fn install_stdout<'js>(ctx: &Ctx<'js>, process: &Object<'js>) -> JsResult<()> {
    let cols: i32 = ctx.globals().get("__runts_cols").unwrap_or(80);
    let rows: i32 = ctx.globals().get("__runts_rows").unwrap_or(24);
    let stdout = Object::new(ctx.clone())?;
    stdout.set("rows", rows)?;
    stdout.set("columns", cols)?;
    let write_out = Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>, msg: String| -> JsResult<()> {
            let host: Function = ctx.globals().get("__runts_stdout__")?;
            let _: rquickjs::Value = host.call((msg,))?;
            Ok(())
        },
    )?;
    stdout.set("write", write_out)?;
    process.set("stdout", stdout)
}

fn install_stderr<'js>(ctx: &Ctx<'js>, process: &Object<'js>) -> JsResult<()> {
    let cols: i32 = ctx.globals().get("__runts_cols").unwrap_or(80);
    let rows: i32 = ctx.globals().get("__runts_rows").unwrap_or(24);
    let stderr = Object::new(ctx.clone())?;
    stderr.set("rows", rows)?;
    stderr.set("columns", cols)?;
    let write_err = Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>, msg: String| -> JsResult<()> {
            let host: Function = ctx.globals().get("__runts_stderr__")?;
            let _: rquickjs::Value = host.call((msg,))?;
            Ok(())
        },
    )?;
    stderr.set("write", write_err)?;
    process.set("stderr", stderr)
}

fn install_event_handlers<'js>(ctx: &Ctx<'js>, process: &Object<'js>) -> JsResult<()> {
    let on_fn = Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>, event: String, callback: Function<'js>| -> JsResult<()> {
            let globals = ctx.globals();
            let handlers: Object = globals
                .get("__runts_event_handlers")
                .unwrap_or_else(|_| Object::new(ctx.clone()).unwrap());
            let arr: Object = handlers
                .get(&*event)
                .unwrap_or_else(|_| Object::new(ctx.clone()).unwrap());
            arr.set(arr.len().to_string(), callback)?;
            handlers.set(&*event, arr)?;
            globals.set("__runts_event_handlers", handlers)
        },
    )?;
    process.set("on", on_fn)
}
