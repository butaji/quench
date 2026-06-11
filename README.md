# Quench

> Run Ink (React for terminals) in rquickjs + Rust with full API compatibility.

Quench is a high-performance terminal UI framework that runs Ink-compatible React components using QuickJS (via rquickjs) and Rust. It provides the exact Ink API, allowing existing Ink applications to run with native Rust performance.

**License:** MIT or Apache-2.0 (dual-licensed) — see [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE)

## Quickstart

```bash
# Build
cargo build --release

# Run TSX examples
./target/release/quench examples/counter.tsx
./target/release/quench examples/todo-list.tsx
./target/release/quench examples/dashboard.tsx

# Run JavaScript examples
./target/release/quench examples/simple-hello.js

# Compile and run TSX on the fly
./target/release/quench --compile examples/counter.tsx

# Run with hot reload
./target/release/quench --watch examples/counter.tsx
```

## Known Issue: Input Stops Working Inside tmux

When running Quench (or any Ink-based TUI) inside a **tmux** session, you may find that keyboard input stops working — keystrokes appear to be swallowed, the app freezes, or pressing keys has no effect.

### Why it happens

Ink apps run in **raw mode** on stdin so they can read individual keypresses (arrows, ctrl+x, escape sequences). That requires the process to have a real TTY, and it requires the terminal driver to be in a non-canonical, non-echo state.

tmux inserts itself as a pseudo-terminal layer between your shell and the real TTY:

```
┌─────────────────┐   ┌─────────────────┐   ┌──────────────┐
│  Real TTY       │◄──┤  tmux (pty)     │◄──┤  Your app    │
│  /dev/tty       │   │  passthrough    │   │  (Quench)    │
└─────────────────┘   └─────────────────┘   └──────────────┘
```

This extra layer breaks raw mode in three common ways:

1. **`process.stdin.isTTY` reports `false`** inside tmux (especially on macOS), so the app skips `setRawMode(true)` and never starts reading keypresses. The app renders fine, but input is silently dropped.

2. **Raw mode was set *before* tmux attached.** If the app was launched outside tmux and tmux attached later (or vice versa), the TTY descriptor no longer matches the active session. The old fd is stale and the new one isn't in raw mode.

3. **Readline + raw mode conflict.** Ink's `useInput` uses `node:readline` (or a shim) on top of raw stdin. Inside tmux, readline's line-editing state machine and raw keypress mode can fight each other — readline waits for a newline while raw mode delivers bytes immediately, so the keypress handler never fires.

### What you see

- App renders correctly (colors, layout, animation)
- First few keystrokes may work, then nothing
- `Ctrl+C` doesn't quit
- `echo` in the same shell works fine — only the TUI is affected

### How to fix it

**Quickest fix — run tmux in "allow-pty" mode** (this is usually the default, but worth checking):

```bash
tmux set-option -g allow-passthrough on
tmux set-option -ga terminal-features ",xterm-256color:RGB"
```

Then re-attach your session.

**Force the app to use the real TTY** by opening `/dev/tty` directly instead of stdin:

```bash
# macOS / Linux — bypasses the tmux pty layer
script -q /dev/null ./target/release/quench examples/counter.tsx
```

or wrap the launch:

```bash
# .tmux.conf
set -g default-command "exec env TERM=xterm-256color"
```

**If you control the source** (the `.tsx` file), make the TTY check lenient — treat "fd exists" as "TTY is fine" rather than requiring `isTTY === true`:

```ts
// Before: app dies silently if isTTY is false
if (!process.stdin.isTTY) return;

// After: try to use stdin, fall back to /dev/tty
const stdin = process.stdin;
if (!stdin.isTTY) {
  try {
    const fd = require("fs").openSync("/dev/tty", "r+");
    Object.assign(stdin, { fd, isTTY: true });
    stdin.setRawMode?.(true);
  } catch { /* no tty available */ }
}
```

**Avoid the conflict between readline and raw mode** by using a raw `data` listener instead of `keypress`:

```ts
// Robust in tmux — no readline state machine to fight with
process.stdin.setRawMode(true);
process.stdin.on("data", (chunk) => {
  // parse bytes yourself: \x03 = ctrl+c, \x1b[A = up arrow, etc.
});
```

### Verifying the fix

```bash
# 1. Start a fresh tmux session
tmux new -s test

# 2. Run Quench
./target/release/quench examples/counter.tsx

# 3. Press keys — counter should increment, arrow keys should navigate
# 4. Ctrl+C should exit cleanly
```

If keystrokes still don't register, check `echo $TERM` inside tmux — it should be `xterm-256color` or `tmux-256color`, not `dumb` or `linux`.

## Parity with Ink

Quench aims for exact Ink API compatibility. The project maintains a parity story:

- **59 examples** run in Quench without source modification
- **100% core Ink API** coverage achieved
- All primary TSX examples (`counter`, `todo-list`, `dashboard`, etc.) work byte-for-byte

See [docs/SPEC.md](docs/SPEC.md) for the full architecture specification and [tasks/](tasks/) for the development roadmap.

## Architecture

```
┌─────────────────────────────┐
│     TSX/JS (Ink API)       │
└─────────────┬───────────────┘
              │ esbuild (optional)
              ↓
┌─────────────────────────────┐
│     runtime.js (~1500 ln)   │
│  • React reconciler        │
│  • Hooks (useState, etc.)  │
│  • Bridge wrappers          │
└─────────────┬───────────────┘
              │ __ink_call FFI
              ↓
┌─────────────────────────────┐
│     Rust (~4000 lines)      │
│  • Yoga layout engine       │
│  • ratatui rendering        │
│  • Event loop (crossterm)   │
└─────────────────────────────┘
```

## Project Structure

```
src/
├── main.rs           # Entry point
├── event_loop.rs    # Terminal events, hot reload
├── render.rs        # ratatui rendering
├── cli.rs           # CLI argument parsing
├── runtime.js       # JS reconciler + hooks (~1500 lines)
├── bridge/          # FFI bridge
│   ├── ffi.rs       # __ink_call dispatch
│   ├── node.rs      # Node creation
│   ├── props.rs     # Props parsing
│   ├── tree.rs      # Tree mutations
│   ├── timers.rs    # Timer registry
│   └── io.rs        # stdout/stderr/exit
├── ink/             # Yoga layout
│   ├── node.rs      # InkNode + Yoga props
│   ├── runtime.rs   # Runtime state
│   └── tree.rs      # Tree operations
└── compiler/        # TSX compiler (esbuild-based)
    └── mod.rs       # JSX transformation

docs/
├── SPEC.md          # Architecture specification

tasks/
└── index.json       # Development roadmap (P0-P3 phases)
```

## Documentation

- [Architecture Spec](docs/SPEC.md) — Full system design
- [Tasks](tasks/) — Development roadmap with task tracking (P0-P3 phases)
- [EXECUTE.md](EXECUTE.md) — Build and development instructions (developer reference)

## Performance

| Metric | Target | Achieved |
|--------|--------|----------|
| Layout (10 boxes × 5 texts) | < 2ms | ~62µs ✅ |
| Tree creation (2200 nodes) | < 5ms | ~490µs ✅ |
| Prop updates (50 nodes) | < 3ms | ~808µs ✅ |
| Binary size | < 5 MB | 2.0 MB ✅ |
| Startup time | < 100ms | ~5ms ✅ |

## Supported Ink API

### Components
- `ink-box`, `ink-text`, `ink-static`, `ink-newline`, `ink-spacer`

### Hooks
- **State:** `useState`, `useEffect`, `useRef`, `useMemo`, `useCallback`
- **Context:** `createContext`, `useContext`
- **Ink:** `useInput`, `useApp`, `useStdin`, `useStdout`, `useStderr`, `useFocus`, `useFocusManager`, `measureElement`
- **Quench:** `useBridge`

### Flexbox Props
- `flexDirection`, `alignItems`, `alignSelf`, `alignContent`, `justifyContent`, `flexWrap`
- `flexGrow`, `flexShrink`, `flexBasis`
- `gap`, `gapX`, `gapY`, `columnGap`, `rowGap`

### Spacing Props
- `margin`, `marginTop`, `marginBottom`, `marginLeft`, `marginRight`, `marginX`, `marginY`
- `padding`, `paddingTop`, `paddingBottom`, `paddingLeft`, `paddingRight`, `paddingX`, `paddingY`

### Border Props
- `borderStyle`, `borderColor`, `borderDimColor`
- `borderTop`, `borderBottom`, `borderLeft`, `borderRight`
- `title`

### Text Props
- `color`, `backgroundColor`
- `bold`, `dimColor`, `italic`, `strikethrough`, `underline`, `inverse`, `small`
- `wrap` / `textWrap`, `transform` (uppercase/lowercase)

### Dimension Props
- `width`, `height`, `minWidth`, `maxWidth`, `minHeight`, `maxHeight`
- `position` (absolute), `display`, `top`, `right`, `bottom`, `left`

## Branch Policy

- `main` — Default branch, stable releases
- `ink` — Experimental Ink runtime features (recommended: delete this branch)

To reconcile branches:
```bash
git push origin --delete ink  # Delete remote ink branch
git branch -d ink           # Delete local ink branch
```

See [tasks/138-*](tasks/) for the branch reconciliation task.

## Contributing

See [docs/SPEC.md](docs/SPEC.md) for the architecture and [tasks/](tasks/) for the development roadmap. All tasks are tracked in `tasks/index.json` with status updates.

## License

Licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.
