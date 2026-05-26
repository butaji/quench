# runts — Performance Targets & Trade-offs

## Core Philosophy

**Fast by default, correct by design.**

Every performance decision is evaluated against Fresh/Deno baseline. If we can't beat Fresh on a metric, we don't ship the feature until we can.

---

## 1. Performance Targets

### 1.1 Development Experience

| Metric | Target | Fresh Baseline | How |
|--------|--------|----------------|-----|
| Cold start (dev server) | <1s | ~2-3s (Deno) | HIR interpreter, no Rust compile |
| Hot reload | <100ms | ~200-500ms | File watcher + single-file HIR reload |
| First page render (dev) | <50ms | ~100ms | Direct HIR interpretation |
| Transpile 1 file | <5ms | N/A (interpreted) | Recursive descent parser |
| Transpile 100 files | <2s | N/A | Parallel with rayon |
| Memory (dev server) | <100MB | ~150MB (Deno) | No V8 heap |

### 1.2 Production Runtime

| Metric | Target | Fresh Baseline | How |
|--------|--------|----------------|-----|
| Request latency (p50) | <1ms | ~5-10ms | Native Rust, zero JS overhead |
| Request latency (p99) | <5ms | ~20-50ms | No GC pauses |
| Throughput (rps) | >100,000 | ~20,000 | Axum + tokio, no JS bridge |
| Binary size (hello-world) | <2MB | ~50MB (Deno runtime) | `panic=abort`, `strip`, LTO |
| Binary size (blog) | <5MB | ~50MB | Tree-shaking, dead code elim |
| Startup time | <5ms | ~100ms | No JIT warmup, direct machine code |
| Memory per request | ~10KB | ~100KB | Stack-allocated VDOM, bump alloc |
| Memory at idle | <20MB | ~100MB | No JS heap, minimal runtime |

### 1.3 Build Performance

| Metric | Target | Fresh Baseline | How |
|--------|--------|----------------|-----|
| Full build (100 routes) | <30s | ~10s (Deno deploy) | Transpile + cargo release |
| Incremental build | <3s | ~10s | Cache + dependency tracking |
| CI build time | <2min | ~1min | Parallel jobs, sccache |
| Reproducible builds | ✅ | ✅ | Cargo.lock + pinned toolchain |

---

## 2. Architectural Trade-offs

### 2.1 Custom Parser vs. swc/Babel

**Decision:** Custom recursive descent parser

| Factor | Custom Parser | swc |
|--------|-------------|-----|
| Parse speed | ⚡ ~5ms/file | ⚡ ~2ms/file |
| Binary size | ✅ Small | ❌ +10MB |
| Error messages | ✅ Full control | ⚠️ Limited |
| JS ecosystem | ❌ Manual updates | ✅ Auto via upstream |
| Subset enforcement | ✅ Native | ⚠️ Post-parse filtering |
| Maintenance | ⚠️ Ongoing | ✅ Community |

**Trade-off:** Slightly slower parsing, dramatically smaller binary, better DX.

### 2.2 HIR Interpreter vs. Full Compilation (Dev Mode)

**Decision:** HIR interpreter for dev, native compilation for production

| Factor | HIR Interpreter | Full Compile |
|--------|-----------------|--------------|
| Dev cold start | ✅ <1s | ❌ ~30s |
| Dev hot reload | ✅ <100ms | ❌ ~5-10s |
| Runtime perf | ⚠️ ~10x slower | ✅ Native speed |
| Memory | ✅ Low | ✅ Low |
| Debuggability | ✅ Source-level | ⚠️ Rust-level |

**Trade-off:** Dev speed vs. prod speed. Two modes gives us both.

### 2.3 VDOM vs. Fine-Grained Reactivity

**Decision:** VDOM for SSR, signals for islands

| Factor | VDOM | Fine-Grained (Leptos) |
|--------|------|----------------------|
| SSR complexity | ✅ Simple | ⚠️ Complex |
| Island updates | ⚠️ Re-render | ✅ Surgical |
| Hydration | ✅ Standard | ⚠️ Complex |
| Fresh compat | ✅ Exact | ⚠️ Different model |
| Implementation | ✅ Done | 🔬 Research |

**Trade-off:** VDOM gives us immediate Fresh compatibility. Signals provide island-level optimization.

### 2.4 String Rendering vs. Buffered Writer

**Decision:** String rendering for MVP, buffered streaming for v0.7

| Factor | String | Streaming |
|--------|--------|-----------|
| Implementation | ✅ Simple | ⚠️ Complex |
| Time to first byte | ❌ Full render | ✅ Early flush |
| Memory | ❌ O(n) | ✅ O(buffer) |
| HTTP/2 push | ❌ Post-render | ✅ Interleaved |

**Trade-off:** v0.5-0.6 use string rendering. v0.7+ implements streaming SSR.

### 2.5 Proc Macro vs. Runtime JSX Parser

**Decision:** `html!` proc macro

| Factor | Proc Macro | Runtime Parser |
|--------|-----------|----------------|
| Compile-time check | ✅ Errors at build | ❌ Errors at runtime |
| Runtime overhead | ✅ Zero | ❌ Parse cost |
| Binary size | ✅ Minimal | ❌ +parser code |
| Flexibility | ⚠️ Static | ✅ Dynamic |

**Trade-off:** Static verification and zero overhead at the cost of dynamic JSX generation.

---

## 3. Memory Model

### 3.1 Server-Side Rendering

```
Per-request allocation strategy:

1. VNode tree: Stack-allocated where possible (small trees)
   → Heap for large/deep trees (Vec children)

2. Signal reads: Clone on read (Copy types are zero-cost)

3. HTML string: Single String::with_capacity(estimated)
   → Grows as needed, returned to hyper

4. No arena/bump allocator in v0.5
   → Planned for v0.7 with measured improvement
```

### 3.2 Island Boundaries

Islands are **self-contained**:
- Each island owns its signal graph
- No cross-island signal subscriptions (by design)
- Island props serialized once at SSR, deserialized once at hydration

### 3.3 Signal Memory

```rust
pub struct Signal<T: Clone> {
    inner: Arc<RwLock<T>>,           // Shared state
    subscribers: Arc<RwLock<Vec<...>>>, // Callbacks (server: empty)
}
```

Server: `subscribers` is always empty (no reactivity needed)
Client: Subscriptions active, cleaned up on island unmount

---

## 4. Benchmarking Methodology

### 4.1 SSR Benchmark

```rust
#[bench]
fn render_home_page(b: &mut Bencher) {
    let props = HomeProps { ... };
    b.iter(|| {
        let vnode = home(props.clone());
        black_box(vnode.render_to_string());
    });
}
```

### 4.2 Dev Reload Benchmark

```bash
# Measure time from file save to HTTP response update
echo "mod" >> routes/index.tsx

# Using curl + time
curl -w "%{time_total}" http://localhost:8000/
```

### 4.3 Build Benchmark

```bash
time runts build --release
du -h target/release/my-app
```

### 4.4 Comparative Benchmarks

All benchmarks run against:
- Fresh (Deno) — same component structure
- Next.js (Node) — equivalent app router
- Leptos (Rust) — closest native equivalent
- Astro (Node) — partial hydration comparison

---

## 5. Optimization Backlog

### 5.1 High Impact / Low Effort

- [ ] String pooling for common HTML tags (`div`, `span`, `p`)
- [ ] Pre-allocate HTML string capacity (estimate from VNode tree)
- [ ] Avoid redundant HashMap allocs for empty attrs/events
- [ ] Use `&'static str` for known attribute names

### 5.2 High Impact / Medium Effort

- [ ] Bump allocator for VNode trees (per-request arena)
- [ ] Streaming SSR (flush early, reduce TTFB)
- [ ] Incremental transpilation caching
- [ ] CSS extraction (avoid inline styles in HTML)

### 5.3 High Impact / High Effort

- [ ] Compile-time VDOM elimination (static parts as `&'static str`)
- [ ] Islands as WebAssembly modules (smaller client bundles)
- [ ] Signal graph compilation (no runtime subscription tracking)
- [ ] Alternative backend: Leptos-style reactive DOM (no VDOM)

### 5.4 Research

- [ ] GPU-accelerated layout (unlikely for SSR)
- [ ] JIT compilation of hot HIR paths in dev mode
- [ ] Shared memory parallelism for multi-core rendering

---

## 6. Profiling Strategy

### 6.1 CPU Profiling

```bash
cargo flamegraph --bin my-app
# Analyze: parser, codegen, render loops, signal reads
```

### 6.2 Memory Profiling

```bash
valgrind --tool=massif target/release/my-app
# Analyze: VNode allocations, signal subscriptions, string growth
```

### 6.3 Load Testing

```bash
wrk -t12 -c400 -d30s http://localhost:8000/
# Measure: throughput, latency distribution, errors
```

---

## 7. Known Limitations

| Limitation | Impact | Mitigation |
|------------|--------|------------|
| Dev mode HIR is ~10x slower than compiled | Acceptable for dev | Prod is native |
| VDOM string rendering allocates full HTML | Memory spike per request | Streaming v0.7 |
| Signal subscribers use Vec (O(n) notify) | Slower with many subscribers | Slab allocator v0.7 |
| No cross-module type inference | Less precise types | Explicit annotations |
| JS runtime for islands is ~5KB | Small overhead | Hand-written, no Preact |
| Binary includes full codegen | Larger binary | Strip dev-only code v0.7 |

---

## 8. Performance Checklist (Before Release)

- [ ] SSR latency p50 <1ms on M1 Mac
- [ ] SSR latency p99 <5ms on M1 Mac
- [ ] Throughput >50k rps (single core)
- [ ] Binary size <2MB (hello-world)
- [ ] Dev cold start <1s
- [ ] Hot reload <100ms
- [ ] Memory at idle <50MB
- [ ] No memory leaks in 1M request load test
- [ ] No regressions vs. previous version

---

## 9. Competitive Position

| Framework | Runtime | SSR Latency | Binary Size | Dev Reload |
|-----------|---------|-------------|-------------|------------|
| Fresh | Deno/V8 | ~5-10ms | ~50MB | ~200ms |
| Next.js | Node/V8 | ~10-20ms | ~100MB+ | ~500ms |
| Astro | Node/V8 | ~5-15ms | ~50MB | ~300ms |
| SvelteKit | Node/V8 | ~5-15ms | ~50MB | ~300ms |
| Leptos | Native/WASM | ~1ms | ~3MB | ~5s |
| **runts** | **Native** | **~1ms** | **~2MB** | **~100ms** |

**Position:** Fastest SSR + smallest binary + fastest dev reload. The unique combination of native speed with interpreted dev mode.
