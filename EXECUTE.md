# TuiBridge — Execution Plan

> **Goal:** Build TuiBridge — an Ink-compatible TUI framework where JSX/hooks run in rquickjs (~200 KB) and Yoga + ratatui handle layout/rendering in native Rust. 10+ Ink examples run identically in both Deno (reference Ink) and TuiBridge with **100% look&feel parity**.

---

## Current Stats

| Metric | Value |
|--------|-------|
| Total Tasks | 52 |
| Phases | 9 |
| Examples | 10 |
| Parity Gate | 100% ANSI cell match |

---

## Architecture Summary

```
Ink API (exact) → React Reconciler → Host Config → __ink_* FFI → Rust
                                                    ↓
                              Yoga (layout) → ratatui Buffer (render)
                                                    ↑
                              Crossterm Events ← tokio::select!
```

- **JS side:** React + react-reconciler + ~15 KB shim. Runs in rquickjs.
- **Rust side:** Yoga C++ for layout, ratatui for terminal output, crossterm for input.
- **FFI:** 16 synchronous functions. One `commit()` per reconciler flush.

---

## Phase 1: Foundation (Tasks 001–008)

**Goal:** Cargo workspace, FFI bridge, and tree mutation primitives.

| # | Task | AC Summary | Depends On |
|---|------|-----------|------------|
| 001 | Project Scaffold | `cargo check/test/clippy` pass | — |
| 002 | FFI Root Node | `create_root` / `destroy_root` with Yoga | 001 |
| 003 | FFI Create Nodes | `create_node(tag,props)` / `create_text_node(text)` | 002 |
| 004 | FFI Tree Mutation | `append_child` / `remove_child` / `insert_before` | 003 |
| 005 | FFI Commit Pipeline | `commit_update` / `set_text` / `commit` trigger | 004 |
| 006 | FFI Text Measure | `measure_text` (unicode-width+textwrap) / `measure_element` | 003 |
| 007 | FFI I/O & Exit | stdout/stderr write, raw mode query, exit flag | 001 |
| 008 | FFI Input Handlers | register/unregister keyboard & mouse callbacks | 001 |

**Phase 1 Exit Criteria:**
- [ ] All FFI functions callable from rquickjs with unit tests.
- [ ] Yoga nodes created and mutated without leaks.
- [ ] Text measurement returns correct dimensions for multi-byte text.

---

## Phase 2: JS Reconciler Shim (Tasks 009–012)

**Goal:** React reconciler host config targeting FFI, plus Ink API surface.

| # | Task | AC Summary | Depends On |
|---|------|-----------|------------|
| 009 | JS Host Config | All reconciler ops map to `__ink_*` | 004, 005 |
| 010 | JS render() API | `render(node, opts)` → `{waitUntilExit, unmount}` | 009, 002 |
| 011 | JS Components | `Box`, `Text`, `Static`, `Newline`, `Spacer` as string tags | 009 |
| 012 | JS Hook Shims | `useInput` and `useApp` bridging to FFI | 008, 007 |

**Phase 2 Exit Criteria:**
- [ ] `<Box><Text>hello</Text></Box>` mounts from JS into Rust Yoga tree.
- [ ] `render()` + `unmount()` cycle completes without panic.
- [ ] Hook mocks verify FFI registration/unregistration.

---

## Phase 3: Event Loop & Polyfills (Tasks 013–019)

**Goal:** tokio-driven event loop with crossterm, timers, and Node.js polyfills.

| # | Task | AC Summary | Depends On |
|---|------|-----------|------------|
| 013 | Event Loop | `tokio::select!` over crossterm / timers / reload | 001 |
| 014 | Keyboard Dispatch | Serialize KeyEvent → JS callback | 008, 013 |
| 015 | Mouse Dispatch | Hit-test + dispatch to deepest matching node | 008, 013 |
| 016 | Terminal Resize | Update Yoga root on Resize event | 002, 013 |
| 017 | Timer Polyfills | `setTimeout` / `setInterval` / `clearTimeout` → tokio | 013 |
| 018 | Microtask Polyfills | `setImmediate` / `process.nextTick` queue | 013 |
| 019 | Console Polyfill | `console.log/error/warn` → `tracing` | 001 |

**Phase 3 Exit Criteria:**
- [ ] Key press in terminal reaches JS handler and updates state.
- [ ] `setTimeout` callback fires exactly once; `setInterval` fires N times.
- [ ] Microtasks execute before timers in each loop iteration.

---

## Phase 4: Yoga Layout Engine (Tasks 020–024)

**Goal:** Map all Ink flex/spacing/border props to Yoga and calculate layouts.

| # | Task | AC Summary | Depends On |
|---|------|-----------|------------|
| 020 | Yoga Flex Props | `flexDirection`, `justifyContent`, `alignItems`, `flexGrow` etc. | 003 |
| 021 | Yoga Spacing & Sizing | `padding`, `margin`, `gap`, `width`, `height`, min/max | 020 |
| 022 | Yoga Borders | `borderStyle`, `borderColor`, title — consume layout space | 021 |
| 023 | Yoga Text Measure | Register Rust measure func on Yoga text nodes | 006, 003 |
| 024 | Yoga Full Layout | `calculate_layout` on root; all nodes have computed rect | 023, 022, 005 |

**Phase 4 Exit Criteria:**
- [ ] Complex flex tree (20 nodes) layouts in < 2 ms.
- [ ] Text wrapping produces correct Yoga dimensions via Rust measure func.
- [ ] Border consumes inner content area (Box with border has smaller inner rect).

---

## Phase 5: ratatui Rendering (Tasks 025–029)

**Goal:** Render Yoga-computed tree to ratatui Buffer with zero JS in hot path.

| # | Task | AC Summary | Depends On |
|---|------|-----------|------------|
| 025 | Render Box | `Block` widget with borders, title, background color | 024, 003 |
| 026 | Render Text | `Paragraph` / `Span` with full style mapping | 025 |
| 027 | Render Static | Overlay layer rendered above main tree | 025 |
| 028 | Render Newline & Spacer | Line breaks and flex filler | 026 |
| 029 | Buffer Diff & Flush | Double-buffered terminal output; minimal ANSI diff | 025 |

**Phase 5 Exit Criteria:**
- [ ] 500-node tree renders in << 1 ms pure Rust.
- [ ] Static overlay appears above main tree.
- [ ] Two frames with single text change produce minimal ANSI diff.

---

## Phase 6: Ink Hooks & API (Tasks 030–036)

**Goal:** Full Ink hook parity: input, app context, stdio, focus, measurement.

| # | Task | AC Summary | Depends On |
|---|------|-----------|------------|
| 030 | useInput | Keyboard handler registration with `isActive` option | 012, 014 |
| 031 | useApp | `{exit, stdout, stdin, stderr}` proxies | 012, 007 |
| 032 | useStdin | `{isRawModeSupported, setRawMode, stdin}` | 031 |
| 033 | useStdout | `{write, columns, rows}` | 031 |
| 034 | useStderr | `{write}` | 031 |
| 035 | useFocus & useFocusManager | Tab order cycling, focus state | 030 |
| 036 | measureElement | `{width, height}` from Yoga computed layout | 006, 010 |

**Phase 6 Exit Criteria:**
- [ ] `useInput` example responds to space/q keys.
- [ ] Three focusable fields cycle with Tab/Shift+Tab.
- [ ] `measureElement(ref)` returns correct terminal cell dimensions.

---

## Phase 7: DevEx (Tasks 037–040)

**Goal:** Hot reload in < 50 ms, production bytecode builds.

| # | Task | AC Summary | Depends On |
|---|------|-----------|------------|
| 037 | File Watcher | `notify` + `esbuild --watch` detect plugin changes | 001 |
| 038 | Remount Cycle | Unmount/eval/remount in same VM < 50 ms | 010, 037, 002 |
| 039 | Bytecode Precompile | `qjsc` → `include_bytes!` for release | 010 |
| 040 | Strip Dev Code | `cfg(debug_assertions)` gates watcher/reload | 037, 039 |

**Phase 7 Exit Criteria:**
- [ ] Modify `.tsx` plugin → see TUI update in < 50 ms.
- [ ] Release binary < 5 MB, runs standalone without source files.

---

## Phase 8: Examples (Tasks 041–050)

**Goal:** 10 Ink examples covering the full API surface. Each runs in Deno (reference) and TuiBridge with 100% parity.

| # | Example | Ink APIs Covered |
|---|---------|-----------------|
| 041 | Counter | `useState`, `useInput`, `useApp`, `Box`, `Text`, `borderStyle`, `color`, `bold`, `dimColor` |
| 042 | Todo List | `useState`, `useInput`, nested flex row/column, keyboard nav |
| 043 | Focus Form | `useFocus`, `useFocusManager`, tab order, focus styles |
| 044 | Dashboard | `flexDirection="row"`, multi-pane, borders, titles, padding |
| 045 | File Tree | Recursive composition, nested padding, dynamic children, Enter key |
| 046 | Log Viewer | `Static`, `useEffect`, `setInterval`, scrolling-like behavior |
| 047 | Spinner | Rapid timer re-renders, dynamic `color`, conditional text |
| 048 | Tabs | `useState`, keyboard arrows, active highlight, conditional children |
| 049 | Chat UI | `useStdin`, `useStdout`, `Newline`, split pane, text input |
| 050 | Mouse App | `useMouse`, hit testing, click/hover, dynamic `backgroundColor` |

**Phase 8 Exit Criteria:**
- [ ] All 10 examples run successfully in TuiBridge.
- [ ] Each example source requires **zero changes** from standard Ink API.
- [ ] Deno reference runs captured for parity comparison.

---

## Phase 9: Parity (Tasks 051–052)

**Goal:** CI gate enforcing 100% look&feel match between Deno Ink and TuiBridge.

| # | Task | AC Summary | Depends On |
|---|------|-----------|------------|
| 051 | Side-by-Side Runner | Run each example in Deno + TuiBridge, capture ANSI | 041–050 |
| 052 | ANSI Diff & Match | Compare cell grids; zero tolerance for mismatch | 051 |

**Phase 9 Exit Criteria:**
- [ ] All 10 examples pass parity gate.
- [ ] Any single cell difference fails CI with visual diff.
- [ ] Parity harness runs in CI on every PR.

---

## Performance Targets

| Metric | Target | Verified By |
|--------|--------|-------------|
| JS memory | < 1 MB | rquickjs heap stats |
| Layout | < 2 ms | 20-node Yoga benchmark |
| Render | < 1 ms | 500-node ratatui benchmark |
| Commit (JS→Rust) | < 3 ms | Reconciler flush benchmark |
| Input latency | < 0.5 ms | Event loop timestamp diff |
| Hot reload | < 50 ms | End-to-end touch-to-render |
| Binary size | < 5 MB | `strip` + LTO release build |
| Idle CPU | 0% | `tokio::select!` blocking |

---

## Development Order (Recommended)

### Sprint 1: Bridge (Tasks 001–008)
Set up workspace. Implement all 16 FFI functions with unit tests. No React yet.

### Sprint 2: JS Shim (Tasks 009–012)
Build reconciler host config. Mount static JSX from JS into Rust tree.

### Sprint 3: Loop & Polyfills (Tasks 013–019)
Event loop, keyboard dispatch, timers, console. Counter app runs with keyboard input.

### Sprint 4: Layout (Tasks 020–024)
Yoga flex, spacing, borders, text measure. Complex layouts render correctly.

### Sprint 5: Render (Tasks 025–029)
ratatui widgets, Static overlay, buffer flush. Visual output matches Ink.

### Sprint 6: Hooks (Tasks 030–036)
useInput, useApp, useStdin/out/err, useFocus, measureElement. All 10 examples run.

### Sprint 7: DevEx (Tasks 037–040)
Hot reload, bytecode, release builds. Polish developer experience.

### Sprint 8: Parity (Tasks 051–052)
Build harness, fix divergences, lock 100% match CI gate.

---

## Testing Pyramid

```
        ┌─────────────┐
        │   Parity    │  10 examples, 100% ANSI match
        │  (Phase 9)  │
        ├─────────────┤
        │ Integration │  FFI round-trip, event loop,
        │  (Phases    │  render-to-buffer, keyboard
        │   3–6)      │
        ├─────────────┤
        │    Unit     │  Yoga props, text measure,
        │  (Phases    │  style mapping, diff algo
        │   1–2,4–5)  │
        └─────────────┘
```

**Unit tests:** Every FFI function, Yoga prop mapping, style parser.
**Integration tests:** Full render cycle, keyboard dispatch, timer callbacks.
**Parity tests:** Side-by-side Deno vs TuiBridge for all 10 examples.

---

## File Structure (Target)

```
tuibridge/
├── Cargo.toml
├── build.rs
├── src/
│   ├── main.rs
│   ├── vm.rs              # rquickjs runtime + bundle eval
│   ├── ffi.rs             # globalThis.__ink_* implementations
│   ├── tree.rs            # InkNode, ShadowTree, diff
│   ├── yoga.rs            # Yoga wrapper + text measure
│   ├── render.rs          # ratatui Buffer rendering
│   ├── events.rs          # crossterm → JS dispatch
│   ├── timers.rs          # setTimeout/setInterval bridge
│   └── polyfill.rs        # console, process, etc.
├── js/
│   ├── index.js           # ink shim entry
│   ├── host-config.js     # reconciler host config
│   ├── hooks.js           # useInput, useApp, etc.
│   └── components.js      # Box, Text, Static, etc.
├── examples/
│   ├── counter.tsx
│   ├── todo-list.tsx
│   ├── focus-form.tsx
│   ├── dashboard.tsx
│   ├── file-tree.tsx
│   ├── log-viewer.tsx
│   ├── spinner.tsx
│   ├── tabs.tsx
│   ├── chat-ui.tsx
│   └── mouse-app.tsx
├── scripts/
│   └── parity.sh          # side-by-side runner
├── tasks/
│   ├── index.json
│   └── 001-052.md
└── docs/
    └── SPEC.md
```

---

## Risk Register

| Risk | Mitigation |
|------|-----------|
| Yoga C++ bindings fail to build | Maintain `taffy` fallback branch |
| React reconciler too large for rquickjs | Tree-shake with esbuild; measure bundle size |
| ratatui style diverges from Ink ANSI | Parity harness catches early; fix in render.rs |
| Timer precision in tokio vs Node.js | Integration tests with deterministic fake time |
| Multi-byte text width mismatches | `unicode-width` roundtrip tests against Deno output |

---

## Done Definition

- [ ] All 52 tasks complete with tests passing.
- [ ] All 10 examples run in TuiBridge without source modification.
- [ ] Parity harness: 100% ANSI cell match against Deno Ink for all examples.
- [ ] Release binary < 5 MB, runs standalone.
- [ ] Hot reload < 50 ms end-to-end.
- [ ] CI green: `cargo test`, clippy, parity gate.
