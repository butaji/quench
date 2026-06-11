<div align="center">

# Quench

### Build terminal UIs with React. Ship a Rust binary.

**The TSX/React workflow you already know. The Rust runtime your CLI deserves.**

[Quickstart](#quickstart) · [Examples](#examples) · [Performance](#performance) · [Ink API](#ink-api) · [tmux](#tmux)

</div>

---

## Built for two kinds of developers

### 1. Rust developers who hate imperative TUI code

You know Rust. You need a TUI. But `ratatui` / `tui-rs` force you into manual layout, mutable widget state, and callback spaghetti.

Quench gives you the component model you already use on the web:

```tsx
const App = () => {
  const [count, setCount] = useState(0);
  useInput((_, key) => key.upArrow && setCount(c => c + 1));

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Counter</Text>
      <Text>{count}</Text>
    </Box>
  );
};
```

Same declarative layout. Same hooks. Same mental model. No browser. No Node.js. Just a `cargo build --release` away.

### 2. Ink developers who are done with the Node.js runtime

You built something great with Ink. Now you're shipping a 50 MB bundle to render an 80×24 grid, fighting V8 GC pauses, and explaining `node_modules` to users who just want a CLI tool.

Quench runs your Ink app as a **single native binary**:

```bash
# Your existing Ink app
quench src/app.tsx
```

No source changes. No `package.json`. No `npm install`. One executable.

---

## Why Quench

| | Ink + Node.js | ratatui (Rust) | Quench |
|---|---|---|---|
| Component model | ✅ React | ❌ imperative widgets | ✅ React |
| Single binary | ❌ needs Node | ✅ | ✅ |
| Idle memory | ~30 MB | ~2–5 MB | **~5 MB** |
| Cold start | 80–200 ms | ~5 ms | **~5 ms** |
| Render frame | 5–15 ms | <1 ms | **<1 ms** |
| GC pauses | ✅ yes | ❌ none | **none** |
| Package footprint | ~150 MB `node_modules` | Cargo deps | **zero runtime deps** |

Quench is the only option that gives you **React's DX with Rust's deployment profile**.

---

## Performance you can feel

TUI apps run in a tight loop: poll input → reconcile state → layout → draw. Every millisecond matters when you're rendering 60 frames per second.

Quench puts the entire hot path in Rust:

```
┌─────────────┐    ┌─────────────┐    ┌─────────────────────────────┐
│  your.tsx   │───▶│  QuickJS    │───▶│  Rust: Yoga + ratatui +     │
│  (React)    │    │  reconciler │    │  crossterm + event loop     │
└─────────────┘    └─────────────┘    └─────────────────────────────┘
       component logic only                all hot paths, zero GC
```

**Measured on a warm release build:**

| Operation | Time | Frame budget at 60fps |
|---|---|---|
| Layout (10 boxes × 5 texts) | ~62 µs | 0.4% |
| Tree creation (2,200 nodes) | ~490 µs | 2.9% |
| Prop updates (50 nodes) | ~808 µs | 4.8% |
| Full frame (200×60 grid) | ~1.2 ms | **7.2%** |

That's **800+ fps** of theoretical headroom. Your terminal becomes the bottleneck, not the runtime.

Run the harness yourself: `cargo bench`.

---

## The Ink compatibility bet

Ink has the largest TUI developer community on the planet. Templates, Stack Overflow answers, design patterns — it's all there.

Quench is a bet on that ecosystem:

- **59 examples** in this repo, all running unmodified against the Ink API
- Every primary component, hook, and prop supported (`Box`, `Text`, `useState`, `useInput`, `useFocus`, …)
- Drop an existing Ink app into Quench and it works

```bash
npx create-ink-app my-app
cd my-app
quench src/app.tsx
```

For Rust developers, this means a mature component library and community knowledge on day one.  
For Ink developers, this is the migration path to a native binary without a rewrite.

---

## DX that actually ships

**Hot reload, no config:**
```bash
$ quench --watch my-app.tsx
  ✓ Compiled in 12ms
  ✓ Reloaded
```

**One binary to distribute:**
```bash
cargo build --release
./target/release/quench --compile my-app.tsx
# ship ./target/release/quench — Alpine, macOS, scratch Docker
```

**Native integrations when you need them:**
Because Quench is Rust under the hood, you can pull in any crate — databases, FFI, async I/O, system APIs — without bridging out to another language.

---

## Quickstart

```bash
# Build
cargo build --release

# Run examples
./target/release/quench examples/counter.tsx
./target/release/quench examples/dashboard.tsx

# Hot-reload your own app
./target/release/quench --watch my-app.tsx
```

**Requirements:** Rust 1.74+, Deno 1.40+ (only for TSX transpilation; `.js` files run directly).

---

## Examples

59 runnable examples in [`examples/`](examples/):

| Example | What it shows |
|---|---|
| `counter.tsx` | State + keyboard input |
| `dashboard.tsx` | Multi-pane Yoga layout |
| `todo-list.tsx` | CRUD with effects |
| `chat-ui.tsx` | Streaming input, focus |
| `tabs.tsx` | Tabbed navigation |
| `spinner.tsx` | Animation |
| `mouse-app.tsx` | Mouse events |
| `log-viewer.js` | Pure JS, no transpile |

All match Ink's output byte-for-byte.

---

## Ink API

### Components
`Box` · `Text` · `Static` · `Newline` · `Spacer`

### Hooks
- **State:** `useState`, `useEffect`, `useRef`, `useMemo`, `useCallback`
- **Context:** `createContext`, `useContext`
- **Ink:** `useInput`, `useApp`, `useStdin`, `useStdout`, `useStderr`, `useFocus`, `useFocusManager`, `measureElement`

### Layout (Yoga)
`flexDirection`, `alignItems`, `alignSelf`, `alignContent`, `justifyContent`, `flexWrap`, `flexGrow`, `flexShrink`, `flexBasis`, `gap`, `gapX`, `gapY`, `columnGap`, `rowGap`

### Spacing
`margin`, `marginTop`, `marginBottom`, `marginLeft`, `marginRight`, `marginX`, `marginY`, `padding`, `paddingTop`, `paddingBottom`, `paddingLeft`, `paddingRight`, `paddingX`, `paddingY`

### Borders
`borderStyle`, `borderColor`, `borderDimColor`, `borderTop`, `borderBottom`, `borderLeft`, `borderRight`, `title`

### Text
`color`, `backgroundColor`, `bold`, `dimColor`, `italic`, `strikethrough`, `underline`, `inverse`, `wrap` / `textWrap`, `transform`

### Dimensions
`width`, `height`, `minWidth`, `maxWidth`, `minHeight`, `maxHeight`, `position` (absolute), `display`, `top`, `right`, `bottom`, `left`

---

## <a name="tmux"></a>Known issue: input drops in tmux

Like all raw-mode TUIs, Quench can lose keyboard input inside tmux. The pty layer breaks raw mode in three ways:

1. **`process.stdin.isTTY` is `false`** inside tmux (especially macOS), so `setRawMode(true)` is skipped.
2. **Stale fd** if tmux attached after the process started.
3. **Readline + raw mode conflict** in Ink's `useInput` shim.

### Fixes

**Configure tmux passthrough:**
```bash
tmux set-option -g allow-passthrough on
tmux set-option -ga terminal-features ",xterm-256color:RGB"
```

**Force the real TTY:**
```bash
script -q /dev/null ./target/release/quench examples/counter.tsx
```

**Lenient `isTTY` check in your app:**
```ts
const stdin = process.stdin;
if (!stdin.isTTY) {
  try {
    const fd = require("fs").openSync("/dev/tty", "r+");
    Object.assign(stdin, { fd, isTTY: true });
    stdin.setRawMode?.(true);
  } catch { /* no tty available */ }
}
```

After any fix, check `echo $TERM` is `xterm-256color` or `tmux-256color`.

---

## Building

```bash
cargo build --release                          # JS runtime
cargo build --release --features hotreload     # + file watching
```

Release profile uses `opt-level = "z"`, LTO, and `panic = "abort"` for a small binary.

---

## License

[MIT](LICENSE-MIT)
