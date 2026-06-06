# runts — Performance Targets & Trade-offs

> **Version:** 0.5.0  
> **Goal:** Prioritize correctness and Ink parity first, then ruthlessly optimize.

---

## 1. Performance Targets

### 1.1 Production Binary

| Metric | Target | v0.5 Status | Methodology |
|--------|--------|-------------|-------------|
| **Binary size** | < 2 MB | ~2.6 MB | `cargo build --release`, `strip`, `du -h` |
| **Memory (baseline RSS)** | < 3 MB | ~2.8 MB | `ps -o rss= -p <pid>` at idle |
| **Cold start** | < 5 ms | < 10 ms | Time from process start to first HTTP response |
| **Hot request latency (p50)** | < 0.5 ms | < 1 ms | SSR of a simple page, warmed up |
| **Hot request latency (p99)** | < 2 ms | < 5 ms | Same, 99th percentile |
| **SSR throughput** | > 50k req/s | ~15k req/s | `wrk -t12 -c400 -d30s` on simple page |
| **Max concurrent connections** | > 10k | > 10k | Limited by OS file descriptors |

### 1.2 Development Mode

| Metric | Target | v0.5 Status | Methodology |
|--------|--------|-------------|-------------|
| **Hot reload latency** | < 100 ms | ~50 ms | Time from `Ctrl+S` to visible update |
| **Initial dev server start** | < 3 s | ~2 s | `runts dev` until first request served |
| **TSX parse speed** | > 1k files/s | ~2k files/s | Parse all files in example project |
| **rquickjs startup** | < 100 ms | ~50 ms | Context creation + bundle eval |

### 1.3 Client Runtime (Browser)

| Metric | Target | v0.5 Status | Methodology |
|--------|--------|-------------|-------------|
| **Client runtime size** | < 5 KB gzipped | ~4.2 KB | `gzip -c runtime.ts | wc -c` |
| **Hydration start delay** | < 16 ms | < 10 ms | `requestAnimationFrame` to first island hydrate |
| **Visible island TTI** | < 50 ms | < 30 ms | IntersectionObserver trigger to interactive |

---

## 2. Benchmarks

### 2.1 Micro-Benchmarks (run `cargo bench`)

```rust
// Benchmark: SSR a simple page
test ssr_simple_page     ... bench:      12,450 ns/iter (+/- 320)

// Benchmark: SSR with islands
test ssr_island_page     ... bench:      28,100 ns/iter (+/- 890)

// Benchmark: Signal get/set
test signal_get_set      ... bench:          45 ns/iter (+/- 2)

// Benchmark: Hook invocation
test hook_use_state      ... bench:         120 ns/iter (+/- 5)

// Benchmark: Route pattern match
test route_match_static  ... bench:         180 ns/iter (+/- 8)
test route_match_dynamic ... bench:         420 ns/iter (+/- 15)

// Benchmark: TSX parse (simple app.tsx)
test parse_app_tsx     ... bench:       1,200 ns/iter (+/- 40)
```

### 2.2 Macro-Benchmarks (`examples/my-blog`)

```bash
# Production build
cd examples/my-blog
runts build --release

# Binary size
ls -lh target/release/my-blog  # 2.6 MB

# Benchmark with wrk
wrk -t12 -c400 -d30s http://localhost:8000/
# v0.5 results:
#   Requests/sec:  15,230
#   Latency:       26.18ms (p50), 89.45ms (p99)

# Memory at idle
ps -o rss= -p $(pgrep my-blog)  # 2860 KB
```

---

## 3. Trade-off Decisions

### 3.1 Parser: Custom Recursive Descent vs swc/oxc

| Dimension | Custom Parser (v0.5) | swc/oxc (v1.0) |
|-----------|---------------------|----------------|
| **Compile time** | Instant | ~5-10s extra build |
| **Binary size** | +0 KB | +2-5 MB |
| **Spec coverage** | Supported subset only | Full TypeScript |
| **Maintenance** | High (we maintain) | Low (upstream) |
| **Error messages** | Excellent (custom) | Good (generic) |
| **Performance** | Fast for subset | Faster for all TS |

**Decision:** Custom parser for v0.5-v0.8. Migrate to `oxc_parser` for v1.0.

### 3.2 Dev Mode: rquickjs vs HIR Interpreter vs Cranelift JIT

| Dimension | rquickjs (chosen) | HIR Interpreter (removed) | Cranelift JIT |
|-----------|-------------------|--------------------------|---------------|
| **Reload speed** | ~50 ms | ~20 ms | ~100-500 ms |
| **JS parity** | 100% | ~40% (incomplete) | 100% |
| **Startup** | ~50 ms | ~20 ms | ~200 ms |
| **Binary size** | +~1MB | +0 | +~5MB |
| **Maintenance** | Low (upstream) | Infinite (custom JS engine) | Medium |
| **Interop** | C bindings | Native Rust | Native Rust |

**Decision:** rquickjs. HIR interpreter was becoming a custom JS engine (3k LOC, 171 complexity). Removed. rquickjs gives full JS semantics with minimal custom code.

### 3.3 Layout Engine: Yoga vs Taffy

| Dimension | Yoga (chosen) | Taffy (removed) |
|-----------|--------------|-----------------|
| **Ink parity** | 100% (same engine) | ~95% (slight differences) |
| **Language** | C++ | Pure Rust |
| **Binary size** | +~500KB | +~300KB |
| **Maintenance** | Low (upstream) | Low (upstream) |
| **Build complexity** | Medium (bindgen) | Low |

**Decision:** Yoga. Same engine Ink uses internally. Taffy removed to maximize layout parity and reduce divergence surface.

### 3.4 Reactivity: Signals vs VDOM Diffing

| Dimension | Signals (chosen) | Full VDOM Diff |
|-----------|-----------------|----------------|
| **Update cost** | O(1) per signal | O(tree size) |
| **Memory** | Low (no VDOM retained) | High (VDOM tree) |
| **SSR speed** | Fast (string building) | Fast (string building) |
| **Mental model** | Fine-grained | Batch + diff |
| **Fresh compat** | Preact Signals native | Preact classic |

**Decision:** Signals for islands, VNode tree for SSR composition. Hybrid gives best of both.

### 3.5 Client Runtime: Vanilla JS vs Preact

| Dimension | Vanilla JS (chosen) | Preact (alt) |
|-----------|---------------------|--------------|
| **Runtime size** | ~4 KB | ~10 KB |
| **Feature parity** | Supported subset only | Full Preact |
| **Maintenance** | High (we maintain) | Low (upstream) |
| **Performance** | Faster (less abstraction) | Fast enough |
| **Ecosystem** | None | Full Preact ecosystem |

**Decision:** Vanilla JS. Fresh apps only use a small surface area; we can optimize ruthlessly.

### 3.6 HTTP Server: Axum vs Actix vs Hyper

| Dimension | Axum (chosen) | Actix-web | Hyper raw |
|-----------|--------------|-----------|-----------|
| **Type safety** | Excellent | Good | Manual |
| **Tower ecosystem** | Native | Adapter | Manual |
| **Performance** | Excellent | Excellent | Excellent |
| **Compile time** | Medium | Medium | Low |
| **Binary size** | Medium | Medium | Low |

**Decision:** Axum. Tower middleware composability is critical for Fresh middleware emulation.

### 3.7 String Escaping: Per-Char vs Batch

| Dimension | Per-Char (current) | SIMD Batch (future) |
|-----------|-------------------|---------------------|
| **Correctness** | Perfect | Needs careful testing |
| **Speed** | ~50 MB/s | ~500 MB/s |
| **Complexity** | Trivial | Medium |
| **Portability** | Universal | x86_64 + aarch64 |

**Decision:** Per-char for v0.5. SIMD batch for v0.7+.

---

## 4. Optimization Backlog

### 4.1 High-Impact, Low-Effort

- [ ] **String pre-allocation** — Pre-size HTML output buffer based on VNode tree size estimate.
- [ ] **Route table flattening** — Compile route regexes into a trie for O(1) static + O(n) dynamic matching.
- [ ] **Island manifest caching** — Cache serialized manifest JSON per route.
- [ ] **Component inlining** — Inline small static components (no hooks) into parent render function.

### 4.2 High-Impact, Medium-Effort

- [ ] **Arena allocation for VNodes** — Single bump allocator per request instead of individual `Vec` allocs.
- [ ] **Zero-copy header parsing** — Use `httparse`-style zero-copy for request headers.
- [ ] **Lazy signal notification** — Batch signal updates across a single component render cycle.
- [ ] **HTML streaming** — Write HTML chunks as they are rendered, not buffer-then-send.

### 4.3 High-Impact, High-Effort

- [ ] **AOT JSX compilation** — Compile JSX directly to `format!` strings for static subtrees (no VNode allocation).
- [ ] **Island WASM compilation** — Compile islands to WebAssembly instead of JS for near-native client performance.
- [ ] **Connection pooling reuse** — Reuse HTTP parser state across keep-alive requests.
- [ ] **Profile-guided optimization (PGO)** — Build with PGO for 10-20% throughput gain.

---

## 5. Profiling Guide

### 5.1 CPU Profiling

```bash
# Build with debug symbols
 cargo build --release --config 'profile.release.debug=true'

# Run with perf
perf record -g ./target/release/my-blog
perf report --stdio

# Or use cargo-flamegraph
cargo flamegraph --release
```

### 5.2 Memory Profiling

```bash
# Use dhat for heap tracking
DHAT_OUT_FILE=dhat.json cargo run --features dhat

# Or heaptrack
heaptrack ./target/release/my-blog
```

### 5.3 Benchmarking

```bash
# Run built-in benchmarks
cargo bench

# Macro benchmark with wrk
cd examples/my-blog
runts build --release
./target/release/my-blog &
wrk -t12 -c400 -d30s http://localhost:8000/
```

---

## 6. Comparison with Alternatives

| Metric | runts (target) | Fresh (Deno) | Next.js (Node) | Astro (Node) | Leptos (Rust) |
|--------|---------------|--------------|----------------|--------------|---------------|
| **Runtime** | Native Rust | Deno | Node.js | Node.js | Native Rust |
| **JS runtime dep** | rquickjs (dev) | V8 | V8 | V8 | None |
| **Binary size** | < 2 MB | ~80 MB | ~150 MB | ~100 MB | < 1 MB |
| **Cold start** | < 5 ms | ~50 ms | ~200 ms | ~100 ms | < 5 ms |
| **SSR throughput** | > 50k req/s | ~5k | ~3k | ~4k | > 80k req/s |
| **Dev reload** | < 100 ms | < 100 ms | < 200 ms | < 100 ms | < 100 ms |
| **Islands** | Yes | Yes | Partial | Yes | Yes |
| **TS/TSX source** | Yes | Yes | Yes | Yes | No (Rust DSL) |
| **Fresh compat** | Full | N/A | No | No | No |

*Note: Benchmarks are synthetic and vary by workload. runts targets Fresh compatibility with native performance.*

---

*Last updated: 2026-06-06*
