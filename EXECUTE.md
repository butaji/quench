# TuiBridge — Execution Plan

> **Goal:** Exact Ink API running in rquickjs + Rust. 10+ examples run identically in Deno (reference Ink) and TuiBridge with **100% look&feel parity**.

---

## Architecture

```
Ink API (exact) → React Reconciler → Host Config → __ink_* FFI → Rust
                                                    ↓
                              Yoga (layout) → ratatui Buffer (render)
                                                    ↑
                              Crossterm Events ← tokio::select!
```

- **JS:** React + react-reconciler + ~15 KB shim in rquickjs
- **Rust:** Yoga C++ for layout, ratatui for terminal output, crossterm for input
- **FFI:** 16 synchronous functions. One `commit()` per reconciler flush.

---

## Important Goals

### 1. Exact Ink API
Users write standard Ink code — same imports, same hooks, same JSX. No porting. The shim intercepts React's host config at the FFI boundary and replaces the Node.js backend (streams, yoga-layout NAPI, `process.stdout`) with Rust.

### 2. Zero JS in Render Hot Path
`terminal.draw()` is pure Rust. JS is idle during rendering. Yoga computes layouts; ratatui writes to a double-buffered terminal buffer. For a 500-node tree: **< 1 ms**.

### 3. 100% Parity Gate
Every example runs side-by-side in Deno (real Ink) and TuiBridge. ANSI output is compared cell-by-cell. Zero tolerance for mismatch. This is a CI gate, not a suggestion.

### 4. Fast Feedback Loop
Hot reload unmounts/remounts the React root in the same rquickjs VM — no restart. Target: **< 50 ms** from file save to updated TUI.

### 5. Production Binary
Release build embeds QuickJS bytecode via `include_bytes!`. No esbuild, no file watcher, no source files. Target: **< 5 MB** stripped binary.

---

## How to Read This Plan

- **`docs/SPEC.md`** — architecture specification (why things work the way they do)
- **`tasks/`** — 52 implementation tasks with acceptance criteria, tests, and dependencies
- **`tasks/index.json`** — phase map and task metadata
- **`examples/`** — 10+ Ink examples that serve as the parity benchmark

---

## Performance Targets

| Metric | Target |
|--------|--------|
| Layout | < 2 ms |
| Render | < 1 ms |
| Commit (JS→Rust) | < 3 ms |
| Input latency | < 0.5 ms |
| Hot reload | < 50 ms |
| Binary size | < 5 MB |
| Idle CPU | 0% |

---

## Testing Strategy

- **Unit:** Every FFI function, Yoga prop mapping, style parser, text measurement
- **Integration:** Full render cycle, keyboard dispatch, timer callbacks, buffer diff
- **Parity:** Side-by-side Deno vs TuiBridge for all examples; cell-level ANSI comparison

---

## Done Definition

- [ ] All tasks in `tasks/` complete with tests passing
- [ ] All examples run in TuiBridge without source modification
- [ ] Parity harness: 100% ANSI cell match against Deno Ink for every example
- [ ] Release binary < 5 MB, runs standalone
- [ ] Hot reload < 50 ms end-to-end
- [ ] CI green: `cargo test`, clippy, parity gate
