//! Ink / Ratatui rquickjs dev path
//!
//! `runts dev --plugin ratatui` transpiles TSX to JS,
//! evaluates it in rquickjs, and either prints one frame
//! (`--once`) or runs an interactive TUI loop.

use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use runts_ink::InputEvent;
use std::io::Stdout;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Render a single frame and return it as a string.
pub fn render_ink_once(project_root: &Path) -> Result<String> {
    let app_tsx = find_app_tsx(project_root)?;
    let js = crate::transpile::bundler::transpile_to_js_bundled(&app_tsx)
        .with_context(|| format!("Failed to transpile {}", app_tsx.display()))?;
    // Debug: print first 200 lines of generated JS
    eprintln!("=== Transpiled JS ({} lines) ===", js.lines().count());
    for (i, line) in js.lines().take(200).enumerate() {
        eprintln!("{:3}: {}", i + 1, line);
    }
    eprintln!("...");
    eval_ink_bundle_and_render(&js)
}

/// Run an interactive TUI loop that routes crossterm events
/// to JS callbacks and re-renders each frame.
pub fn run_ink_interactive(project_root: &Path) -> Result<()> {
    let app_tsx = find_app_tsx(project_root)?;
    let js = crate::transpile::bundler::transpile_to_js_bundled(&app_tsx)?;
    run_interactive_loop(&js)
}

fn find_app_tsx(project_root: &Path) -> Result<PathBuf> {
    let candidates = [
        project_root.join("tui").join("app.tsx"),
        project_root.join("app.tsx"),
        project_root.join("main.tsx"),
    ];
    for c in &candidates {
        if c.exists() {
            return Ok(c.clone());
        }
    }
    for entry in walkdir::WalkDir::new(project_root)
        .max_depth(2)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext == "tsx" {
                return Ok(path.to_path_buf());
            }
        }
    }
    anyhow::bail!("No .tsx file found in {}", project_root.display())
}

fn eval_ink_bundle_and_render(js: &str) -> Result<String> {
    let runtime = rquickjs::Runtime::new()
        .map_err(|e| anyhow::anyhow!("Failed to create runtime: {:?}", e))?;
    let ctx = rquickjs::Context::full(&runtime)
        .map_err(|e| anyhow::anyhow!("Failed to create context: {:?}", e))?;

    let rendered = ctx
        .with(|ctx| eval_bundle_in_ctx(&ctx, js))
        .map_err(|e| anyhow::anyhow!("QuickJS error: {:?}", e))?;

    Ok(rendered)
}

fn eval_bundle_in_ctx(ctx: &rquickjs::Ctx, js: &str) -> anyhow::Result<String> {
    setup_ink_ctx(ctx)?;
    if let Err(e) = ctx.eval::<rquickjs::Value, _>(js) {
        let msg = extract_js_error(&ctx, &e);
        anyhow::bail!("Bundle eval failed: {}", msg);
    }
    let render_js = "runts_ink.render_to_string(__runts_render_with_effects({}));";
    eval_render_js(ctx, render_js)
}

fn extract_js_error(ctx: &rquickjs::Ctx, e: &rquickjs::Error) -> String {
    let base = format!("{:?}", e);
    if matches!(e, rquickjs::Error::Exception) {
        let caught: rquickjs::Value = ctx.catch();
        let msg = caught.as_string().map(|s| s.to_string().unwrap_or_default()).unwrap_or_else(|| {
            caught.as_object().and_then(|o| {
                o.get::<&str, rquickjs::String>("message").ok().and_then(|s| s.to_string().ok())
            }).unwrap_or_else(|| format!("{:?}", caught.type_of()))
        });
        return format!("{} - {}", base, msg);
    }
    base
}

fn eval_render_js(ctx: &rquickjs::Ctx, render_js: &str) -> anyhow::Result<String> {
    match ctx.eval::<rquickjs::Value, _>(render_js) {
        Ok(v) => v.get().map_err(|e| anyhow::anyhow!("Get output failed: {:?}", e)),
        Err(rquickjs::Error::Exception) => {
            let caught = ctx.catch();
            anyhow::bail!("Render exception: {:?}", caught)
        }
        Err(e) => anyhow::bail!("Render failed: {:?}", e),
    }
}

fn setup_ink_ctx(ctx: &rquickjs::Ctx) -> anyhow::Result<()> {
    let globals = ctx.globals();
    let print_fn =
        rquickjs::Function::new(ctx.clone(), |msg: String| {
            eprint!("{}", msg);
        })
        .map_err(|e| anyhow::anyhow!("Failed to create print fn: {:?}", e))?;
    globals
        .set("__runts_stderr__", print_fn)
        .map_err(|e| anyhow::anyhow!("Failed to set __runts_stderr__: {:?}", e))?;
    runts_ink::js_bridge::install(ctx)
        .map_err(|e| anyhow::anyhow!("Failed to install ink bridge: {:?}", e))?;
    Ok(())
}

fn run_interactive_loop(js: &str) -> Result<()> {
    let runtime = rquickjs::Runtime::new()
        .map_err(|e| anyhow::anyhow!("Failed to create runtime: {:?}", e))?;
    let ctx = rquickjs::Context::full(&runtime)
        .map_err(|e| anyhow::anyhow!("Failed to create context: {:?}", e))?;

    init_js_context(&ctx, js)?;
    let mut terminal = setup_terminal()?;

    while should_keep_running(&ctx, &mut terminal)? {}

    teardown_terminal(&mut terminal);
    Ok(())
}

fn should_keep_running(
    ctx: &rquickjs::Context,
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
) -> Result<bool> {
    render_single_frame(ctx, terminal)?;
    let tick = Duration::from_millis(50);
    let mut running = true;
    if crossterm::event::poll(tick)? {
        running = handle_single_event(ctx, terminal)?;
    }
    if running && check_exit_requested(ctx)? {
        running = false;
    }
    Ok(running)
}

fn init_js_context(ctx: &rquickjs::Context, js: &str) -> Result<()> {
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    ctx.with(|ctx| {
        setup_ink_ctx(&ctx)?;
        ctx.globals().set("__runts_cols", i32::from(cols))?;
        ctx.globals().set("__runts_rows", i32::from(rows))?;
        ctx.eval::<rquickjs::Value, _>(js)
            .map_err(|e| anyhow::anyhow!("Bundle eval failed: {:?}", e))?;
        Ok::<_, anyhow::Error>(())
    })?;
    Ok(())
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    let mut stdout = std::io::stdout();
    crossterm::terminal::enable_raw_mode().context("enable raw mode")?;
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )
    .context("enter alternate screen")?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    Terminal::new(backend).context("create terminal")
}

fn teardown_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) {
    crossterm::terminal::disable_raw_mode().ok();
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )
    .ok();
    terminal.show_cursor().ok();
}

fn render_single_frame(
    ctx: &rquickjs::Context,
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
) -> Result<()> {
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let area = ratatui::layout::Rect::new(0, 0, cols, rows);

    let vnode = ctx.with(|ctx| -> anyhow::Result<runts_ink::VNode> {
        let val = ctx
            .eval("__runts_render_with_effects({})")
            .map_err(|e| anyhow::anyhow!("Render eval failed: {:?}", e))?;
        Ok(runts_ink::js_bridge::vnode_from_js(&ctx, &val)?)
    })?;

    runts_ink::draw_vnode(&vnode, terminal, area).context("draw vnode")?;
    Ok(())
}

fn handle_single_event(
    ctx: &rquickjs::Context,
    _terminal: &Terminal<CrosstermBackend<Stdout>>,
) -> Result<bool> {
    match crossterm::event::read()? {
        crossterm::event::Event::Key(key) if key.kind == KeyEventKind::Press => {
            route_key_event(ctx, &key)?;
            let quit = key.code == KeyCode::Char('q')
                && !key.modifiers.contains(KeyModifiers::CONTROL);
            Ok(!quit)
        }
        crossterm::event::Event::Resize(w, h) => {
            ctx.with(|ctx| {
                ctx.globals().set("__runts_cols", i32::from(w))?;
                ctx.globals().set("__runts_rows", i32::from(h))?;
                Ok::<_, rquickjs::Error>(())
            })?;
            Ok(true)
        }
        _ => Ok(true),
    }
}

fn check_exit_requested(ctx: &rquickjs::Context) -> Result<bool> {
    let exit: bool = ctx.with(|ctx| -> anyhow::Result<bool> {
        let val: rquickjs::Value = ctx
            .globals()
            .get("__runts_exit")
            .map_err(|e| anyhow::anyhow!("Failed to get __runts_exit: {:?}", e))?;
        Ok(val.as_bool().unwrap_or(false))
    })?;
    Ok(exit)
}

fn route_key_event(ctx: &rquickjs::Context, key: &KeyEvent) -> Result<()> {
    let ev = InputEvent::from_crossterm(key.clone());
    let js = build_input_js(&ev);
    ctx.with(|ctx| {
        ctx.eval::<rquickjs::Value, _>(js.as_str())
            .map_err(|e| anyhow::anyhow!("Input routing failed: {:?}", e))?;
        Ok::<_, anyhow::Error>(())
    })?;
    Ok(())
}

fn build_input_js(ev: &InputEvent) -> String {
    format!(
        r#"
        (function() {{
            var key = {{
                upArrow: {}, downArrow: {}, leftArrow: {}, rightArrow: {},
                pageUp: {}, pageDown: {}, home: {}, end: {},
                return: {}, escape: {}, backspace: {}, delete: {}, tab: {},
                ctrl: {}, shift: {}, meta: {}
            }};
            var handlers = __runts_input_handlers || [];
            for (var i = 0; i < handlers.length; i++) {{
                handlers[i]('{}', key);
            }}
        }})();
        "#,
        ev.key.up_arrow,
        ev.key.down_arrow,
        ev.key.left_arrow,
        ev.key.right_arrow,
        ev.key.page_up,
        ev.key.page_down,
        ev.key.home,
        ev.key.end,
        ev.key.return_key,
        ev.key.escape,
        ev.key.backspace,
        ev.key.delete,
        ev.key.tab,
        ev.key.ctrl,
        ev.key.shift,
        ev.key.meta,
        escape_js_string(&ev.input)
    )
}

fn escape_js_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}
