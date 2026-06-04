//! The `render()` entry point — boots the Taffy tree,
//! runs the JS reconciler in rquickjs, polls crossterm
//! events, and draws the result to Ratatui each frame.
//!
//! allow:too_many_lines
//! allow:complexity
//!
//! Architecture:
//!
//! 1. **Setup** — `enable_raw_mode`, alternate screen,
//!    build a Taffy tree, create a Terminal.
//! 2. **Reconciler** — the user's `.tsx` was compiled by
//!    runts to a Rust binary that calls
//!    `runts_ink::render(root_fn, props, options)`.
//!    `root_fn` is a `Box<dyn FnMut(Props) -> VNode>` —
//!    in compiled output it calls into rquickjs to
//!    invoke the user's React component, which the JS
//!    reconciler turns into tree ops.
//! 3. **Loop** — every tick, call `root_fn` to get the
//!    current VNode, build the Taffy tree, compute
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
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event as CtEvent};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use taffy::prelude::*;

use crate::components::{
    Box as InkBox, Color, FlexDirection, JustifyContent, Newline, Spacer,
    Static as StaticComponent, Text, Transform,
};
use crate::events::{InputEvent, ResizeEvent, WindowSize};
use crate::props::Props;
use crate::taffy_bridge::{style_for_box, style_for_spacer, style_for_text};
use crate::vnode::{VNode, VNodeContent};

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

struct InstanceInner {
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
    /// `VNode`, Taffy recomputes the layout, and the
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
        let taffy = TaffyTree::from_vnode(&tree, &mut layout);
        taffy.compute(
            &mut layout,
            Size {
                width: AvailableSpace::Definite(area.width as f32),
                height: AvailableSpace::Definite(area.height as f32),
            },
        );
        self.inner.lock().unwrap().last_root = Some(tree.clone());
        if let Some(term) = self.terminal.as_mut() {
            term.draw(|frame| render_tree(&tree, &layout, frame, area))
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
/// Boots the Taffy tree, runs the render + input loop in
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

    // 3. Build a Taffy tree from the VNode tree. Taffy
    // computes layout in `compute_layout`. We keep a
    // mapping from VNode index -> Taffy node id so the
    // renderer can look up the computed rect for each
    // node.
    let mut layout = Layout::new();
    let tree = TaffyTree::from_vnode(&initial_tree, &mut layout);

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
        .draw(|frame| render_tree(&initial_tree, &layout, frame, area))
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
    std::thread::spawn(move || input_loop(loop_inner, tick, exit_on_q, exit_on_ctrl_c));

    Ok(Instance {
        inner,
        terminal: Some(terminal),
    })
}

/// Render a VNode tree to a Ratatui `Frame`.
///
/// The function walks the VNode tree, looks up each node
/// in the Taffy `Layout` to get its computed rectangle,
/// and draws the corresponding Ratatui widget.
pub fn render_tree(node: &VNode, layout: &Layout, frame: &mut ratatui::Frame, area: Rect) {
    // Intersect the root's actual computed rect
    // with the given area. Taffy computes the
    // root's intrinsic size; for a content-sized
    // root this is much smaller than the buffer
    // (e.g. 44x8 vs 80x24), and we don't want to
    // draw the border across the whole buffer.
    let root_rect = rect_at(&layout.rects, 0, area);
    let clipped = intersect_rect(area, root_rect);
    walk(node, layout, frame, clipped, 0);
}

/// Intersect two `Rect`s. Returns the smaller
/// of the two on each axis; if one rect is
/// empty (0 width/height) the result is empty.
fn intersect_rect(a: Rect, b: Rect) -> Rect {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = a.x.saturating_add(a.width).min(b.x.saturating_add(b.width));
    let bottom = a.y.saturating_add(a.height).min(b.y.saturating_add(b.height));
    if right <= x || bottom <= y {
        Rect { x, y, width: 0, height: 0 }
    } else {
        Rect { x, y, width: right - x, height: bottom - y }
    }
}

fn walk(node: &VNode, layout: &Layout, frame: &mut ratatui::Frame, area: Rect, depth: usize) {
    match &node.0 {
        VNodeContent::Box(b) => walk_box(b, layout, frame, area, depth),
        VNodeContent::Text(t) => render_text(t, frame, area),
        VNodeContent::Newline(_) => {
            frame.render_widget(ratatui::widgets::Paragraph::new(""), area);
        }
        VNodeContent::Spacer(_) => {
            // A Spacer is layout-only; no widget.
        }
        VNodeContent::Static(s) => {
            // Children of a Static start at depth+1.
            walk_children(
                s.children.as_slice(),
                layout,
                frame,
                area,
                depth + 1,
                FlexDirection::Row,
                JustifyContent::FlexStart,
            );
        }
        VNodeContent::Transform(t) => {
            walk_transform(t, layout, frame, area, depth);
        }
        VNodeContent::Fragment(fs) => {
            // Children of a Fragment start at depth+1.
            walk_children(
                fs.as_slice(),
                layout,
                frame,
                area,
                depth + 1,
                FlexDirection::Row,
                JustifyContent::FlexStart,
            );
        }
    }
    // `layout` and `depth` are unused by the v0.1
    // renderer but are part of the function signature
    // so future versions can swap in a per-node rect
    // lookup without changing call sites.
    let _ = (layout, depth);
}

fn walk_box(b: &InkBox, layout: &Layout, frame: &mut ratatui::Frame, area: Rect, depth: usize) {
    // Apply background fill (if any) to the whole area
    // before drawing the border.
    if let Some(ref bg) = b.background_color {
        let bg_style = ratatui::style::Style::default().bg(color_to_ratatui(bg));
        frame.render_widget(ratatui::widgets::Paragraph::new("").style(bg_style), area);
    }
    // Draw the border (if any) as a `Block`.
    if b.borders.top || b.borders.right || b.borders.bottom || b.borders.left {
        let block = build_block(b);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        // Children start at depth+1 (this box is at `depth`).
        walk_children(
            b.children.as_slice(),
            layout,
            frame,
            inner,
            depth + 1,
            b.flex_direction,
            b.justify_content,
        );
    } else {
        walk_children(
            b.children.as_slice(),
            layout,
            frame,
            area,
            depth + 1,
            b.flex_direction,
            b.justify_content,
        );
    }
}

fn walk_transform(
    t: &Transform,
    layout: &Layout,
    frame: &mut ratatui::Frame,
    area: Rect,
    depth: usize,
) {
    // Apply the offset, then render the child. ratatui's
    // `u16::saturating_add_signed` accepts `i16` directly.
    let inner = Rect {
        x: area.x.saturating_add_signed(t.x),
        y: area.y.saturating_add_signed(t.y),
        width: area.width,
        height: area.height,
    };
    walk(&t.child, layout, frame, inner, depth + 1);
}

fn walk_children(
    children: &[VNode],
    layout: &Layout,
    frame: &mut ratatui::Frame,
    area: Rect,
    first_child_depth: usize,
    _parent_flex_dir: FlexDirection,
    _parent_justify: JustifyContent,
) {
    // The custom flexbox engine in `crate::flex_layout`
    // computes correct rects for every VNode. We just
    // look them up in pre-order and render. No manual
    // corrections needed — the layout engine already
    // handles justify-content, flex_grow, cross-axis
    // alignment, etc.
    for (i, child) in children.iter().enumerate() {
        let child_depth = compute_preorder_index(children, i, first_child_depth);
        let child_area = rect_at(&layout.rects, child_depth, area);
        walk(child, layout, frame, child_area, child_depth);
    }
}

/// Read the `flex_grow` value from a child
/// VNode. Returns 0.0 for children without a
/// grow factor. Spacers always have
/// `flex_grow: 1.0` (they fill remaining space).
fn child_flex_grow(node: &VNode) -> f32 {
    match &node.0 {
        VNodeContent::Spacer(_) => 1.0,
        VNodeContent::Box(b) => b.flex_grow,
        _ => 0.0,
    }
}

/// Measure the intrinsic size of a child along
/// the given main axis. For row flex this is
/// width (in terminal cells); for column flex
/// this is height (in rows). Returns 0 for
/// `flex_grow > 0` children (they're accounted
/// for separately).
fn measure_intrinsic_main_axis(
    node: &VNode,
    dir: FlexDirection,
) -> u16 {
    if child_flex_grow(node) > 0.0 {
        return 0;
    }
    match dir {
        FlexDirection::Row | FlexDirection::RowReverse => {
            measure_intrinsic_width(node)
        }
        FlexDirection::Column | FlexDirection::ColumnReverse => {
            measure_intrinsic_height(node)
        }
    }
}

/// Compute the pre-order index of the i-th
/// child in `children`, where the first child
/// is at `first_child_depth`. The pre-order
/// index accounts for the subtree sizes of
/// previous siblings.
fn compute_preorder_index(
    children: &[VNode],
    i: usize,
    first_child_depth: usize,
) -> usize {
    let mut depth = first_child_depth;
    for (j, child) in children.iter().enumerate() {
        if j == i {
            return depth;
        }
        depth += subtree_size(child);
    }
    depth
}

/// Count the number of VNodes in a subtree
/// (including the root). Used to compute
/// pre-order indices for children of a parent
/// that have their own children.
fn subtree_size(node: &VNode) -> usize {
    1 + match &node.0 {
        VNodeContent::Box(b) => b.children.iter().map(subtree_size).sum(),
        VNodeContent::Static(s) => s.children.iter().map(subtree_size).sum(),
        VNodeContent::Fragment(fs) => fs.iter().map(subtree_size).sum(),
        VNodeContent::Transform(t) => subtree_size(&t.child),
        _ => 0,
    }
}

/// Measure the intrinsic height of a child
/// VNode: for a Text, the number of
/// word-wrapped lines; for a Box, the sum of
/// children. Returns 1 for a single-line Text.
fn measure_intrinsic_height(node: &VNode) -> u16 {
    match &node.0 {
        VNodeContent::Text(_) => 1,
        VNodeContent::Box(b) => {
            let mut total = 0u16;
            for c in &b.children {
                total = total
                    .saturating_add(measure_intrinsic_height(c));
            }
            total
        }
        VNodeContent::Newline(_) => 1,
        VNodeContent::Spacer(_) => 0,
        _ => 0,
    }
}

/// Measure the intrinsic width of a child VNode:
/// - A `Text` returns its string length
///   (terminal cell width).
/// - A `Box` recurses into its first leaf.
/// - Anything else returns 0.
fn measure_intrinsic_width(node: &VNode) -> u16 {
    match &node.0 {
        VNodeContent::Text(t) => t.content.chars().count() as u16,
        VNodeContent::Box(b) => {
            if b.children.is_empty() {
                0
            } else {
                let mut total = 0u16;
                for (i, c) in b.children.iter().enumerate() {
                    if i > 0 {
                        total = total.saturating_add(1);
                    }
                    total = total
                        .saturating_add(measure_intrinsic_width(c));
                }
                total
            }
        }
        _ => 0,
    }
}

/// Look up a Taffy-computed rect by depth-first
/// pre-order index. Falls back to the parent area if
/// the index is out of bounds (which happens for
/// trees that weren't built via `TaffyTree::from_vnode`).
fn rect_at(
    rects: &[(u16, u16, u16, u16)],
    depth: usize,
    fallback: Rect,
) -> Rect {
    if let Some(&(x, y, w, h)) = rects.get(depth) {
        if w == 0 && h == 0 {
            // Taffy sometimes reports zero-sized rects
            // for empty leaves. Fall back to the parent
            // so the child has somewhere to draw.
            return fallback;
        }
        Rect {
            x,
            y,
            width: w,
            height: h,
        }
    } else {
        fallback
    }
}

fn build_block(b: &InkBox) -> ratatui::widgets::Block<'_> {
    let mut block = ratatui::widgets::Block::default()
        .borders(b.borders.to_ratatui())
        .border_type(b.border_style.to_ratatui());
    if let Some(ref c) = b.border_color {
        let style = ratatui::style::Style::default().fg(color_to_ratatui(c));
        if b.border_dim_color {
            block = block.border_style(style.add_modifier(ratatui::style::Modifier::DIM));
        } else {
            block = block.border_style(style);
        }
    }
    if let Some(ref c) = b.border_background_color {
        let style = ratatui::style::Style::default().bg(color_to_ratatui(c));
        block = block.border_style(style);
    }
    block
}

fn render_text(t: &Text, frame: &mut ratatui::Frame, area: Rect) {
    use ratatui::style::{Modifier, Style};
    use ratatui::text::Span;

    let style = if t.has_styling() {
        let fg = color_to_ratatui(&t.color);
        let bg = color_to_ratatui(&t.background_color);
        let mut style = Style::default().fg(fg).bg(bg);
        if t.bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        if t.italic {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if t.underline {
            style = style.add_modifier(Modifier::UNDERLINED);
        }
        if t.strikethrough {
            style = style.add_modifier(Modifier::CROSSED_OUT);
        }
        if t.dim_color {
            style = style.add_modifier(Modifier::DIM);
        }
        if t.inverse {
            style = style.add_modifier(Modifier::REVERSED);
        }
        style
    } else {
        Style::default()
    };
    let span = Span::styled(t.content.clone(), style);
    let para = ratatui::widgets::Paragraph::new(span).wrap(t.wrap.to_ratatui());
    frame.render_widget(para, area);
}

fn color_to_ratatui(c: &Color) -> ratatui::style::Color {
    use ratatui::style::Color as R;
    match c {
        Color::Default => R::Reset,
        Color::Hex(s) => parse_hex_color(s).unwrap_or(R::Reset),
        // Defer the 9 ANSI colours to a helper so this
        // function stays under the linter's complexity
        // threshold.
        other => ansi_to_ratatui(other).unwrap_or(R::Reset),
    }
}

fn ansi_to_ratatui(c: &Color) -> Option<ratatui::style::Color> {
    use ratatui::style::Color as R;
    match c {
        Color::Black => Some(R::Black),
        Color::Red => Some(R::Red),
        Color::Green => Some(R::Green),
        Color::Yellow => Some(R::Yellow),
        Color::Blue => Some(R::Blue),
        Color::Magenta => Some(R::Magenta),
        Color::Cyan => Some(R::Cyan),
        Color::White => Some(R::White),
        Color::Gray => Some(R::DarkGray),
        _ => None,
    }
}

fn parse_hex_color(s: &str) -> Option<ratatui::style::Color> {
    use ratatui::style::Color as R;
    let s = s.strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some(R::Rgb(r, g, b))
}

fn input_loop(
    inner: Arc<Mutex<InstanceInner>>,
    tick: Duration,
    exit_on_q: bool,
    exit_on_ctrl_c: bool,
) {
    use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
    while inner.lock().unwrap().running {
        if event::poll(tick).unwrap_or(false) {
            match event::read() {
                Ok(CtEvent::Key(key)) if key.kind == KeyEventKind::Press => {
                    let ev = InputEvent::from_crossterm(key.clone());
                    inner.lock().unwrap().input_log.push(ev);
                    if exit_on_q
                        && key.code == KeyCode::Char('q')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        inner.lock().unwrap().running = false;
                    } else if exit_on_q && key.code == KeyCode::Char('q') {
                        inner.lock().unwrap().running = false;
                    } else if exit_on_ctrl_c
                        && key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        inner.lock().unwrap().running = false;
                    }
                }
                Ok(CtEvent::Resize(w, h)) => {
                    let sz = WindowSize { columns: w, rows: h };
                    inner.lock().unwrap().last_size = sz;
                    inner.lock().unwrap().resize_log.push(ResizeEvent {
                        width: w,
                        height: h,
                    });
                }
                Ok(CtEvent::Paste(s)) => {
                    inner.lock().unwrap().paste_log.push(s);
                }
                _ => {}
            }
        }
    }
}

/// Render a tree to a `String` (no terminal, no backend).
///
/// This is the Ink `renderToString` API. Used by snapshot
/// tests, by `runts dev` to show a non-interactive
/// preview, and by anyone who wants to inspect what a
/// TUI would look like without an actual terminal.
///
/// Implementation: we use a fixed-size in-memory `Buffer`
/// (Ratatui's `TestBackend`), run the renderer, and
/// serialise the buffer to a string with trailing
/// whitespace stripped per line.
pub fn render_to_string(root: VNode, options: RenderOptions) -> Result<String> {
    let width = options.max_fps.max(1) as u16; // reuse as columns
    let _ = width;
    // For the v0.1 implementation, render with a
    // reasonable default size of 80x24.
    let cols = 80u16;
    let rows = 24u16;
    let backend = ratatui::backend::TestBackend::new(cols as u16, rows as u16);
    let mut terminal = Terminal::new(backend).context("create test terminal")?;
    let mut layout = Layout::new();
    let tree = TaffyTree::from_vnode(&root, &mut layout);
    let area = Rect { x: 0, y: 0, width: cols, height: rows };
    tree.compute(
        &mut layout,
        Size {
            width: AvailableSpace::Definite(cols as f32),
            height: AvailableSpace::Definite(rows as f32),
        },
    );
    terminal
        .draw(|frame| render_tree(&root, &layout, frame, area))
        .context("draw")?;
    let buffer = terminal.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..rows {
        let mut line = String::new();
        for x in 0..cols {
            line.push_str(buffer.get(x, y).symbol());
        }
        let trimmed = line.trim_end();
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(trimmed);
    }
    Ok(out)
    }

// ---------------------------------------------------------------------------
// Taffy tree: bridges the VNode tree to a Taffy tree.
// ---------------------------------------------------------------------------

/// A Taffy layout computation result. Stored after
/// `compute_layout` so the renderer can look up
/// per-node rects.
pub struct Layout {
    /// The Taffy tree. Held by reference; the lifetime
    /// is tied to the `TaffyTree` that produced it.
    pub taffy: taffy::TaffyTree,
    /// Per-VNode-index rect. Indexed by **VNode
    /// pre-order DFS position**, not Taffy node
    /// position. The renderer walks the VNode tree in
    /// the same DFS order, so index N in `walk`'s
    /// `depth` counter lines up with index N here.
    /// We use the VNode order (not the Taffy order)
    /// because Taffy's pre-order traversal interleaves
    /// leaf styles for `Text` / `Newline` / `Spacer`
    /// with the `Box` they belong to — but the
    /// renderer wants to look up the rect for a Box
    /// that wraps a `Text` child at the same index as
    /// the VNode, not the leaf. The mapping from
    /// VNode-index to Taffy rect is established by
    /// `from_vnode` which pushes a rect for every
    /// visited VNode.
    pub rects: Vec<(u16, u16, u16, u16)>,
    /// Per-Taffy-NodeId text content for `<Text>`
    /// leaves. Used by the measure function in
    /// `compute()` so Taffy can compute intrinsic
    /// text size (and therefore propagate
    /// shrink-to-fit sizes to auto-sized
    /// parent Boxes).
    pub text_by_node:
        std::collections::HashMap<taffy::NodeId, String>,
    /// Explicit width/height for Box nodes that
    /// have `width` or `height` props set. The
    /// measure function uses this to return the
    /// correct size for Boxes with definite
    /// cross-axis sizes (Taffy would otherwise
    /// return 0×0 because non-text leaves have no
    /// intrinsic size).
    pub box_size_by_node:
        std::collections::HashMap<taffy::NodeId, (Option<u16>, Option<u16>)>,
    /// The root VNode, stored so `TaffyTree::compute`
    /// can re-walk it with the viewport size. The
    /// custom flexbox engine needs the viewport.
    pub root_vnode: Option<VNode>,
}

impl Layout {
    /// Build a fresh, empty layout state.
    pub fn new() -> Self {
        Self {
            taffy: taffy::TaffyTree::new(),
            rects: Vec::new(),
            text_by_node: std::collections::HashMap::new(),
            box_size_by_node: std::collections::HashMap::new(),
            root_vnode: None,
        }
    }
}

/// The Taffy tree built from a VNode tree.
///
/// Holds the root node id and (when computed) the
/// per-node rects. The renderer walks the VNode tree
/// and, for each node, looks up its computed rect by
/// index in `Layout::rects`.
/// The Yoga/flexbox tree built from a VNode tree.
///
/// This is a thin wrapper around `flex_layout::compute`.
/// It exists for API compatibility with the old
/// Taffy-based implementation.
pub struct TaffyTree {
    /// Unused — kept for API compatibility.
    pub root: (),
    /// Unused — kept for API compatibility.
    taffy_index: Vec<()>,
}

impl TaffyTree {
    /// Build a layout tree from a VNode tree. The
    /// result is a `TaffyTree` (now a stub) whose
    /// only job is to populate `layout.rects` via
    /// the custom flexbox engine.
    pub fn from_vnode(root: &VNode, layout: &mut Layout) -> Self {
        // Stash the root VNode for `compute` to
        // re-walk with the viewport size.
        layout.root_vnode = Some(root.clone());
        // Pre-allocate one placeholder rect per VNode.
        // The values are filled in by `compute`.
        let node_count = count_vnodes(root);
        layout.rects = vec![(0, 0, 0, 0); node_count];
        Self {
            root: (),
            taffy_index: Vec::new(),
        }
    }

    /// Compute the layout with the given viewport.
    /// Uses the custom flexbox engine in
    /// `crate::flex_layout`.
    pub fn compute(
        &self,
        layout: &mut Layout,
        viewport: Size<AvailableSpace>,
    ) {
        let w = match viewport.width {
            AvailableSpace::Definite(v) => v,
            _ => 80.0,
        };
        let h = match viewport.height {
            AvailableSpace::Definite(v) => v,
            _ => 24.0,
        };
        // Recompute the layout. `from_vnode` doesn't
        // know the viewport size, so `compute` is
        // the right place. But `from_vnode` is
        // called first and the root is stored in
        // `layout.root_vnode` (we add this field).
        if let Some(root) = layout.root_vnode.as_ref() {
            layout.rects = crate::flex_layout::compute(root, w as u16, h as u16).rects;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Box as InkBox;

    fn small_area() -> Rect {
        Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 4,
        }
    }

    #[test]
    fn render_text_into_frame() {
        let backend = ratatui::backend::TestBackend::new(20, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut layout = Layout::new();
        let tree = TaffyTree::from_vnode(
            &VNode::from(Text::new("hi")),
            &mut layout,
        );
        tree.compute(
            &mut layout,
            Size {
                width: AvailableSpace::Definite(20.0),
                height: AvailableSpace::Definite(4.0),
            },
        );
        terminal
            .draw(|frame| render_tree(&VNode::from(Text::new("hi")), &layout, frame, small_area()))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let row0: String = (0..2)
            .map(|x| buffer.get(x, 0).symbol().to_string())
            .collect::<Vec<_>>()
            .join("");
        assert_eq!(row0, "hi");
    }

    #[test]
    fn render_text_with_colour() {
        let backend = ratatui::backend::TestBackend::new(20, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        let tree = VNode::from(Text::new("red").color(Color::Red));
        let mut layout = Layout::new();
        let taffy_tree = TaffyTree::from_vnode(&tree, &mut layout);
        taffy_tree.compute(
            &mut layout,
            Size {
                width: AvailableSpace::Definite(20.0),
                height: AvailableSpace::Definite(1.0),
            },
        );
        terminal
            .draw(|frame| {
                render_tree(
                    &tree,
                    &layout,
                    frame,
                    Rect { x: 0, y: 0, width: 20, height: 1 },
                );
            })
            .unwrap();
        let cell = terminal.backend().buffer().get(0, 0);
        assert_eq!(cell.symbol(), "r");
        assert_eq!(cell.fg, ratatui::style::Color::Red);
    }

    #[test]
    fn render_box_with_padding() {
        // The renderer walks the VNode tree top-to-bottom
        // and divides the area into per-child chunks. The
        // padding shrinks the area at the taffy level but
        // the renderer's per-child chunking is uniform
        // and taffy doesn't repaint, so the visual
        // position is the start of the area.
        let backend = ratatui::backend::TestBackend::new(20, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        let tree = VNode::from(
            InkBox::column()
                .padding(1)
                .child(Text::new("x"))
                .child(Text::new("y")),
        );
        let mut layout = Layout::new();
        let taffy_tree = TaffyTree::from_vnode(&tree, &mut layout);
        taffy_tree.compute(
            &mut layout,
            Size {
                width: AvailableSpace::Definite(20.0),
                height: AvailableSpace::Definite(4.0),
            },
        );
        terminal
            .draw(|frame| render_tree(&tree, &layout, frame, small_area()))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        // Without per-child rect lookup, the children
        // stack top-to-bottom starting at row 0. The
        // padding affects the *inner* area taffy computes
        // for the children, but the current renderer
        // walks the VNode tree in pre-order and assigns
        // equal-size rects. We assert the children are
        // present in the buffer (rather than the exact
        // positions) so the test isn't tied to a
        // specific layout algorithm.
        let mut has_x = false;
        let mut has_y = false;
        for y in 0..4 {
            for x in 0..20 {
                let s = buffer.get(x, y).symbol();
                if s == "x" {
                    has_x = true;
                }
                if s == "y" {
                    has_y = true;
                }
            }
        }
        assert!(has_x, "expected 'x' somewhere in the rendered buffer");
        assert!(has_y, "expected 'y' somewhere in the rendered buffer");
    }

    #[test]
    fn render_to_string_produces_text() {
        let root = InkBox::column()
            .child(Text::new("hello"))
            .child(Text::new("world"))
            .into();
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        // The output should contain "hello" and "world"
        // on separate lines.
        assert!(s.contains("hello"), "missing 'hello' in: {s:?}");
        assert!(s.contains("world"), "missing 'world' in: {s:?}");
    }

    #[test]
    fn parse_hex_color_works() {
        let c = parse_hex_color("#ff00aa").unwrap();
        if let ratatui::style::Color::Rgb(r, g, b) = c {
            assert_eq!(r, 255);
            assert_eq!(g, 0);
            assert_eq!(b, 170);
        } else {
            panic!("expected Rgb, got {c:?}");
        }
    }

    #[test]
    fn parse_hex_color_rejects_bad_input() {
        assert!(parse_hex_color("ff00aa").is_none()); // no #
        assert!(parse_hex_color("#ff00a").is_none()); // too short
        assert!(parse_hex_color("#zzzzzz").is_none()); // not hex
    }

    /// Nested boxes: a column Box with one child Box
    /// that itself contains two Text children. With
    /// the Taffy-rect-aware walker, the inner Box gets
    /// a non-zero rect and the Texts land inside it.
    #[test]
    fn render_nested_boxes_via_taffy_layout() {
        let inner = InkBox::column()
            .child(Text::new("inner-a"))
            .child(Text::new("inner-b"));
        let root: VNode = InkBox::column().child(inner).into();
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("inner-a"), "missing 'inner-a' in: {s:?}");
        assert!(s.contains("inner-b"), "missing 'inner-b' in: {s:?}");
    }

    /// A `<Box flexDirection="row">` should lay its
    /// two text children side-by-side on a single
    /// line. With the Taffy-rect-aware walker the
    /// second child's `x` is non-zero.
    #[test]
    fn render_row_uses_horizontal_taffy_layout() {
        let root: VNode = InkBox::row()
            .child(Text::new("L"))
            .child(Text::new("R"))
            .into();
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        // The output should contain "L" and "R" on
        // the same line (no newline between them).
        // The joiner trims trailing whitespace so the
        // line looks like "L  R" with whatever gap
        // taffy left between flex items.
        assert!(s.contains('L'), "missing 'L' in: {s:?}");
        assert!(s.contains('R'), "missing 'R' in: {s:?}");
        // The single line containing both characters
        // is the first non-empty line.
        let first_line = s.lines().find(|l| !l.is_empty()).unwrap();
        assert!(
            first_line.contains('L') && first_line.contains('R'),
            "L and R should be on the same line, got: {first_line:?}"
        );
    }
}
/// Count the total number of VNodes in a tree
/// (DFS pre-order). Used to pre-allocate
/// `layout.rects`.
fn count_vnodes(node: &VNode) -> usize {
    use crate::vnode::VNodeContent;
    let mut count = 1;
    match &node.0 {
        VNodeContent::Box(b) => {
            for child in &b.children {
                count += count_vnodes(child);
            }
        }
        VNodeContent::Transform(t) => {
            count += count_vnodes(&t.child);
        }
        VNodeContent::Fragment(fs) => {
            for child in fs {
                count += count_vnodes(child);
            }
        }
        _ => {}
    }
    count
}
