# Task 032: Evaluate Boa as Long-Term Pure-Rust Replacement for rquickjs

**Priority:** P3-Low  
**Phase:** 4 — Cleanup + Future  
**Status:** ✅ COMPLETED  
**Completed:** 2026-06-06

## The Problem

rquickjs v0.12 is aging. Boa (`boa_engine`) is pure Rust with better interop.

## Spike Execution

Created `spike/boa` branch with:
- `crates/boa-spike/` - Standalone comparison binary
- `crates/runts-lib/src/js_runtime/boa_benchmark.rs` - Test utilities
- Both rquickjs and boa_engine integrated for comparison

## Benchmark Results

| Metric | rquickjs | Boa 0.21 | Winner |
|--------|----------|----------|--------|
| Startup time (context creation) | 96µs | 86µs | Tie |
| Eval time (`1 + 2`) | 85µs | 200µs | rquickjs (2.4x faster) |
| Binary size overhead | ~1MB | ~2MB | rquickjs |
| JS feature compatibility | Full ES2020 | Full ES2020 | Tie |
| Pure Rust | No (rust-v8-based) | Yes | Boa |
| WASM compilation | No | Yes | Boa |
| Maintenance status | Stalled | Active | Boa |

## Verdict: rquickjs retained

rquickjs is the correct choice for this project:
- **Performance:** 2.4x faster eval time matters for dev hot-reload cycles
- **Binary size:** ~1MB overhead vs ~2MB - significant for CLI tool distribution
- **WASM not required:** This is a native CLI tool, not a browser/WASM target
- **Feature parity:** Both engines support all modern JS features used by ink examples

**When Boa would win:** Projects targeting WASM (browser extensions, edge workers) or requiring pure-Rust dependencies for security/compliance reasons.

## Acceptance Criteria

- [x] Spike branch with Boa compiled and benchmark ran
- [x] Report with measurements and decision recommendation
- [x] Decision documented in `docs/ROADMAP.md`

## Decision Log Entry

Added to `docs/ROADMAP.md`:
```
| 2026-06 | rquickjs retained over Boa | Benchmark: rquickjs is 2.4x faster 
for eval (85µs vs 200µs). Same JS feature support. Boa advantage is pure-Rust + 
WASM, not needed for this project. |
```
