# TuiBridge Specification v0.1

## 1. The Core Insight

Ink is a **React custom renderer**. It does not use the DOM; it uses `react-reconciler` with a "host config" that creates Yoga nodes, measures text, and writes ANSI to `process.stdout`.

If we provide a JS shim that exports the exact same Ink API but whose reconciler host config calls into Rust instead of Node.js, **Ink examples work unmodified**. The JS side is ~15 KB of shim + React reconciler. The heavy work (Yoga, terminal I/O, event loop) is Rust.

---

## 2. What Runs in rquickjs

A single bundled JS file loaded into the VM:

```
ink-shim.js (~120 KB total)
├── React + react-reconciler (~80 KB, pure JS, works in QuickJS)
├── Host Config Interceptor (~5 KB) — TWO backends
│   DENO: createInstance → Yoga-WASM node + Deno stdout ANSI
│   bridge:  createInstance → __ink_create_node()
│   bridge:  appendChild    → __ink_append_child()
│   bridge:  commitUpdate   → __ink_commit_update()
│   bridge:  measureText    → __ink_measure_text()
├── Ink API Surface (~10 KB)
│   └── render(), Box, Text, Static, Newline, Spacer
│   └── useInput, useApp, useStdin, useStdout, useStderr
│   └── useFocus, useFocusManager, measureElement
│   └── useEffect, useState, useMemo, useCallback, useRef (React built-ins)
└── User Plugin Code
    └── import {render, Box, Text, useInput} from 'ink'
```

**Deno backend:** Host config targets Yoga-WASM (or taffy-WASM) for layout and writes ANSI directly via `Deno.stdout.write()`. Uses Deno-native timers and `setTimeout`.

**TuiBridge backend:** Host config calls `__ink_*` bridge functions into Rust.

**Polyfills provided by Rust via `globalThis` (TuiBridge only):**
- `setTimeout` / `setInterval` / `clearTimeout` → tokio timer bridge
- `setImmediate` / `clearImmediate` → microtask queue
- `process.nextTick` → microtask queue
- `console` → tracing/log

---

## 3. The Rust Backend

### 3.1 Layout Engine: Yoga (`yoga-rs`)

We use Facebook's Yoga C++ library via Rust bindings (`yoga-rs` or `taffy` as fallback). Every reconciler node has a 1:1 `YogaNode` in Rust.

**Why Yoga in Rust?** Ink's layout behavior is defined by Yoga. Using the same engine guarantees pixel-identical layouts. Text measurement is bridged to Rust's `unicode-width` + `textwrap`.

```rust
struct InkNode {
    id: u32,
    tag: InkTag,               // Box | Text | Static | Spacer
    props: HashMap<String, PropValue>,
    children: Vec<u32>,
    yoga: YogaNode,
    text: Option<String>,      // concatenated text from child text instances
    style: Style,              // ratatui style cache
}

// Note: <Text> may contain nested <Text> children and raw strings.
// The reconciler creates text instances via createTextInstance() and
// appends them as children. The parent Text node's text field is
// concatenated from children during commit, or render builds Spans
// from child nodes for full style nesting.
```

**Text Measurement Bridge:**
When Yoga lays out a `<Text>` node, it calls a measure function we register in Rust:

```rust
fn measure_text_node(node_id: u32, width: f32, _height: f32) -> Size {
    let node = tree.get(node_id);
    let text = node.text.as_ref().unwrap();
    let max_width = width as usize;
    let lines = textwrap::wrap(text, max_width);
    let w = lines.iter()
        .map(|l| unicode_width::UnicodeWidthStr::width(l.as_ref()))
        .max().unwrap_or(0) as f32;
    let h = lines.len() as f32;
    Size { width: w, height: h }
}
```

### 3.2 Renderer: ratatui Buffer (Not Layout)

We **do not use ratatui's `Layout` system**. Yoga already computed absolute `x, y, width, height` for every node. We use ratatui purely for:

- **Double-buffered terminal output** (`Buffer` diffing + flush)
- **Widget primitives** (`Block`, `Paragraph`, `Span`) placed at Yoga coordinates
- **Crossterm backend abstraction**

```rust
fn render_yoga_tree(node_id: u32, buf: &mut Buffer) {
    let node = tree.get(node_id);
    let layout = node.yoga.get_layout();
    let rect = Rect {
        x: layout.left() as u16,
        y: layout.top() as u16,
        width: layout.width() as u16,
        height: layout.height() as u16,
    };

    match node.tag {
        InkTag::Box => {
            let block = Block::default()
                .borders(parse_border(&node.props))
                .title(node.props.get("title").cloned().unwrap_or_default());
            block.render(rect, buf);
        }
        InkTag::Text => {
            let text = Paragraph::new(node.text.clone().unwrap_or_default())
                .style(node.style)
                .wrap(Wrap { trim: true });
            text.render(rect, buf);
        }
        InkTag::Static => {
            // Static items are rendered above the main tree
            // (same semantics as Ink: unmounting Static items is expensive)
        }
        _ => {}
    }

    for &child_id in &node.children {
        render_yoga_tree(child_id, buf);
    }
}
```

**Performance guarantee:** The render pass is a single recursive Rust function writing to a `Buffer`. No JS, no allocations, no bridge. For a 500-node tree, this is **<< 1 ms**.

### 3.3 Event Loop: Event-Driven, Zero Polling

```rust
#[tokio::main]
async fn main() -> Result<()> {
    let mut terminal = setup_terminal()?;
    let mut reader = EventStream::new();
    let mut vm = InkVm::new().await?; // owns rquickjs Runtime, Yoga root, timer/reload channels

    // Load initial bundle
    vm.eval_bundle(include_str!("dist/bundle.js"))?;
    vm.mount_app()?; // calls render(<App />) in JS; stores unmount() callback

    let mut dirty = true; // initial render

    loop {
        // Block forever until something happens
        tokio::select! {
            biased;

            Some(Ok(evt)) = reader.next() => {
                match evt {
                    Event::Key(k) => vm.dispatch_key(k),
                    Event::Mouse(m) => vm.dispatch_mouse(m),
                    Event::Resize(w, h) => vm.dispatch_resize(w, h),
                    _ => {}
                }
                // JS handlers ran synchronously. If they called setState,
                // the reconciler already called __ink_commit() before we returned.
                dirty = vm.is_dirty();
            }

            Some(timer_id) = vm.timer_rx.recv() => {
                vm.dispatch_timer(timer_id);
                dirty = vm.is_dirty();
            }

            Some(path) = vm.reload_rx.recv() => {
                vm.hot_reload(&path).await?;
                dirty = true;
            }
        }

        // Batch rapid events (e.g., timer fired during render)
        while vm.drain_events() {
            dirty = true;
        }

        // Render exactly once per event batch
        if dirty {
            terminal.draw(|frame| {
                let area = frame.area();
                vm.yoga_root.set_width(area.width as f32);
                vm.yoga_root.set_height(area.height as f32);
                vm.yoga_root.calculate_layout();
                vm.render_yoga_tree(vm.root_id, frame.buffer_mut());
            })?;
            vm.clear_dirty();
            dirty = false;
        }
    }
}
```

---

## 4. The Bridge API (`globalThis.__ink_*`)

These are the only functions JS calls into Rust. All are synchronous, batched during reconciliation.

| JS Call | Rust Handler | Purpose |
|---------|-------------|---------|
| `__ink_create_root()` | `create_root()` → `u32` | Create terminal root node |
| `__ink_create_node(tag, props)` | `create_node(tag, props)` → `u32` | Create Yoga node, set flex props |
| `__ink_create_text_node(text)` | `create_text_node(text)` → `u32` | Create Yoga leaf with measure func |
| `__ink_append_child(p, c)` | `append_child(p, c)` | Build tree, mark dirty |
| `__ink_remove_child(p, c)` | `remove_child(p, c)` | Detach, mark dirty |
| `__ink_insert_before(p, c, b)` | `insert_before(...)` | Reorder, mark dirty |
| `__ink_commit_update(id, props)` | `update_node(id, props)` | Update Yoga props, mark dirty |
| `__ink_set_text(id, text)` | `set_text(id, text)` | Update text, mark dirty |
| `__ink_commit()` | `commit()` | Trigger layout + render |
| `__ink_measure_text(text, width)` | `measure_text(...)` → `{w, h}` | Text measurement for Yoga |
| `__ink_measure_element(id)` | `measure_element(id)` → `{w, h}` | Yoga-computed node dimensions |
| `__ink_register_input(cb)` | `register_input(id, cb)` | Store JS callback for keys/mouse |
| `__ink_unregister_input(id)` | `unregister_input(id)` | Remove handler |
| `__ink_exit(code?)` | `exit_app(code?)` | Break event loop |
| `__ink_stdout_write(data)` | `stdout.write(data)` | Direct crossterm write |
| `__ink_stderr_write(data)` | `stderr.write(data)` | Direct crossterm write |
| `__ink_stdin_is_raw()` | `is_raw_mode()` → `bool` | Query terminal state |
| `__ink_set_raw_mode(enabled)` | `set_raw_mode(enabled)` | Toggle crossterm raw mode |
| `__ink_destroy_root(id)` | `destroy_root(id)` | Drop Yoga tree and all nodes |

**No other host calls exist.** React's reconciliation may call these 50–100 times per commit, but they are all in-memory Rust operations (HashMap inserts, Yoga node updates). Total commit overhead: **<< 2 ms**.

---

## 5. Ink API Implementation Details

### 5.1 `render(<App />, {stdout, stdin, stderr, debug, patchConsole})`

```javascript
// ink/index.js
import Reconciler from 'react-reconciler';
import {hostConfig} from './host-config.js';

const InkRenderer = Reconciler(hostConfig);

// Host config must implement getPublicInstance so refs work:
// getPublicInstance: (instance) => ({id: instance.id})

export function render(node, options = {}) {
  const rootId = globalThis.__ink_create_root();
  const container = {id: rootId};
  const root = InkRenderer.createContainer(container, 0, null, false, null, '', console.error, null);

  InkRenderer.updateContainer(node, root, null, () => {
    globalThis.__ink_commit();
  });

  return {
    waitUntilExit: () => new Promise(r => globalThis.__ink_on_exit = r),
    unmount: () => {
      InkRenderer.updateContainer(null, root, null, () => {});
      globalThis.__ink_destroy_root(rootId);
    }
  };
}
```

### 5.2 Components

```javascript
// Box, Text, Static, etc. are just React components that return
// reconciler elements. The host config intercepts them.
export const Box = 'ink-box';
export const Text = 'ink-text';
export const Static = 'ink-static';
export const Newline = 'ink-newline';
export const Spacer = 'ink-spacer';
```

### 5.3 Hooks

```javascript
// useInput registers a callback in Rust's crossterm dispatcher
export function useInput(handler, options = {}) {
  useEffect(() => {
    const id = globalThis.__ink_register_input((input, key) => {
      if (options.isActive !== false) {
        handler(input, key);
      }
    });
    return () => globalThis.__ink_unregister_input(id);
  }, [options.isActive]);
}

// useApp returns the app context
export function useApp() {
  return useMemo(() => ({
    exit: (err) => globalThis.__ink_exit(err ? 1 : 0),
    stdout: { write: (d) => globalThis.__ink_stdout_write(d) },
    stdin: { isRawModeSupported: () => globalThis.__ink_stdin_is_raw() },
    stderr: { write: (d) => globalThis.__ink_stderr_write(d) },
  }), []);
}

// useStdin, useStdout, useStderr, useFocus, useFocusManager
// are all thin wrappers over __ink_* calls or React context
```

### 5.4 `measureElement(ref)`

```javascript
export function measureElement(ref) {
  if (!ref?.current?.id) return undefined;
  return globalThis.__ink_measure_element(ref.current.id);
}
```

Rust returns `{width, height}` from the Yoga node's computed layout.

---

## 6. Hot Reload

Because React state lives in JS, we do a **fast remount** instead of trying to patch the reconciler:

1. `notify` detects `plugins/*.tsx` change
2. `esbuild --watch` rebuilds in **~10 ms**
3. Rust calls the JS `unmount()` function returned by `render()`, which destroys the React root and calls `__ink_destroy_root`
4. Rust calls `ctx.eval(new_bundle)` in the **same** rquickjs runtime (no VM restart)
5. Rust calls `vm.mount_app()` → React mounts fresh, emits `__ink_commit()`
6. Rust renders new tree

**Latency:** ~30 ms end-to-end. You lose component state on reload, but TUI state is usually ephemeral. For state preservation, serialize hook states to Rust before unmount and rehydrate after — optional Phase 2 feature.

---

## 7. Production Build

```rust
// Precompile JS to QuickJS bytecode at build time
// (use qjsc CLI or rquickjs compile_module API)
let bytecode = compile_bundle(include_str!("dist/bundle.js"));
// Embed in binary
const BUNDLE: &[u8] = include_bytes!("../dist/bundle.qbc");

// At runtime, load directly into VM — no parse overhead
let module = ctx.compile_module("ink-app", BUNDLE)?;
module.eval()?;
```

No `esbuild`, no file watcher, no source maps. A single native binary with an embedded JS app.

---

## 8. Performance Summary

| Metric | Value | Why |
|--------|-------|-----|
| JS engine memory | ~300 KB | rquickjs + React reconciler + shim |
| Layout | ~0.5–2 ms | Yoga C++ in Rust |
| Render | ~0.3–1 ms | Pure Rust recursive buffer write |
| Commit (JS→Rust) | ~1–3 ms | Batched bridge, no per-node chatter |
| Input latency | ~0.5 ms | Event-driven, no polling |
| Hot reload | ~30 ms | esbuild + remount |
| Binary size | ~3–5 MB | Rust + Yoga + ratatui + rquickjs static |
| Idle CPU | 0% | Blocks on `tokio::select!` |

---

## 9. Build Order

**Week 1: Bridge**
- [ ] Set up rquickjs + `yoga-rs` + ratatui + crossterm
- [ ] Implement `__ink_create_node`, `__ink_append_child`, `__ink_commit`
- [ ] Build host config that targets these functions
- [ ] Mount a static `<Box><Text>hello</Text></Box>` from JS

**Week 2: React + Ink API**
- [ ] Bundle React + react-reconciler for QuickJS
- [ ] Implement `render()`, `Box`, `Text`, `useInput`
- [ ] Event-driven crossterm loop dispatching to JS

**Week 3: Layout + Widgets**
- [ ] Map all Ink flex props to Yoga (`flexDirection`, `justifyContent`, `alignItems`, `padding`, `margin`, `borderStyle`, etc.)
- [ ] Text measurement bridge
- [ ] `Paragraph`, `Static`, `Newline`, `Spacer`

**Week 4: Advanced API**
- [ ] `useApp`, `useStdin`, `useStdout`, `useStderr`, `useFocus`, `useFocusManager`
- [ ] `measureElement`
- [ ] Mouse support

**Week 5: DevEx + Ship**
- [ ] `esbuild --watch` integration
- [ ] Hot reload (unmount/remount)
- [ ] QuickJS bytecode precompilation
- [ ] Strip dev code for release

---

## 10. Example: Exact Ink Code Working

```tsx
// plugins/dashboard.tsx
import React, {useState, useEffect} from 'react';
import {render, Box, Text, useInput, useApp, Static} from 'ink';

const Counter = () => {
  const [count, setCount] = useState(0);
  const {exit} = useApp();

  useInput((input, key) => {
    if (input === 'q') exit();
    if (input === ' ') setCount(c => c + 1);
  });

  useEffect(() => {
    const t = setInterval(() => setCount(c => c + 1), 1000);
    return () => clearInterval(t);
  }, []);

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text color="green" bold>Counter App</Text>
      <Text>Count: {count}</Text>
      <Text dimColor>[space] increment | [q] quit</Text>
    </Box>
  );
};

render(<Counter />);
```

This file requires **zero changes** from standard Ink. It imports from `ink`, uses React hooks, `useApp`, `useInput`, flex props, `borderStyle`, `color`, `dimColor`, `bold`. The only difference is that at build time, `esbuild` bundles it with our `ink` shim instead of the npm `ink` package, and the runtime is rquickjs + Rust instead of Node.js.

**That is the final architecture.** Intercept the reconciler, keep the API, move everything else to Rust.
