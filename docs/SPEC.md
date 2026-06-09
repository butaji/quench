# TuiBridge Specification v0.1

## 1. Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        USER CODE                              │
│               TS/TSX (Ink-compatible API)                    │
│              examples/counter.tsx                            │
└─────────────────────────┬───────────────────────────────────┘
                          │ esbuild transpile (optional)
                          ↓
┌─────────────────────────────────────────────────────────────┐
│                      TRANSPILED JS                           │
│     Element trees: { type: "ink-box", props: {...} }       │
│     Component functions with hooks (useState, useInput)     │
└─────────────────────────┬───────────────────────────────────┘
                          │ rquickjs loads & executes
                          ↓
┌─────────────────────────────────────────────────────────────┐
│                   RUNTIME.JS (~900 lines JS)                 │
│  Loaded via include_str!("runtime.js") into rquickjs VM    │
│                                                              │
│  • React reconciler (mountTree, reconcileTree)             │
│  • Hooks (useState, useEffect, useRef, useMemo, ...)       │
│  • Bridge wrappers (__ink_create_node → __ink_call)        │
│  • Timer/microtask dispatch (__tb_invoke_timers)           │
│  • Console/process polyfills                               │
└─────────────────────────┬───────────────────────────────────┘
                          │ __ink_call FFI
                          ↓
┌─────────────────────────────────────────────────────────────┐
│                    ALL HOT PATH IN RUST                     │
│                                                              │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  main.rs      866 lines — Event loop, rendering     │   │
│  │  bridge.rs   1191 lines — FFI, timers, I/O          │   │
│  │  ink.rs       614 lines — Yoga tree, layout         │   │
│  │  hotreload.rs 127 lines — File watching, remount    │   │
│  │  ink_js.rs     52 lines — Constants (Box, Text...)  │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

**Design principle:** Reconciler in JS (runs on state change), all hot paths in Rust (runs every frame).

---

## 2. Stack

| Layer | Technology | Lines | Purpose |
|-------|------------|-------|---------|
| User code | TS/TSX | — | Ink-compatible API |
| Transpile | esbuild | — | TSX → JS (optional for .js files) |
| Reconciler | JS (runtime.js) | ~1050 | Hooks, component lifecycle, tree diff |
| Bridge | JS→Rust FFI | — | `__ink_call(method, args)` |
| **Runtime** | **Rust** | **2850** | **Tree, layout, render, timers, I/O, hot reload** |

**Total Rust:** ~2,875 lines  
**Total JS (runtime):** ~1050 lines  
**Ratio:** ~73% Rust, ~27% JS

---

## 3. Rust Modules

```
src/
├── main.rs       866 lines  # Entry point, event loop, ratatui rendering
├── bridge.rs    1191 lines  # FFI bridge, timers, microtasks, I/O
├── ink.rs        614 lines  # Yoga tree, layout calculation
├── hotreload.rs  127 lines  # File watching, remount cycle
└── ink_js.rs      52 lines  # Constants registration (Box, Text, etc.)

build.rs            # Bytecode precompilation
scripts/
├── parity.sh       # Side-by-side Deno/TuiBridge runner
└── ansi-diff.js    # ANSI output comparison
```

---

## 4. JS Runtime (runtime.js)

The reconciler lives in JS because:
1. **rquickjs Function references** — callbacks, component functions
2. **Hook state** — per-instance hook arrays, React rules
3. **Tree diffing** — DOM-like reconciliation algorithm

**NOT on the hot path.** The reconciler only runs when:
- Initial render
- State changes (setCount, setState)
- Props change

**Hot path (every frame) is in Rust:**
- Event loop: tokio::select! in main.rs
- Layout: Yoga in ink.rs
- Rendering: ratatui in main.rs

### 60fps Optimization (Task 055)

**Before:** Each timer callback was a string, eval'd individually:
```rust
// OLD: 1 eval per callback
for cb in callbacks { ctx.eval("callback_code"); }
```

**After:** Timer callbacks stored as Function refs in JS Map. Rust passes IDs. Single batch dispatch:
```rust
// NEW: 1 eval for all callbacks
let ids = bridge::__ink_process_timers(); // "[1,2,3]"
ctx.eval("__tb_invoke_timers([1,2,3])");  // JS calls Function refs
```

**Key/mouse dispatch:** Still uses string eval (1 per event). Future optimization.

---

## 5. Event Loop (Rust)

```rust
#[tokio::main]
async fn main() -> Result<()> {
    let runtime = rquickjs::Runtime::new()?;
    let ctx = rquickjs::Context::full(&runtime)?;

    // 1. Register constants (Box, Text, etc.)
    ink_js::register(ctx)?;

    // 2. Load reconciler runtime
    ctx.eval(include_str!("runtime.js"))?;

    // 3. Run user code
    ctx.eval(user_code)?;

    // 4. Event loop
    loop {
        tokio::select! {
            Some(Ok(Event::Key(key))) = event_stream.next() => {
                // String dispatch (1 eval per key)
                ctx.eval("__tb_dispatch_key('Enter', false, ...)")
            }
            Some(Ok(Event::Mouse(mouse))) = event_stream.next() => {
                ctx.eval("__tb_dispatch_mouse(...)")
            }
            Some(Ok(Event::Resize(w, h))) = event_stream.next() => {
                bridge::__ink_set_terminal_size(w, h);
            }
            _ = timer_tick => {
                // Batch dispatch (1 eval for all timers)
                let ids = bridge::__ink_process_timers();
                ctx.eval("__tb_invoke_timers([...])")
            }
        }

        if bridge::__ink_is_dirty() {
            terminal.draw(|frame| render_tree(frame))?;
            bridge::__ink_clear_dirty();
        }
    }
}
```

---

## 6. Performance

| Component | Path | Latency | Status |
|-----------|------|---------|--------|
| Layout | Rust + Yoga | ~1ms | ✅ |
| Rendering | Rust + ratatui | ~1ms | ✅ |
| Reconciler | JS (runtime.js) | ~2ms | ✅ (on state change) |
| Timer dispatch | Batch eval + Function.call | ~0.1ms | ✅ (Task 055) |
| **Key dispatch** | String eval | **~0.5ms** | ⚠️ 1 eval per key |
| **Mouse dispatch** | String eval | **~0.5ms** | ⚠️ 1 eval per event |

### 60fps Budget

| Operation | Time | Cumulative |
|-----------|------|------------|
| Key event | 0.5ms | 0.5ms |
| State update | 0.1ms | 0.6ms |
| Re-render (JS) | 2ms | 2.6ms |
| Yoga layout | 1ms | 3.6ms |
| ratatui render | 1ms | 4.6ms |
| Buffer flush | 0.5ms | **5.1ms** |

**Budget: 16.6ms — within target.**

---

## 7. Current State

### ✅ Complete (51 tasks)

| Area | Tasks | Status |
|------|-------|--------|
| Bridge FFI | 001–008 | All done |
| JS Integration | 009–012 | All done |
| Event Loop | 013–019 | All done |
| Yoga Layout | 020–024 | All done |
| ratatui Render | 025–028 | Box, Text, Static, Newline, Spacer done. backgroundColor, padding, underline, inverse done |
| Ink Hooks | 030–036 | All done (via runtime.js) |
| DevEx | 037–040 | Hot reload, bytecode, feature flags done |
| JS Examples | 041–050 | All 10 JS examples done |
| TS Examples | counter.ts–mouse-app.ts | All 10 TS examples done |
| Parity | 051–052 | Harness and diff scripts done |

### ⚠️ Partial (1 task)

| Task | Status | Note |
|------|--------|------|
| 029 Buffer Diff | Partial | Basic flush works via ratatui double-buffering. Full cell-level diff not implemented. |

---

## 8. Remaining Work

### Optional Enhancements

**Buffer Diff (Task 029):**
- Basic flush works via ratatui's native double-buffering
- Full cell-level diff not implemented (always redraws entire screen)

**Key/Mouse Direct Dispatch:**
- Current: 1 ctx.eval per event
- Future: Store Function refs, call directly
- Impact: ~0.5ms → ~0.05ms per event

**Border Colors:**
- `borderColor` not implemented
- `borderDimColor` not implemented
- `borderTop`, `borderBottom`, `borderLeft`, `borderRight` not implemented

**Flex Props:**
- `minWidth`, `maxWidth`, `minHeight`, `maxHeight` not implemented
- `flexBasis`, `flexGrow`, `flexShrink` from props not implemented

---

## 9. Running Examples

### JavaScript (no transpile needed)
```bash
tuibridge examples/counter.js
tuibridge examples/dashboard.js
```

### TypeScript (transpile first)
```bash
npx esbuild examples/counter.ts --bundle --outfile=dist/counter.js \
  --external:ink --jsx-factory=createElement --jsx-fragment=Fragment
tuibridge dist/counter.js
```

### With hot reload
```bash
tuibridge --watch examples/counter.js
# Or
tuibridge --hot examples/counter.js
```

### Parity harness
```bash
./scripts/parity.sh
```

---

## 10. Examples Matrix

| Task | Example | JS | TS | Status |
|------|---------|----|----|--------|
| 041 | Counter | ✅ `counter.js` | ✅ `counter.ts` | Done |
| 042 | Todo List | ✅ `todo-list.js` | ✅ `todo-list.ts` | Done |
| 043 | Focus Form | ✅ `focus-form.js` | ✅ `focus-form.ts` | Done |
| 044 | Dashboard | ✅ `dashboard.js` | ✅ `dashboard.ts` | Done |
| 045 | File Tree | ✅ `file-tree.js` | ✅ `file-tree.ts` | Done |
| 046 | Log Viewer | ✅ `log-viewer.js` | ✅ `log-viewer.ts` | Done |
| 047 | Spinner | ✅ `spinner.js` | ✅ `spinner.ts` | Done |
| 048 | Tabs | ✅ `tabs.js` | ✅ `tabs.ts` | Done |
| 049 | Chat UI | ✅ `chat-ui.js` | ✅ `chat-ui.ts` | Done |
| 050 | Mouse App | ✅ `mouse-app.js` | ✅ `mouse-app.ts` | Done |

**All 10 JS examples: DONE**  
**All 10 TS examples: DONE**
