# TuiBridge Specification v0.1

## 1. Design Philosophy

**JavaScript decides what to draw. Rust decides how and when to draw it.**

- **Zero JS in the render hot path.** The ratatui immediate-mode loop is 100% Rust. It never calls into JS.
- **One FFI call per commit.** The JS reconciler emits the entire UI tree in a single `commit()` call to Rust. No per-node FFI chatter.
- **JS is a plugin format, not a runtime dependency.** In production, JS bundles are pre-compiled to QuickJS bytecode and embedded in the binary.

---

## 2. High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│  DEV LOOP                                                           │
│  ┌──────────────┐   esbuild --watch   ┌──────────────┐            │
│  │  plugins/    │ ──────────────────► │  dist/       │            │
│  │  *.tsx       │    < 20 ms          │  *.js        │            │
│  └──────────────┘                     └──────────────┘            │
└─────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────┐
│  RUST HOST (single binary)                                          │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │  rquickjs VM (200 KB footprint)                             │   │
│  │  ┌─────────────────────────────────────────────────────┐    │   │
│  │  │  Core Library (~8 KB gzipped)                       │    │   │
│  │  │  • Custom reconciler (Fiber-lite)                   │    │   │
│  │  │  • Hooks: useState, useEffect, useMemo, useRef      │    │   │
│  │  │  • JSX factory (h) + Fragment                       │    │   │
│  │  │  • Event emitter polyfill                           │    │   │
│  │  │  • Timer polyfill (setTimeout/setInterval)            │    │   │
│  │  └─────────────────────────────────────────────────────┘    │   │
│  │  ┌─────────────────────────────────────────────────────┐    │   │
│  │  │  Plugin Bundle (user code)                          │    │   │
│  │  │  export default function App() { ... }              │    │   │
│  │  └─────────────────────────────────────────────────────┘    │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                          │ commit(tree)                           │
│                          ▼ (single call)                           │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │  Rust Bridge                                                │   │
│  │  • Deserializes JS object tree → ShadowTree                 │   │
│  │  • Diffs against previous tree                              │   │
│  │  • Marks dirty regions                                      │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                          │                                        │
│                          ▼ (60 fps or event-driven)                │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │  ratatui Render Loop (pure Rust)                            │   │
│  │  • Layout pass (top-down rect splitting)                   │   │
│  │  • Widget render (Block, Paragraph, List, etc.)             │   │
│  │  • Crossterm event pump                                     │   │
│  └─────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 3. JavaScript Reconciler (QuickJS-side)

### 3.1 JSX Transform

All `.tsx` files compile with `esbuild` using the **classic JSX transform** targeting a custom factory:

```json
{
  "jsxFactory": "h",
  "jsxFragment": "Fragment"
}
```

Example plugin source:

```tsx
import { Box, Text, useState, useEffect, useInput } from '@tui/core';

export default function App() {
  const [count, setCount] = useState(0);

  useInput((input, key) => {
    if (input === 'q') process.exit(); // handled by Rust bridge
    if (input === ' ') setCount(c => c + 1);
  });

  useEffect(() => {
    const id = setInterval(() => setCount(c => c + 1), 1000);
    return () => clearInterval(id);
  }, []);

  return (
    <Box border="round" title="Counter" padding={1}>
      <Text bold color="green">
        Count: {count}
      </Text>
      <Text dimColor>Press [space] to increment, [q] to quit</Text>
    </Box>
  );
}
```

### 3.2 Reconciler API

The reconciler is a **Preact-style VDOM** (~600 lines) with a React-compatible hook system. It does not use React DOM or Ink. It targets a "host config" that points to Rust.

```typescript
// Core library internal API
interface HostConfig {
  // Called after reconciliation completes
  commitRoot(tree: VNode): void;

  // Called when effects need scheduling
  scheduleCallback(cb: () => void): void;
}

// VNode shape emitted to Rust
type VNode = VElement | VText;

interface VElement {
  type: 'element';
  tag: string;           // 'Box', 'Text', 'Paragraph', 'List', etc.
  key: string | number | null;
  props: Record<string, any>;
  children: VNode[];
}

interface VText {
  type: 'text';
  content: string;
}
```

### 3.3 Hooks Implementation

Hooks are implemented using a **component stack** (not true fibers, but sufficient for TUI complexity):

```typescript
// Simplified hook state
let currentComponent: ComponentInstance | null = null;
let hookIndex = 0;

function useState<T>(initial: T): [T, (next: T | ((prev: T) => T)) => void] {
  const comp = currentComponent!;
  const hooks = comp.hooks;
  const idx = hookIndex++;

  if (hooks[idx] === undefined) {
    hooks[idx] = { state: initial, queue: [] };
  }

  const hook = hooks[idx];
  const setState = (next) => {
    hook.queue.push(next);
    scheduleUpdate(comp); // triggers reconciler
  };

  return [hook.state, setState];
}

function useEffect(cb: () => void | (() => void), deps?: any[]) {
  // Standard effect comparison + cleanup
}
```

**Rules enforced by convention (same as React):**
- Hooks only at top level of function components
- Hooks called in same order every render

---

## 4. Virtual Tree Protocol

### 4.1 Commit Format

The reconciler calls `globalThis.__tui_commit(tree)` once per commit. The `tree` is a plain JS object graph. Rust traverses it via the rquickjs API.

**Why not JSON strings?** Avoiding `JSON.stringify` in JS and `serde_json` in Rust saves ~30% of commit latency for large trees. rquickjs allows Rust to iterate JS objects directly.

### 4.2 Node Schema

```typescript
// Standard element
{
  type: 'element',
  tag: 'Box',
  key: null,
  props: {
    border: 'round',          // 'none' | 'single' | 'double' | 'round'
    title: 'Counter',
    titleAlign: 'center',
    padding: [1, 2, 1, 2],    // [top, right, bottom, left]
    width: '100%',             // number | '100%' | 'auto'
    height: 'auto',
    flex: 1,                   // for flex children
    backgroundColor: 'blue',   // ratatui Color
    borderColor: 'red',
    style: { bold: true, fg: 'green' }  // ratatui Style shorthand
  },
  children: [
    { type: 'text', content: 'Hello' },
    { type: 'element', tag: 'Paragraph', props: { text: '...' }, children: [] }
  ]
}

// Text node (always leaf)
{ type: 'text', content: 'Hello world' }
```

### 4.3 Supported Tags (Ink-compatible subset)

| Tag | ratatui Widget | Key Props |
|-----|---------------|-----------|
| `Fragment` | (transparent) | `children` |
| `Box` | `Block` | `border`, `title`, `padding`, `style` |
| `Text` | `Text` / `Span` | `content`, `bold`, `color`, `dimColor` |
| `Paragraph` | `Paragraph` | `text`, `wrap`, `alignment`, `scroll` |
| `List` | `List` | `items` (array of strings or VNodes) |
| `Table` | `Table` | `rows`, `header`, `widths`, `columnSpacing` |
| `Row` | `Row` | `cells` |
| `Cell` | `Cell` | `content` |
| `Gauge` | `Gauge` | `ratio`, `label`, `style` |
| `Chart` | `Chart` | `datasets`, `xAxis`, `yAxis` |
| `Tabs` | `Tabs` | `titles`, `selected`, `block` |
| `Layout` | (layout container) | `direction`, `constraints`, `children` |

---

## 5. Rust Bridge Architecture

### 5.1 ShadowTree

Rust maintains a persistent **ShadowTree** — a mirror of the JS virtual tree using strongly-typed Rust structs. This tree survives across commits and is reused for diffing.

```rust
#[derive(Debug, Clone)]
enum ShadowNode {
    Element(ShadowElement),
    Text(ShadowText),
}

#[derive(Debug, Clone)]
struct ShadowElement {
    tag: String,
    key: Option<String>,
    props: HashMap<String, PropValue>,
    children: Vec<ShadowNode>,
    // Cached layout rect from last frame
    computed_rect: Option<Rect>,
    // Event handlers (stored by JS callback ID)
    on_key: Option<u32>,
    on_click: Option<u32>,
}

#[derive(Debug, Clone)]
struct ShadowText {
    content: String,
    style: Style,
}
```

### 5.2 Diff Algorithm

When `commit()` is called, Rust diffs the new tree against the old ShadowTree:

1. **Walk both trees in parallel** (pre-order)
2. **If tag/key match:** Update props in-place, recurse into children
3. **If mismatch:** Replace subtree
4. **If node removed:** Drop it (trigger JS cleanup effects if needed)

The diff is **O(n)** where n = tree size. For typical TUI trees (< 500 nodes), this takes < 50 μs.

### 5.3 Dirty Flagging

Only nodes with changed props or changed children are marked `dirty`. During the ratatui render pass, only dirty subtrees are re-laid-out and re-rendered. Static subtrees skip layout computation.

---

## 6. ratatui Integration (Zero-Degradation Rendering)

This is the most critical section. The render loop must be pure Rust.

### 6.1 Render Loop Separation

```rust
// Main loop (tokio::task or std::thread)
loop {
    // 1. Poll crossterm events (non-blocking, 1ms timeout)
    if event::poll(Duration::from_millis(1))? {
        let evt = event::read()?;
        dispatch_to_js(evt); // may trigger JS commit
    }

    // 2. Check if JS produced a new tree
    if bridge.has_new_commit() {
        bridge.apply_commit(); // diff into ShadowTree
    }

    // 3. Render (pure Rust, zero JS)
    terminal.draw(|frame| {
        let area = frame.area();
        shadow_tree.render(area, frame.buffer_mut());
    })?;

    // 4. Frame rate cap (60fps = 16ms)
    thread::sleep(Duration::from_millis(16));
}
```

### 6.2 ShadowTree Rendering

Each `ShadowNode` implements a custom render method that integrates with ratatui:

```rust
impl ShadowNode {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        match self {
            ShadowNode::Text(t) => {
                let span = Span::styled(t.content.clone(), t.style);
                buf.set_span(area.x, area.y, &span, area.width);
            }
            ShadowNode::Element(el) => {
                // 1. Compute inner area (subtract padding/border)
                let inner = self.compute_inner_area(area);

                // 2. If this node has a border, render Block
                if let Some(border) = el.props.get("border") {
                    let block = Block::default()
                        .borders(parse_border(border))
                        .title(el.props.get("title").and_then(|t| t.as_str()));
                    block.render(area, buf);
                }

                // 3. Layout children
                let child_areas = self.layout_children(inner);

                // 4. Recurse
                for (child, child_area) in el.children.iter().zip(child_areas) {
                    child.render(child_area, buf);
                }
            }
        }
    }
}
```

### 6.3 Layout Engine

ratatui uses **constraint-based splitting**, not flexbox. We map JS props to ratatui's `Layout` system:

```rust
fn layout_children(&self, area: Rect) -> Vec<Rect> {
    let el = match self { ShadowNode::Element(e) => e, _ => return vec![] };

    let direction = el.props.get("direction")
        .and_then(|d| d.as_str())
        .map(|d| match d {
            "row" => Direction::Horizontal,
            _ => Direction::Vertical,
        })
        .unwrap_or(Direction::Vertical);

    let constraints: Vec<Constraint> = el.children.iter().map(|child| {
        match child {
            ShadowNode::Element(c) => parse_constraint(&c.props),
            ShadowNode::Text(_) => Constraint::Length(1),
        }
    }).collect();

    Layout::default()
        .direction(direction)
        .constraints(constraints)
        .split(area)
        .to_vec()
}

fn parse_constraint(props: &HashMap<String, PropValue>) -> Constraint {
    if let Some(flex) = props.get("flex").and_then(|v| v.as_f64()) {
        // Flex is mapped to ratio; exact calculation needs parent total
        Constraint::Ratio(flex as u32, 1) // simplified
    } else if let Some(w) = props.get("width").and_then(|v| v.as_f64()) {
        Constraint::Length(w as u16)
    } else if let Some(pct) = props.get("width").and_then(|v| v.as_str()) {
        if pct.ends_with('%') {
            Constraint::Percentage(pct.trim_end_matches('%').parse().unwrap_or(100))
        } else {
            Constraint::Min(0)
        }
    } else {
        Constraint::Min(0)
    }
}
```

**Performance note:** `Layout::split()` is cached internally by ratatui when constraints haven't changed. Because our ShadowTree persists across frames, we can store the `Layout` object and only recompute when children or constraints change.

### 6.4 Style Mapping

JS props map directly to ratatui's `Style`:

```rust
fn parse_style(props: &HashMap<String, PropValue>) -> Style {
    let mut style = Style::default();
    if let Some(c) = props.get("color").and_then(|v| v.as_str()) {
        style = style.fg(parse_color(c));
    }
    if props.get("bold").and_then(|v| v.as_bool()) == Some(true) {
        style = style.add_modifier(Modifier::BOLD);
    }
    if props.get("dimColor").and_then(|v| v.as_bool()) == Some(true) {
        style = style.add_modifier(Modifier::DIM);
    }
    // ... underline, italic, crossed_out, etc.
    style
}
```

---

## 7. Event System

### 7.1 Rust → JS Dispatch

Crossterm events are serialized and pushed into a JS queue:

```rust
fn dispatch_to_js(evt: Event) {
    let js_event = match evt {
        Event::Key(k) => json!({
            "type": "key",
            "code": format!("{:?}", k.code),
            "modifiers": parse_modifiers(k.modifiers),
        }),
        Event::Mouse(m) => json!({
            "type": "mouse",
            "kind": format!("{:?}", m.kind),
            "column": m.column,
            "row": m.row,
        }),
        Event::Resize(w, h) => json!({
            "type": "resize",
            "width": w,
            "height": h,
        }),
        _ => return,
    };

    // Call JS global handler
    ctx.eval(format!("__tui_dispatch({})", js_event)).ok();
}
```

### 7.2 JS Event Handling

The core library provides `useInput` and `useMouse`:

```typescript
export function useInput(handler: (input: string, key: Key) => void) {
  useEffect(() => {
    const id = globalThis.__tui_registerInputHandler((raw) => {
      handler(raw.input, raw);
    });
    return () => globalThis.__tui_unregisterHandler(id);
  }, []);
}
```

### 7.3 Hit Testing (Mouse)

For mouse clicks, Rust does hit-testing against the `computed_rect` stored in each ShadowNode. The event is dispatched to the deepest matching node that has an `onClick` handler.

---

## 8. Hot Reload System

### 8.1 File Watcher

```rust
use notify::{Watcher, RecursiveMode, watcher};
use std::sync::mpsc::channel;

let (tx, rx) = channel();
let mut watcher = watcher(tx, Duration::from_millis(100)).unwrap();
watcher.watch("plugins/", RecursiveMode::NonRecursive).unwrap();

// In main loop
if let Ok(event) = rx.try_recv() {
    match event {
        DebouncedEvent::Write(path) => reload_plugin(&path),
        _ => {}
    }
}
```

### 8.2 Reload Strategy

**Full Context Replacement** (recommended for simplicity):

1. Save current terminal state (optional)
2. Drop old rquickjs `Runtime` / `Context`
3. Create new `Runtime` with fresh core library
4. Load updated plugin bundle via `ctx.eval()`
5. Call `mount()` to trigger initial render
6. First commit rebuilds the entire ShadowTree

**Latency:** rquickjs context creation + eval takes ~2-5 ms. esbuild watch takes ~10-20 ms. Total reload latency: **<< 50 ms**.

### 8.3 State Preservation (Optional)

For preserving state across reloads, the core library can serialize hook states to Rust before context destruction:

```typescript
// Core library, pre-reload
globalThis.__tui_preserveState = () => {
  return rootComponent.__hooks.map(h => h.state);
};
```

Rust stores this, then injects it into the new context post-reload. This is opt-in.

---

## 9. Plugin API Specification

### 9.1 Entry Contract

Every plugin must export a default function component:

```typescript
export default function MyPlugin(props: { id: string }): VNode;
```

### 9.2 Core Library Exports

```typescript
// @tui/core
export function h(type: string | Function, props: any, ...children: any[]): VNode;
export function Fragment(props: { children: VNode[] }): VNode;

export function useState<T>(initial: T): [T, (v: T | ((p: T) => T)) => void];
export function useEffect(effect: () => void | (() => void), deps?: any[]): void;
export function useMemo<T>(factory: () => T, deps: any[]): T;
export function useCallback<T extends Function>(fn: T, deps: any[]): T;
export function useRef<T>(initial: T): { current: T };
export function useContext<T>(ctx: Context<T>): T;
export function createContext<T>(defaultValue: T): Context<T>;

export function useInput(handler: (input: string, key: Key) => void): void;
export function useMouse(handler: (event: MouseEvent) => void): void;
export function useResize(handler: (width: number, height: number) => void): void;

export const Box: string;
export const Text: string;
export const Paragraph: string;
export const List: string;
export const Table: string;
// ... etc
```

### 9.3 Multi-Plugin Support

Rust can load multiple plugins and mount them as separate roots:

```rust
let plugins = vec![
    load_plugin("plugins/status-bar.tsx"),
    load_plugin("plugins/file-tree.tsx"),
];
// Render them into a Layout-managed grid
```

---

## 10. Performance Guarantees

| Metric | Target | How |
|--------|--------|-----|
| **JS memory** | < 1 MB | rquickjs baseline ~200 KB + reconciler ~50 KB + plugin code |
| **Render loop** | < 1 ms/frame | Pure Rust ratatui; no JS, no allocations in hot path |
| **Commit latency** | < 5 ms | Single FFI call; diff in Rust; O(n) tree walk |
| **Hot reload** | < 50 ms | esbuild watch + rquickjs context swap |
| **Binary size** | < 5 MB | Rust + ratatui + rquickjs static link; no V8, no Node |
| **Startup** | < 20 ms | No JS engine initialization overhead |

---

## 11. Implementation Roadmap

### Phase 1: Foundation (Week 1)
- [ ] Set up rquickjs + ratatui + crossterm scaffold
- [ ] Build custom reconciler (JS): `h`, `Fragment`, basic mount/unmount
- [ ] Implement `useState` hook
- [ ] Build Rust bridge: `commit()` receiver, ShadowTree storage
- [ ] Map `Box` and `Text` tags to ratatui `Block` + `Span`

### Phase 2: Layout & Events (Week 2)
- [ ] Implement constraint mapping (`width`, `height`, `flex`, `direction`)
- [ ] Build `Layout` tag using ratatui's `Layout::split()`
- [ ] Crossterm event pump → JS dispatch
- [ ] `useInput` hook
- [ ] `Paragraph`, `List` widgets

### Phase 3: Polish & Reload (Week 3)
- [ ] ShadowTree diffing (in-place updates)
- [ ] `useEffect` + cleanup
- [ ] `useRef`, `useMemo`, `useCallback`
- [ ] File watcher + context hot-swap
- [ ] esbuild integration

### Phase 4: Advanced Widgets (Week 4)
- [ ] `Table`, `Gauge`, `Chart`, `Tabs`
- [ ] Mouse support + hit testing
- [ ] `useContext` / `createContext`
- [ ] Multi-plugin orchestration

### Phase 5: Production (Week 5)
- [ ] QuickJS bytecode precompilation (`qjsc` / `write_object`)
- [ ] Embed bytecode in Rust binary with `include_bytes!`
- [ ] Strip dev-only code (hot reload, watcher)
- [ ] Release builds + optimization

---

## 12. Example: Complete Plugin

```tsx
// plugins/dashboard.tsx
import { Box, Text, Paragraph, List, useState, useEffect, useInput } from '@tui/core';

export default function Dashboard() {
  const [items, setItems] = useState(['File 1', 'File 2', 'File 3']);
  const [selected, setSelected] = useState(0);

  useInput((input, key) => {
    if (key.name === 'up') setSelected(i => Math.max(0, i - 1));
    if (key.name === 'down') setSelected(i => Math.min(items.length - 1, i + 1));
    if (input === 'd') {
      setItems(prev => prev.filter((_, idx) => idx !== selected));
    }
  });

  return (
    <Box direction="row" width="100%" height="100%">
      <Box width="30%" border="single" title="Files">
        <List
          items={items.map((item, i) => (
            <Text color={i === selected ? 'cyan' : 'white'} bold={i === selected}>
              {i === selected ? '> ' : '  '}{item}
            </Text>
          ))}
        />
      </Box>
      <Box flex={1} border="single" title="Preview" padding={1}>
        <Paragraph>
          Selected: {items[selected] || 'None'}
        </Paragraph>
      </Box>
    </Box>
  );
}
```

---

## 13. Why This Protects ratatui

1. **No JS in `draw()`:** The `terminal.draw()` closure only touches Rust structs. JS is idle during rendering.
2. **Persistent ShadowTree:** Layout constraints are cached in Rust. Only changed nodes trigger `Layout::split()` recomputation.
3. **No dynamic dispatch in render:** Tags are matched via `match` on `String` (or ideally interned strings/small enum). No vtables, no JS callbacks.
4. **Zero-copy where possible:** Text content is stored as `String` in Rust (owned). No repeated deserialization.
5. **Event-driven commits:** JS only wakes up on keyboard input or timers, not on every frame.

This gives you the **Ink development experience** (save a `.tsx` file, see the TUI update in <50ms) with the **runtime efficiency of a native Rust TUI** (sub-millisecond frames, <5 MB binaries).
