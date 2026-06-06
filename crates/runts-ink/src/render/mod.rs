//! The `render()` entry point — boots the Yoga tree,
//! runs the JS reconciler in rquickjs, polls crossterm
//! events, and draws the result to Ratatui each frame.
//!
//!
//! Architecture:
//!
//! 1. **Setup** — `enable_raw_mode`, alternate screen,
//!    build a Yoga tree, create a Terminal.
//! 2. **Reconciler** — the user's `.tsx` was compiled by
//!    runts to a Rust binary that calls
//!    `runts_ink::render(root_fn, props, options)`.
//!    `root_fn` is a `Box<dyn FnMut(Props) -> VNode>` —
//!    in compiled output it calls into rquickjs to
//!    invoke the user's React component, which the JS
//!    reconciler turns into tree ops.
//! 3. **Loop** — every tick, call `root_fn` to get the
//!    current VNode, build the Yoga tree, compute
//!    layout, draw to Ratatui, poll for key events, and
//!    route them back to the JS `useInput` handlers.
//! 4. **Teardown** — when `unmount` is called, restore
//!    the terminal and drop the runtime.
//!
//! The runtime path is exercised by users who write
//! their TUI app in pure Rust without going through
//! `.tsx`. The plugin path (`runts-ratatui` emits
//! Ratatui code directly from HIR) is separate and
//! doesn't touch this render loop.

use std::io::{self, Stdout};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
// Layout computed by flex_layout (yoga-based)
use crate::events::{InputEvent, ResizeEvent, WindowSize};
use crate::props::Props;
use crate::vnode::VNode;

/// Options that control how `render` mounts the TUI.
///
/// In Ink these are spread onto the third argument to
/// `render()`. The fields mirror Ink's `render` options.
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Patch `console.log` etc. to print through Ratatui.
    /// Implemented at the JS reconciler level — Rust side
    /// just records the flag and forwards it to the
    /// reconciler setup.
    pub patch_console: bool,
    /// If true, exit the render loop on `q` (no Ctrl).
    /// Useful for tests that want a clean way to drain
    /// the event loop without Ctrl-C.
    pub exit_on_q: bool,
    /// If true, map `Ctrl-C` to `exit()` instead of the
    /// terminal's default (which is to leave the
    /// alternate screen).
    pub exit_on_ctrl_c: bool,
    /// Tick interval in milliseconds. The render loop
    /// re-draws at this rate. Default 100ms.
    pub tick_ms: u64,
    /// If true, switch to the alternate screen. Default
    /// true.
    pub alternate_screen: bool,
    /// Maximum frame rate. The render loop sleeps
    /// between frames to cap at this FPS. Default 60.
    pub max_fps: u32,
    /// If true, only redraw changed lines. Currently
    /// not implemented — the renderer always draws the
    /// full frame.
    pub incremental_rendering: bool,
    /// Terminal columns for rendering. Default 80.
    pub columns: u16,
    /// Terminal rows for rendering. Default 24.
    pub rows: u16,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            patch_console: false,
            exit_on_q: false,
            exit_on_ctrl_c: true,
            tick_ms: 100,
            alternate_screen: true,
            max_fps: 60,
            incremental_rendering: false,
            columns: 80,
            rows: 24,
        }
    }
}

impl RenderOptions {
    /// Default options: TTY stdout, raw mode, 100ms tick.
    pub fn new() -> Self {
        Self::default()
    }
}

/// The handle returned by `render`.
///
/// Drop the handle to unmount the TUI. The handle also
/// exposes `unmount` for explicit teardown and
/// `drain_input_log` for tests.
pub struct Instance {
    inner: Arc<Mutex<InstanceInner>>,
    terminal: Option<Terminal<CrosstermBackend<Stdout>>>,
}

pub(crate) struct InstanceInner {
    running: bool,
    last_root: Option<VNode>,
    last_size: WindowSize,
    input_log: Vec<InputEvent>,
    paste_log: Vec<String>,
    resize_log: Vec<ResizeEvent>,
}

impl Instance {
    /// Returns `true` while the render loop is still
    /// running.
    pub fn is_running(&self) -> bool {
        self.inner.lock().unwrap().running
    }

    /// Take the input event log. Each call drains the
    /// log.
    pub fn drain_input_log(&self) -> Vec<InputEvent> {
        std::mem::take(&mut self.inner.lock().unwrap().input_log)
    }

    /// Take the paste event log.
    pub fn drain_paste_log(&self) -> Vec<String> {
        std::mem::take(&mut self.inner.lock().unwrap().paste_log)
    }

    /// Take the resize event log.
    pub fn drain_resize_log(&self) -> Vec<ResizeEvent> {
        std::mem::take(&mut self.inner.lock().unwrap().resize_log)
    }

    /// Returns the most recent VNode tree the renderer
    /// drew.
    pub fn last_root(&self) -> Option<VNode> {
        self.inner.lock().unwrap().last_root.clone()
    }

    /// Returns the last known window size.
    pub fn last_size(&self) -> WindowSize {
        self.inner.lock().unwrap().last_size
    }

    /// Re-render the current root. The user calls this
    /// from their main loop after mutating state.
    /// The `RootFn` is invoked to get a fresh
    /// `VNode`, Yoga recomputes the layout, and the
    /// new tree is drawn to the terminal.
    pub fn redraw(&mut self, root: &mut RootFn) -> Result<()> {
        let size = self.inner.lock().unwrap().last_size;
        let area = Rect {
            x: 0,
            y: 0,
            width: size.columns,
            height: size.rows,
        };
        let tree = root(Props::new());
        let mut layout = Layout::new();
        let yoga = YogaTree::from_vnode(&tree, &mut layout);
        yoga.compute(
            &mut layout,
            Size {
                width: AvailableSpace::Definite(area.width as f32),
                height: AvailableSpace::Definite(area.height as f32),
            },
        );
        self.inner.lock().unwrap().last_root = Some(tree.clone());
        if let Some(term) = self.terminal.as_mut() {
            term.draw(|frame| tree::render_tree(&tree, &layout, frame, area))
                .context("redraw")?;
        }
        Ok(())
    }

    /// Tear down the TUI. Idempotent.
    pub fn unmount(&mut self) -> Result<()> {
        self.inner.lock().unwrap().running = false;
        if let Some(mut term) = self.terminal.take() {
            disable_raw_mode().ok();
            execute!(term.backend_mut(), LeaveAlternateScreen, DisableMouseCapture).ok();
            term.show_cursor().ok();
        }
        Ok(())
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        let _ = self.unmount();
    }
}

/// A function that produces the root `VNode` given the
/// current `Props`.
///
/// In Ink, the user passes a JSX element directly. In
/// Rust, the cleanest equivalent is a `fn(Props) -> VNode`
/// (or closure with the same signature). The render
/// pipeline doesn't need to move this closure across
/// threads (the v0.1 renderer is single-threaded), so
/// the trait bound is `FnMut(Props) -> VNode` without
/// `Send`. If a future revision spawns the render
/// loop on a worker thread, this alias is the place
/// to add the `Send` bound back.
pub type RootFn = std::boxed::Box<dyn FnMut(Props) -> VNode>;

/// Mount a root component and return an `Instance`.
///
/// Boots the Yoga tree, runs the render + input loop in
/// a background thread, and returns a handle the caller
/// can use to inspect or tear down the session.
///
/// This is the canonical Ink-style `render` entry point.
/// In the plugin path (`runts build --plugin ratatui`),
/// the generated binary calls `render` itself, then
/// drops the `Instance` to keep the terminal open until
/// the user quits.
///
/// The caller passes `&mut root` rather than `root` so
/// they can call `instance.redraw(&mut root)` later to
/// re-render after mutating state. The renderer never
/// retains its own copy of the closure.
pub fn render(
    root: &mut RootFn,
    initial_props: Props,
    options: RenderOptions,
) -> Result<Instance> {
    // 1. Query the terminal size before we switch to the
    // alternate screen — the size query itself works
    // either way but doing it first lets us report a
    // sensible `last_size` on the first frame.
    let initial_size = crossterm::terminal::size().unwrap_or((80, 24));
    let area = Rect {
        x: 0,
        y: 0,
        width: initial_size.0,
        height: initial_size.1,
    };

    // 2. Run the root once to get the initial tree.
    let initial_tree = root(initial_props);

    // 3. Build a Yoga tree from the VNode tree. Yoga
    // computes layout in `compute_layout`. We keep a
    // mapping from VNode index -> Yoga node id so the
    // renderer can look up the computed rect for each
    // node.
    let mut layout = Layout::new();
    let tree = YogaTree::from_vnode(&initial_tree, &mut layout);

    // 4. Compute the initial layout.
    tree.compute(
        &mut layout,
        Size {
            width: AvailableSpace::Definite(area.width as f32),
            height: AvailableSpace::Definite(area.height as f32),
        },
    );

    // 5. Set up the terminal. We always set up the
    // alternate screen and raw mode — that's the only
    // way Ratatui can draw.
    let mut stdout = io::stdout();
    enable_raw_mode().context("enable raw mode")?;
    if options.alternate_screen {
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
            .context("enter alternate screen")?;
    }
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("create terminal")?;

    // 6. Render the initial tree.
    terminal
        .draw(|frame| tree::render_tree(&initial_tree, &layout, frame, area))
        .context("initial draw")?;

    // 7. Build the Instance state.
    let inner = Arc::new(Mutex::new(InstanceInner {
        running: true,
        last_root: Some(initial_tree),
        last_size: WindowSize {
            columns: initial_size.0,
            rows: initial_size.1,
        },
        input_log: Vec::new(),
        paste_log: Vec::new(),
        resize_log: Vec::new(),
    }));

    // 8. Run the input loop in a background thread.
    // The render loop calls `root` to get the current
    // tree; the input loop polls crossterm for events.
    // Both threads share the Instance's `inner` mutex.
    let loop_inner = inner.clone();
    let tick = Duration::from_millis(options.tick_ms.max(1));
    let exit_on_q = options.exit_on_q;
    let exit_on_ctrl_c = options.exit_on_ctrl_c;
    std::thread::spawn(move || input::input_loop(loop_inner, tick, exit_on_q, exit_on_ctrl_c));

    Ok(Instance {
        inner,
        terminal: Some(terminal),
    })
}

/// Render a VNode tree to a Ratatui `Frame`.
///
/// The function walks the VNode tree, looks up each node
/// in the Yoga `Layout` to get its computed rectangle,
/// and draws the corresponding Ratatui widget.
/// Draw a `VNode` tree to any `ratatui::Backend`.
///
/// Builds the Yoga tree, computes layout, and draws the
/// result to the supplied terminal in a single frame.
pub fn draw_vnode<B: ratatui::backend::Backend>(
    vnode: &VNode,
    terminal: &mut Terminal<B>,
    area: Rect,
) -> Result<()> {
    let mut layout = Layout::new();
    let tree = YogaTree::from_vnode(vnode, &mut layout);
    tree.compute(
        &mut layout,
        Size {
            width: AvailableSpace::Definite(area.width as f32),
            height: AvailableSpace::Definite(area.height as f32),
        },
    );
    terminal
        .draw(|frame| tree::render_tree(vnode, &layout, frame, area))
        .context("draw vnode")?;
    Ok(())
}

pub fn render_to_string(root: VNode, options: RenderOptions) -> Result<String> {
    // Use terminal size from options, default to 80x24
    let cols = options.columns.max(1);
    let rows = options.rows.max(1);
    let backend = ratatui::backend::TestBackend::new(cols, rows);
    let mut terminal = Terminal::new(backend).context("create test terminal")?;
    let mut layout = Layout::new();
    let tree = YogaTree::from_vnode(&root, &mut layout);
    let area = Rect { x: 0, y: 0, width: cols, height: rows };
    tree.compute(
        &mut layout,
        Size {
            width: AvailableSpace::Definite(cols as f32),
            height: AvailableSpace::Definite(rows as f32),
        },
    );
    terminal
        .draw(|frame| tree::render_tree(&root, &layout, frame, area))
        .context("draw")?;
    eprintln!("DEBUG layout.rects len={} rects={:?}", layout.rects.len(), layout.rects);
    let buffer = terminal.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..rows {
        let mut line = String::new();
        for x in 0..cols {
            line.push_str(buffer.get(x, y).symbol());
        }
        let trimmed = line.trim_end();
        // Skip empty lines at the end
        if trimmed.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(trimmed);
    }
    Ok(out)
    }

pub mod color;
pub mod input;
pub mod layout;
pub mod measure;
pub mod tree;
pub use color::{color_to_ratatui, parse_hex_color};
pub use layout::{AvailableSpace, Layout, Size, YogaTree};
pub use tree::render_tree;

#[cfg(test)]
mod tests {
    include!("tests.inc");
}
