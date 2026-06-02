# runts-ink: Architecture

This document describes the full Ink-compatible
crate-plugin for runts. The user-facing surface is
TSX with Ink-style JSX (`<Box>`, `<Text>`, etc.); the
runtime stack is **Taffy** (flexbox layout) +
**Ratatui** (rendering) + **crossterm** (events).
A future step will add **rquickjs** as the JS
runtime so users can write Ink components in
TypeScript; the current `runts-ink` is the
*reference implementation* that the plugin's
emitted code matches.

## Why these crates

| Crate | Role | Why |
|---|---|---|
| Taffy | CSS flexbox/grid layout | Rust-native; used by Bevy, Dioxus, Zed. Implements the same flexbox spec as Yoga (which Ink uses in JS). |
| Ratatui | Immediate-mode TUI rendering | De-facto Rust TUI library. `Span` / `Line` / `Paragraph` / `Block` map cleanly to Ink's `Text` / `Box` primitives. |
| crossterm | Terminal control + events | Cross-platform, used by Ratatui. Provides raw mode, alternate screen, mouse, kitty keyboard, bracketed paste, focus events. |
| rquickjs | JS runtime (future) | Embeds QuickJS so the user's `.tsx` runs without a Node dep. |

## Component Mapping

### `<Text>` → `Span` / `Line` / `Paragraph`

| Ink prop | Ratatui equivalent |
|---|---|
| `children` (string) | `Span::new(text)` or `Line::from(spans)` |
| `color` | `Style::fg(Color::from_str(...))` — parse hex, rgb, named |
| `backgroundColor` | `Style::bg(Color::...)` |
| `bold` | `Modifier::BOLD` |
| `italic` | `Modifier::ITALIC` |
| `underline` | `Modifier::UNDERLINED` |
| `strikethrough` | `Modifier::CROSSED_OUT` |
| `inverse` | Swap `fg` ↔ `bg` in the `Style` |
| `dimColor` | `Modifier::DIM` (or darken the colour) |
| `wrap="wrap"` | `Paragraph::wrap(Wrap { trim: false })` |
| `wrap="hard"` | `Paragraph::wrap(Wrap { trim: true })` |
| `wrap="truncate"` | Manual truncation before creating `Span` |
| `wrap="truncate-middle"` | `..."..."` truncation logic |

`<Text>` is a Taffy **leaf** with `width: auto`,
`height: auto`. After Taffy layout, render a
`Paragraph` constrained to the computed rect. Nested
`<Text>` merges into a single `Line` with multiple
`Span`s.

### `<Box>` → Taffy node + `Block` wrapper

| Ink prop | Taffy style | Ratatui render |
|---|---|---|
| `width` / `height` | `taffy::Size { width: px/%, height: px/% }` | `Block` sized to computed rect |
| `minWidth` / `minHeight` | `min_size` | — |
| `maxWidth` / `maxHeight` | `max_size` | — |
| `paddingTop/Bottom/Left/Right` | `padding` (Rect) | `Block` inner margin |
| `paddingX` / `paddingY` / `padding` | shorthand expanded | — |
| `marginTop/Bottom/Left/Right` | `margin` (Rect) | Taffy handles spacing |
| `marginX` / `marginY` / `margin` | shorthand expanded | — |
| `gap` / `columnGap` / `rowGap` | `gap` (Size) | — |
| `flexGrow` | `flex_grow: f32` | — |
| `flexShrink` | `flex_shrink: f32` | — |
| `flexBasis` | `flex_basis` | — |
| `flexDirection` | `flex_direction: Row/Column/RowReverse/ColumnReverse` | Ratatui splits rects accordingly |
| `flexWrap` | `flex_wrap: NoWrap/Wrap/WrapReverse` | — |
| `alignItems` | `align_items: FlexStart/Center/FlexEnd/Stretch/Baseline` | — |
| `alignSelf` | `align_self` | — |
| `alignContent` | `align_content` | — |
| `justifyContent` | `justify_content` | — |
| `position="absolute"` | `position: Absolute` + `inset` | Place widget at computed coords |
| `top/right/bottom/left` | `inset` (Rect) | — |
| `display="none"` | Skip node in Taffy tree | Don't render |
| `overflowX/Y="hidden"` | Clip children to computed rect | `Paragraph` with wrap + width constraint |

**Borders (on `<Box>`):**

| Ink prop | Ratatui equivalent |
|---|---|
| `borderStyle="single"` | `BorderType::Plain` + `Borders::ALL` |
| `borderStyle="double"` | `BorderType::Double` |
| `borderStyle="round"` | `BorderType::Rounded` |
| `borderStyle="bold"` | `BorderType::Thick` |
| `borderStyle="classic"` | Custom `Block` (manual corners) |
| `borderColor` | `Block::border_style(Style::fg(...))` |
| `borderTop/Right/Bottom/Left` | `Borders::TOP/RIGHT/BOTTOM/LEFT` |
| `borderDimColor` | Dim the border style color |
| `borderBackgroundColor` | `Block::border_style(Style::bg(...))` |

**Background:** `backgroundColor` → `Block::style(Style::bg(...))`
fills the entire computed rect.

### `<Newline>` → `\n` injection

Insert `\n` into the parent `<Text>` string, or split
the `Line` vector. `<Newline count={n}>` appends `n`
newline chars to the preceding text node.

### `<Spacer>` → Taffy node with `flex_grow: 1.0`

Taffy leaf with `flex_grow: 1.0`, `flex_shrink: 1.0`,
no render output. Occupies remaining space along the
flex axis.

### `<Static>` → Pre-rendered buffer

1. On first render (or when items change), render
   static items to a separate `Buffer`.
2. Store the buffer.
3. Each frame: draw the stored buffer at the top,
   then draw the dynamic tree below.
4. Static items never re-render unless `items` prop
   changes.

### `<Transform>` → String pre-processing

`<Transform transform={fn}>` receives the string
output of its children. Since Ratatui works with
structured text (not raw ANSI strings), implement at
the **reconciler level**:

1. Collect all child `<Text>` strings.
2. Apply the JS `transform` function via rquickjs.
3. Use the transformed string to create `Span`s.

The transform runs in JS before Rust gets the final
text. The reconciler calls `transform(output, index)`
for each line, then passes the result to Rust as the
text content.

## Hook Mapping

### `useInput(handler, options?)` → crossterm key events

```rust
loop {
    if let Event::Key(key) = crossterm::event::read()? {
        let ink_key = InkKey {
            left_arrow: key.code == KeyCode::Left,
            // ... all key mappings
            ctrl: key.modifiers.contains(KeyModifiers::CONTROL),
            shift: key.modifiers.contains(KeyModifiers::SHIFT),
        };
        js_ctx.call(handler, &[input_str, ink_key])?;
    }
}
```

- `options.isActive` → enable/disable the event
  listener in Rust.
- `usePaste` → enable crossterm bracketed paste mode
  (`\x1b[?2004h`), parse paste content as a single
  string.

### `useApp()` → App lifecycle

| Ink | Rust |
|---|---|
| `exit()` | Send signal to stop the `Terminal::draw()` loop |
| `waitUntilRenderFlush()` | After `terminal.draw(...)`, the frame is flushed synchronously; return a resolved Promise immediately |

### `useStdin()` → crossterm raw mode

| Ink | Rust |
|---|---|
| `stdin` | `std::io::stdin()` (not used directly; crossterm reads events) |
| `isRawModeSupported` | `crossterm::terminal::is_raw_mode_supported()` |
| `setRawMode(true/false)` | `crossterm::terminal::enable_raw_mode()` / `disable_raw_mode()` |

### `useStdout()` → Direct stdout write

`write(data)` writes directly to `std::io::stdout()`
**outside** Ratatui's draw cycle. Must save/restore
cursor position or use `terminal.insert_before()` if
Ratatui supports it. In practice: queue a `Print(data)`
command before the Ratatui draw.

### `measureElement(ref)` / `useBoxMetrics(ref)` → Taffy layout read

```rust
let layout = taffy.layout(node_id)?;
InkMetrics {
    width: layout.size.width as u16,
    height: layout.size.height as u16,
    left: layout.location.x as u16,
    top: layout.location.y as u16,
}
```

### `useStderr()` → `std::io::stderr()`

Same pattern as stdout.

### `useWindowSize()` → crossterm resize

```rust
let (cols, rows) = crossterm::terminal::size()?;
// Listen for Event::Resize(cols, rows)
```

### `useFocus()` / `useFocusManager()` → Focus state machine

Rust maintains:
- `Vec<FocusId>` of focusable components (in render order)
- `active_focus_index: usize`
- `Tab` → `focus_next()`, `Shift+Tab` → `focus_previous()`
- `focus(id)` → find index by ID, set active
- `isFocused` boolean passed to component via React context

### `useCursor()` → crossterm cursor

```rust
crossterm::execute!(
    stdout,
    cursor::MoveTo(x, y),
    cursor::Show
)?;
```

Set after each `terminal.draw()` frame.
`setCursorPosition(undefined)` → `cursor::Hide`.

### `useAnimation(options?)` → Tokio interval

```rust
let mut interval = tokio::time::interval(Duration::from_millis(options.interval));
let start = Instant::now();
let mut last = start;
let mut frame_count = 0u64;

loop {
    interval.tick().await;
    let now = Instant::now();
    let time = now.duration_since(start).as_millis();
    let delta = now.duration_since(last).as_millis();
    last = now;

    js_ctx.call(animation_callback, &[frame_count, time, delta])?;
    frame_count += 1;
}
```

`options.isActive` → pause/resume the interval task.

## API Mapping

### `render(tree, options?)` → `Terminal::new(CrosstermBackend)` + event loop

| Ink option | Rust behaviour |
|---|---|
| `stdout` | `Terminal::new(CrosstermBackend::new(stdout))` |
| `stdin` | Event loop reads from `std::io::stdin()` |
| `stderr` | Available for `useStderr` writes |
| `exitOnCtrlC` | If true, map `Ctrl+C` to `exit()` |
| `patchConsole` | Intercept `console.log` in rquickjs, route through Ratatui's `insert_before` or static area |
| `onRender` | Call JS callback after each `terminal.draw()` with render timing |
| `isScreenReaderEnabled` | Add screen-reader-friendly output (simpler formatting, no cursor tricks) |
| `debug` | Disable clear-screen; append each frame |
| `maxFps` | Cap the render loop with `tokio::time::interval` |
| `incrementalRendering` | Only redraw changed lines (Ratatui `Buffer::diff` + partial updates) |
| `concurrent` | Use React 18+ concurrent features in QuickJS (if supported) |
| `alternateScreen` | `terminal.enter_alternate_screen()` / `leave_alternate_screen()` |
| `kittyKeyboard` | Enable crossterm kitty keyboard protocol |

### `renderToString(tree, options?)` → Buffer-only render

1. Create a `Buffer` with `options.columns` width and
   arbitrary height.
2. Build Taffy tree with fixed width constraint.
3. Compute layout.
4. Render widgets to `Buffer` (no `Terminal`, no
   backend).
5. Convert `Buffer` to string (strip trailing
   whitespace per line, join with `\n`).

### Instance methods

| Ink | Rust |
|---|---|
| `rerender(tree)` | Trigger React root render in rquickjs with new tree |
| `unmount()` | Drop rquickjs context, stop event loop, restore terminal |
| `waitUntilExit()` | `tokio::sync::oneshot::Receiver` awaiting exit signal |
| `waitUntilRenderFlush()` | Return resolved future (Ratatui draw is sync) |
| `cleanup()` | `terminal.clear()`, `terminal.leave_alternate_screen()`, restore cursor |
| `clear()` | `terminal.clear()` |

## The Render Loop

```rust
fn run_app(js_bundle: String, options: RenderOptions) -> Result<()> {
    // 1. Setup terminal
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    if options.alternate_screen {
        terminal.enter_alternate_screen()?;
    }
    enable_raw_mode()?;

    // 2. Setup rquickjs + React reconciler
    let ctx = Context::builder().build()?;
    ctx.eval(REACT_POLYFILL)?;
    ctx.eval(RECONCILER_BRIDGE)?; // JS reconciler that calls __rust_bridge.*
    ctx.eval(&js_bundle)?;

    // 3. Setup Taffy
    let mut taffy = Taffy::new();
    let root_id = taffy.new_leaf(Style::default())?;

    // 4. Setup event channels
    let (exit_tx, exit_rx) = oneshot::channel();
    let (render_tx, mut render_rx) = mpsc::channel::<TreeOps>(16);

    // 5. Inject bridge into JS global
    ctx.globals().set("__rust_bridge", JsBridge { render_tx })?;

    // 6. Start event loop
    let mut last_size = terminal.size()?;

    loop {
        // Handle tree ops from JS reconciler
        while let Ok(op) = render_rx.try_recv() {
            apply_tree_op(&mut taffy, op);
        }

        // Handle crossterm events
        if event::poll(Duration::from_millis(16))? {
            match event::read()? {
                Event::Key(k) => handle_key(&ctx, k),
                Event::Resize(w, h) => { last_size = (w, h); }
                Event::Paste(s) => handle_paste(&ctx, s),
                _ => {}
            }
        }

        // Compute layout
        taffy.compute_layout(root_id, Size {
            width: AvailableSpace::Definite(last_size.0 as f32),
            height: AvailableSpace::Definite(last_size.1 as f32),
        })?;

        // Render
        terminal.draw(|frame| {
            render_ink_tree(frame, &taffy, root_id);
        })?;

        // Check exit
        if exit_rx.try_recv().is_ok() { break; }
    }

    cleanup(terminal)?;
    Ok(())
}
```

## Gotchas & Decisions

1. **Yoga vs. Taffy.** Taffy supports every CSS flexbox
   property Ink uses. The mapping is nearly 1:1. Taffy
   returns `f32` pixel positions; cast to `u16` for
   Ratatui rects.

2. **Text measurement.** Ink uses `string-width` for
   Unicode width. Ratatui's `Span` uses `unicode-width`
   (same algorithm). Don't pre-measure in the JS
   reconciler; let Taffy + Ratatui handle it.

3. **ANSI in `<Transform>`.** If transforms strip ANSI,
   use `strip-ansi` in JS before passing to Rust.
   Ratatui doesn't use ANSI strings internally.

4. **Borders and padding.** Ratatui `Block` borders
   consume space *inside* the widget area. Taffy
   padding should account for this, or render borders
   as a separate overlay layer.

5. **Performance.** Taffy layout is fast but not free.
   For `maxFps` and `incrementalRendering`, only
   recompute Taffy layout when the tree changes, not
   every frame.

6. **Refs.** `useRef` and `measureElement` need stable
   node IDs. The reconciler assigns an incrementing ID
   to each created node; store the mapping in Rust.

7. **Hot reload.** On file change, destroy the rquickjs
   context, create a new one, re-evaluate the bundle.
   Taffy tree rebuilds from scratch. State is lost
   (acceptable for v1).

## Implementation Order

1. **Start with `render_ink_tree()`** — the function
   that takes a Taffy-computed tree and draws Ratatui
   widgets. Once `<Box borderStyle="round"><Text
   color="green">Hello</Text></Box>` renders correctly,
   the rest is incremental.

2. **Add the per-component property translation**
   (the table at the top of this document).

3. **Wire the render loop** with the `oneshot` exit
   channel and `mpsc` render ops channel.

4. **Add hooks one at a time** — `useInput` first
   (most useful), then `useApp`, then `useFocus`,
   then `useAnimation`.

5. **Add `useFocus` / `useFocusManager`** as a
   separate state struct with `Vec<FocusId>` and
   `active_focus_index`.

6. **Add `<Static>`** as a `Buffer` cache keyed on
   the items identity.

7. **Add `<Transform>`** by routing through rquickjs
   in the reconciler bridge.

## Current State (v0.1)

The `runts-ink` crate is in the workspace. It has:
- Component types (`Box`, `Text`, `Newline`, `Spacer`,
  `Static`, `Transform`).
- VNode tree.
- Props (JSON-bag).
- `render()` entry point that boots a Ratatui terminal
  and runs a render loop.
- Taffy-based flexbox layout.
- Ink keyboard flags (`Key::from_crossterm`).
- 49 unit tests passing.

**What is missing:**

- `render_ink_tree()` reads Taffy-computed rects from
  `Layout::rects` to position children. **The current
  walker (`render.rs::walk`) falls back to equal-stacking
  and ignores padding/margin.** This is the next gap to
  close.
- The hook list (`useApp`, `useStdin`, `useStdout`,
  `useStderr`, `useWindowSize`, `useFocus`,
  `useFocusManager`, `useCursor`, `useAnimation`) is
  not yet implemented. Only `useInput` is partially
  covered by the keyboard event types.
- The rquickjs JS runtime is not wired up — the
  current path is pure-Rust component trees.
- The runts-ratatui plugin recognises Ink JSX tags
  (`<Box>`, `<Text>`, `<Newline>`, `<Spacer>`,
  `<Static>`, `<Transform>`) but the recursive
  child-layout codegen for `<Box>` is sketched, not
  exercised.
