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
│                   RUNTIME.JS (~1060 lines JS)                │
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
│  │  src/main.rs         192 lines  Event loop entry    │   │
│  │  src/event_loop.rs   289 lines  sync poll          │   │
│  │  src/render.rs       374 lines  ratatui rendering  │   │
│  │  src/bridge/         476 lines  FFI, timers, I/O    │   │
│  │  src/ink/            919 lines  Yoga tree, layout │   │
│  │  src/hotreload.rs    196 lines  File watching      │   │
│  │  src/ink_js.rs        52 lines  Constants          │   │
│  │  src/cli.rs           200 lines  CLI args          │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

**Design principle:** Reconciler in JS (runs on state change), all hot paths in Rust (runs every frame).

---

## 2. Stack

| Layer | Technology | Lines | Purpose |
|-------|------------|-------|---------|
| User code | TS/TSX | - | Ink-compatible API |
| Transpile | esbuild OR swc (optional) | - | TSX → JS, JSX → createElement |
| Reconciler | JS (runtime.js) | ~1060 | Hooks, component lifecycle, tree diff |
| Bridge | JS→Rust FFI | - | `__ink_call(method, args)` |
| **Runtime** | **Rust** | **~4900** | **Tree, layout, render, timers, I/O, hot reload** |

**Total Rust:** ~4,900 lines
**Total JS (runtime):** ~1,300 lines
**Ratio:** ~78% Rust, ~22% JS

---

## 3. Rust Modules

```
src/
├── main.rs           192 lines  # Entry point, minimal
├── event_loop.rs     289 lines  # Event loop + hot reload
├── render.rs         380 lines  # ratatui rendering
├── cli.rs            200 lines  # CLI argument parsing
├── hotreload.rs      196 lines  # File watching, remount cycle
├── bridge_config.rs  217 lines  # Platform detection, useBridge()
├── compat.rs         166 lines  # Prop validation, partial support warnings
├── ink_js.rs          52 lines  # Constants registration
├── bridge/                    # FFI bridge (modular)
│   ├── mod.rs          58 lines  # Module exports
│   ├── ffi.rs        376 lines  # __ink_call FFI
│   ├── io.rs         105 lines  # stdout/stderr/exit
│   ├── node.rs       383 lines  # Node creation/updates
│   ├── props.rs      314 lines  # Props parsing
│   ├── timers.rs     174 lines  # Timer registry
│   └── tree.rs       173 lines  # Tree mutation
├── ink/                      # Yoga tree (modular)
│   ├── mod.rs          21 lines  # Module exports
│   ├── node.rs        390 lines  # InkNode + Yoga layout + gap
│   ├── runtime.rs     190 lines  # InkRuntime state
│   ├── tree.rs        159 lines  # Tree operations
│   └── shared.rs       18 lines  # Shared types
└── compiler/                  # Optional TSX compiler (feature "compiler")
    ├── mod.rs         143 lines  # Module exports
    ├── jsx.rs        497 lines  # JSX → createElement
    └── shim.rs       208 lines  # Import removal

build.rs            # Bytecode precompilation + lint rules (warning-only)
scripts/
├── parity.sh       # Side-by-side Deno/TuiBridge runner
└── ansi-diff.js    # ANSI output comparison
```

---

## 4. JS Runtime (runtime.js)

The reconciler lives in JS because:
1. **rquickjs Function references** - callbacks, component functions
2. **Hook state** - per-instance hook arrays, React rules
3. **Tree diffing** - DOM-like reconciliation algorithm

**NOT on the hot path.** The reconciler only runs when:
- Initial render
- State changes (setCount, setState)
- Props change

**Hot path (every frame) is in Rust:**
- Event loop: synchronous `crossterm::event::poll` in src/event_loop.rs (see Task 077 for async migration)
- Layout: Yoga in src/ink/
- Rendering: ratatui in src/render.rs

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

**Key/mouse dispatch:** Single `ctx.eval()` per event calling JS `__tb_dispatch_key`/`__tb_dispatch_mouse`. Key names mapped to Ink format (`q`, `upArrow`, `f1`-`f12`, `return`, `escape`). Meta key (Cmd/Super) detected. No per-handler string building.

---

## 5. Event Loop (Rust)

Event loop is in `src/event_loop.rs` using synchronous `crossterm::event::poll`.

```rust
// src/event_loop.rs
pub fn run_event_loop(...) -> Result<()> {
    loop {
        // Poll for terminal events
        if let Ok(true) = crossterm::event::poll(Duration::from_millis(10)) {
            if let Ok(event) = crossterm::event::read() {
                handle_event(event)?;
            }
        }
        
        // Poll timers and microtasks
        poll_timers()?;
        
        // Render if dirty
        if bridge::__ink_is_dirty() {
            render_tree(terminal, root_id)?;
        }
    }
}
```

---

## 6. Performance

### Benchmark Results (2026-06-10)

| Operation | Target | Achieved | Status |
|-----------|--------|----------|--------|
| Layout (10 boxes × 5 texts) | < 2ms | **~62µs** | ✅ |
| Tree creation (2200 nodes) | < 5ms | **~490µs** | ✅ |
| Prop updates (50 nodes) | < 3ms | **~808µs** | ✅ |
| Binary size | < 5 MB | **2.0 MB** | ✅ |
| Startup time | < 100ms | **~5ms** | ✅ |

### Hot Path Performance

| Component | Path | Latency | Status |
|-----------|------|---------|--------|
| Layout | Rust + Yoga | ~62µs | ✅ |
| Rendering | Rust + ratatui | ~1ms | ✅ |
| Reconciler | JS (runtime.js) | ~2ms | ✅ (on state change) |
| Timer dispatch | Batch eval + Function.call | ~0.1ms | ✅ (Task 055) |
| **Key dispatch** | Single eval → JS Map | **~0.5ms** | ✅ Ink key names mapped |
| **Mouse dispatch** | Single eval → JS Map | **~0.5ms** | ✅ Hit-tested dispatch |

### 60fps Budget

| Operation | Time | Cumulative |
|-----------|------|------------|
| Key event | 0.5ms | 0.5ms |
| State update | 0.1ms | 0.6ms |
| Re-render (JS) | 2ms | 2.6ms |
| Yoga layout | 1ms | 3.6ms |
| ratatui render | 1ms | 4.6ms |
| Buffer flush | 0.5ms | **5.1ms** |

**Budget: 16.6ms - within target.**

### Cursor Handling
Cursor is hidden once at startup (`terminal.hide_cursor()`) and restored on exit. No per-frame cursor hide/show (eliminates flicker and reduces I/O).

---

## 7. Current State

### Task Overview (85 tasks: 65 done, 2 partial, 16 pending, 2 deferred)

| Area | Tasks | Status |
|------|-------|--------|
| Bridge FFI | 001-008 | All done |
| JS Integration | 009-012 | All done |
| Event Loop | 013-019 | All done |
| Yoga Layout | 020-024 | All done |
| ratatui Render | 025-029 | Box, Text, Static, Newline, Spacer done. backgroundColor, padding, underline, inverse done. ratatui double-buffering handles diff |
| Optimizations | 053-057 | Hot-path batching done. Reconciler stays in JS. Render parity gaps closed. |
| Ink Hooks | 030-036 | All done (via runtime.js) |
| DevEx | 037-040 | File watcher ✅, bytecode ✅, feature flags ✅. **Hot reload broken** — see Task 072. |
| JS Examples | 041-050 | All 10 JS examples done |
| TSX Examples | 041-050 | All 10 primary TSX examples done |
| Parity | 051-052 | Harness and diff scripts done |
| Compatibility | 059-067 | Validation, coverage, props done. React reconciler (063) and esbuild pipeline (065) deferred. |
| Ink Props | 066-067 | gap/small/title + remaining Ink props (alignSelf, alignContent, position, hooks) — done |
| Remaining Gaps | 068-071 | 3 pending (068-070): border colors, renderToString, overflow/aspectRatio. 071 (API audit) done. |
| Code Quality | 058 | 🟡 Linter rules in `build.rs` (warning-only). Refactor required to enforce. |

---

## 8. Remaining Work

### Ink API Gaps (Tasks 068-070)

1. **Task 068: Individual Border Colors** - `borderTopColor`, `borderBottomColor`, etc. (ratatui limitation)
2. **Task 069: renderToString** - Synchronous string rendering without terminal I/O
3. **Task 070: overflow/aspectRatio** - Content clipping and proportional sizing

### Completed Enhancements (no longer remaining)

**Buffer Diff (Task 029):**
- ✅ ratatui's native double-buffering handles cell-level diff
- Cursor hidden during draw, restored on exit

**Color Parsing:**
- ✅ Named colors (`red`, `brightRed`, `gray`, etc.)
- ✅ Hex colors (`#rrggbb`, `#rgb`)

**Border Colors & Sides:**
- ✅ `borderColor` sets border foreground
- ✅ `borderDimColor` applies `Modifier::DIM` to borders
- ✅ `borderTop`, `borderBottom`, `borderLeft`, `borderRight` boolean props

**Text Transform:**
- ✅ `transform` (`uppercase` / `lowercase`)

**Flex Props:**
- ✅ `minWidth`, `maxWidth`, `minHeight`, `maxHeight` (number + %)
- ✅ `flexBasis`, `flexGrow`, `flexShrink` from props
- ✅ `gap`, `gapX`, `gapY` for flex gap spacing
- ✅ `columnGap`, `rowGap` (Ink 7 aliases)
- ✅ `alignSelf` for child alignment override
- ✅ `alignContent` for multi-line alignment

**Layout Accuracy:**
- ✅ `calculate_layout` uses terminal dimensions (was hardcoded 512×512)
- ✅ Float→cell: `round()` positions, `ceil()` dimensions

**Code Quality (Task 058):**
- `build.rs` lints Rust sources for file length (≤500 lines), function length (≤40 lines), and complexity (≤10)
- File length enforced globally; function length/complexity warning-only
- **Progress:** `src/compiler/` module fully compliant (panic on violation)
- 2 clippy warnings remain in `build.rs` itself (Task 083)

---

## 9. Running Examples

### TSX (recommended)
```bash
tuibridge examples/counter.tsx
tuibridge examples/dashboard.tsx
```

### Legacy JavaScript
```bash
tuibridge examples/counter.js
tuibridge examples/dashboard.js
```

### With hot reload
```bash
tuibridge --watch examples/counter.tsx
# Or
tuibridge --hot examples/counter.tsx
```

### Parity harness
```bash
./scripts/parity.sh
```

---

## 10. Examples Matrix

### Primary (TSX) — Recommended
All new examples should be written in TSX for full type safety.

| Task | Example | Hooks | Props | Status |
|------|---------|-------|-------|--------|
| 041 | `counter.tsx` | useState, useEffect, useInput | flexDirection, borderStyle, padding | ✅ |
| 042 | `todo-list.tsx` | useState, useInput | flexDirection, children | ✅ |
| 043 | `focus-form.tsx` | useFocus, useFocusManager | focus styling | ✅ |
| 044 | `dashboard.tsx` | useEffect, setInterval | borders, stats | ✅ |
| 045 | `file-tree.tsx` | useState, useInput | backgroundColor, recursive | ✅ |
| 046 | `log-viewer.tsx` | useEffect, useRef | scrolling | ✅ |
| 047 | `spinner.tsx` | useEffect, useState | animation | ✅ |
| 048 | `tabs.tsx` | useState | conditional render | ✅ |
| 049 | `chat-ui.tsx` | useState, useInput | split pane | ✅ |
| 050 | `mouse-app.tsx` | useInput | mouse events | ✅ |

### Extended (TSX) — API Coverage
Additional examples demonstrating specific API features.

| Example | Coverage | Status |
|---------|----------|--------|
| `border-styles.tsx` | borderColor, borderDimColor, individual sides, title | ✅ |
| `context-demo.tsx` | createContext, useContext | ✅ |
| `focus-manager.tsx` | useFocus, useFocusManager | ✅ |
| `measure-ref.tsx` | useRef, measureElement | ✅ |
| `sizing-constraints.tsx` | min/max, position, display | ✅ |
| `spacing-props.tsx` | margin/padding/gap variants | ✅ |
| `static-overlay.tsx` | Static, Newline, Spacer | ✅ |
| `stdin-stdout.tsx` | useStdin, useStdout, useStderr | ✅ |
| `use-bridge.tsx` | useBridge (TuiBridge-specific) | ✅ |
| `wizard.tsx` | useMemo, useCallback | ✅ |
| `animations.tsx` | useEffect with intervals, animations, small text | ✅ |
| `component-composition.tsx` | component patterns, composition | ✅ |
| `confirm-prompt.tsx` | yes/no dialog, focus navigation | ✅ |
| `flex-layouts.tsx` | alignItems, justifyContent, flexWrap | ✅ |
| `progress-bar.tsx` | progress visualization | ✅ |
| `scroll-view.tsx` | keyboard scroll, list navigation | ✅ |
| `select-input.tsx` | select/dropdown pattern | ✅ |
| `table-demo.tsx` | tabular data, column layout | ✅ |
| `terminal-resize.tsx` | useStdout, terminal dimensions | ✅ |
| `text-styles.tsx` | bold, italic, strikethrough, underline, inverse, transform | ✅ |

### Advanced (TSX) — Real-World Patterns
Real-world examples based on Ink community usage.

| Example | Coverage | Status |
|---------|----------|--------|
| `align-demo.tsx` | alignSelf override, child alignment | ✅ |
| `flex-basis-demo.tsx` | flexBasis, flexGrow interaction | ✅ |
| `text-wrap-demo.tsx` | textWrap modes, truncation | ✅ |
| `transform-demo.tsx` | text transformation, ANSI codes | ✅ |
| `form-validation.tsx` | form handling, validation patterns | ✅ |
| `multi-select.tsx` | checkbox selection, multi-choice | ✅ |
| `realtime-dashboard.tsx` | live data, real-time updates | ✅ |
| `loading-states.tsx` | spinners, progress bars, skeletons | ✅ |

### Legacy (JS) — Reference Only
Original examples kept for compatibility reference.

| Example | Purpose | Status |
|---------|---------|--------|
| `*.js` | Legacy JS examples | ✅ (reference) |
| `text-styles.js` | Text props reference | ✅ |
| `text-wrap.js` | textWrap reference | ✅ |
| `flex-layouts.js` | Flexbox reference | ✅ |
| `simple-hello.js` | Minimal example | ✅ |
| `stdin-stdout.js` | I/O hooks reference | ✅ |

**All 10 primary TSX examples: DONE**
**All extended TSX examples: DONE**
**All advanced TSX examples: DONE**
**39 TSX examples total, 100% core Ink API coverage achieved**

---

## Done Definition

### Verification Status (as of 2026-06-10)

| Criteria | Status | Notes |
|----------|--------|-------|
| All tasks in `tasks/` complete | 🟡 | **85 tasks**, 65 "done", 2 "partial", 16 "pending", 2 "deferred" |
| Tests passing | ✅ | Tests in bridge/, ink/, compat.rs, hotreload.rs |
| Examples run without modification | ✅ | JS + TSX examples work |
| Release binary < 5 MB | ✅ | **2.0 MB** (under target) |
| Rust/JS ratio | ✅ | **78% Rust, 22% JS** |
| Linter compliance | 🟡 | All files under 500 lines; function length/complexity warning-only; 2 build.rs clippy warnings (Task 083) |
| Hot reload | 🔴 | **BROKEN** — Task 072. New context never gets `setup_runtime()`. |
| TSX compiler | ✅ | `--compile` and `--run` flags |
| `cargo test` | ✅ | Tests passing |
| clippy | 🟡 | 0 warnings in library, 2 warnings in `build.rs` — Task 083 |
| Binary size | ✅ | 2.0 MB release binary |

### Remaining Gaps (Pending Tasks)

All pending tasks are documented in the tasks directory. The 16 pending tasks fall into three categories:

1. **Ink API Gaps** (068-070): border colors, renderToString, overflow/aspectRatio
2. **Post-Review Critical Bugs** (072-075): hot reload, JSON parser, terminal cleanup, render FFI
3. **Post-Review Improvements** (076-084): event dispatch, async loop, storage, sandbox, cleanup, polish

## 11. Post-Review Remediation (Tasks 072-084)

An architecture and code review (2026-06-10) identified critical bugs and significant improvements. These are tracked in new tasks:

### 🔴 Critical Bugs (P0)

| Task | Issue | Impact |
|------|-------|--------|
| **072** | Hot reload creates new rquickjs Context without `setup_runtime()` | Hot reload silently does nothing |
| **073** | Custom 180-line JSON parser instead of `serde_json` | Fragile, unnecessary, already a dependency |
| **074** | `process::exit(0)` bypasses terminal cleanup | Terminal stays in raw mode on panic/error |
| **075** | Renderer does 250+ FFI calls per frame for prop queries | Major performance overhead at scale |

### 🟡 Significant Improvements (P1)

| Task | Issue | Impact |
|------|-------|--------|
| **076** | Key/mouse dispatch uses string `eval()` per event | ~10x speedup with `rquickjs::Function` refs |
| **077** | Event loop is synchronous despite tokio dependency | Wastes CPU, poor timer accuracy |
| **078** | Node storage uses sparse `Vec` instead of `HashMap` | O(n) growth, memory waste |
| **079** | No rquickjs memory/stack limits | Malicious scripts can crash process |
| **080** | Yoga C++ node memory cleanup unverified | Potential memory leak on tree destroy |

### 🟠 Polish (P2)

| Task | Issue | Impact |
|------|-------|--------|
| **081** | `render.rs` uses JSON-string props instead of `PropValue` | String alloc + trim overhead |
| **082** | `fill_background()` manually iterates cells | Redundant — ratatui Block handles this |
| **083** | Dead CLI match arm, unused `#[allow]`, build.rs warnings | Code hygiene |
| **084** | JS errors swallowed by `tracing::error!` | Users see blank screen with no error message |

### Optional Enhancements (Deferred)

1. **PTY for Parity** - `scripts/parity.sh` exists, needs proper TTY emulation
2. **Hot Reload Benchmark** - Currently broken (Task 072); benchmark after fix
3. **Visual Verification** - Run in tmux to verify 100% look&feel parity
4. **React Reconciler Bridge (063)** - Optional, for full React app support
