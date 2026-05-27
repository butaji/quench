# runts Performance Targets and Trade-offs

> Performance is a core design goal, but never at the expense of correctness or Fresh compatibility.

---

## 1. Performance Targets

### 1.1 Development Mode

| Metric | Target | v0.5 Status | How |
|--------|--------|-------------|-----|
| Hot reload latency | < 50ms | ~100ms | HIR interpreter (no compilation) |
| Cold dev server start | < 500ms | ~1s | Lazy module loading + cache |
| Memory per request | < 1MB | ~2MB | Arena allocation + request-scoped values |
| Parser throughput | > 50MB/s | ~20MB/s | Hand-written recursive descent |
| Concurrent requests | > 1000 | ~500 | Tokio task-per-request |

### 1.2 Production Mode

| Metric | Target | v0.5 Status | How |
|--------|--------|-------------|-----|
| Binary size | < 2MB | ~5MB | LTO + strip + panic=abort |
| Cold start | < 10ms | ~50ms | Static compilation, no JIT |
| Memory baseline | < 5MB RSS | ~20MB | Minimal runtime, no GC |
| SSR throughput | > 200K req/s | ~50K req/s | Zero-copy rendering + connection pooling |
| P99 latency | < 1ms | ~5ms | Predictable allocation, no pauses |
| Island hydration | < 16ms | ~50ms | Direct DOM binding, no VDOM diff |
| Build time | < 10s | ~30s | Incremental + parallel codegen |

### 1.3 Client-side (Islands)

| Metric | Target | v0.5 Status | How |
|--------|--------|-------------|-----|
| Island bundle (simple) | < 3KB gzipped | ~5KB | Vanilla JS, no framework runtime |
| Island bundle (complex) | < 10KB gzipped | ~15KB | Tree-shaking + shared chunks |
| Signal read | O(1) | O(1) | Cached value |
| Signal write | O(subscribers) | O(subscribers) | Direct notification |
| First paint (island) | < 100ms | ~200ms | Eager hydration |
| Time to interactive | < 200ms | ~500ms | Lazy hydration strategies |

---

## 2. Benchmarks

### 2.1 SSR Benchmark: "Hello World" Route

```typescript
// routes/index.tsx
export default function Home() {
  return <h1>Hello, World!</h1>;
}
```

| Framework | Throughput | Latency (p50) | Memory |
|-----------|-----------|---------------|--------|
| runts (target) | 200K req/s | 0.1ms | 5MB |
| runts (v0.5) | 50K req/s | 0.5ms | 20MB |
| Fresh (Deno) | 30K req/s | 1.0ms | 50MB |
| Next.js (Node) | 15K req/s | 2.0ms | 100MB |
| Nuxt (Node) | 12K req/s | 2.5ms | 120MB |
| Leptos (Rust) | 250K req/s | 0.05ms | 3MB |
| Astro (Node) | 20K req/s | 1.5ms | 80MB |

### 2.2 SSR Benchmark: Complex Page

```typescript
// routes/blog/[slug].tsx with 10 islands, 50 components
export default function BlogPost({ data }: PageProps<PostData>) {
  return (
    <article>
      <Header title={data.title} />
      <Markdown content={data.content} />
      <CommentList comments={data.comments} />
      <LikeButton postId={data.id} />
      <ShareButtons url={data.url} />
    </article>
  );
}
```

| Framework | Throughput | Latency (p50) |
|-----------|-----------|---------------|
| runts (target) | 50K req/s | 0.5ms |
| runts (v0.5) | 10K req/s | 2ms |
| Fresh (Deno) | 8K req/s | 3ms |
| Next.js | 3K req/s | 8ms |

### 2.3 Island Hydration Benchmark

10 interactive islands on a page, each with `useState` and `onClick`.

| Strategy | Time to Hydrate | JS Bundle Size |
|----------|----------------|----------------|
| runts (eager) | 50ms | 25KB total |
| Fresh (Preact) | 80ms | 45KB (incl. Preact runtime) |
| Next.js (React) | 150ms | 120KB (incl. React runtime) |
| Astro (islands) | 60ms | 35KB |

---

## 3. Architectural Trade-offs

### 3.1 HIR Interpreter vs Incremental Compilation (Dev Mode)

| Approach | Pros | Cons | Decision |
|----------|------|------|----------|
| **HIR Interpreter** (chosen) | <100ms reload, guaranteed parity, simpler implementation | Runtime overhead per request, limited debugging | ✅ Primary dev mode |
| Incremental rustc | Zero runtime overhead, native debugging | 5-30s reload, complex state management | Future: optional |
| WASM JIT | Fast execution, JS interop | Requires WASM runtime, larger binary | Rejected |

**Rationale**: Developer experience (fast reload) outweighs the small runtime overhead in development. Production is fully compiled.

### 3.2 VDOM vs Direct String Rendering

| Approach | Pros | Cons | Decision |
|----------|------|------|----------|
| **VDOM** (chosen) | Familiar model, easy islands integration, diffing for updates | Allocation overhead, slower than strings | ✅ Server + islands SSR |
| Direct string rendering | Fastest, zero allocation | No component model, hard to hydrate | Used for static routes (fast path) |
| Template-based | Very fast, compile-time optimization | Less flexible, no dynamic composition | Future optimization |

**Rationale**: VDOM provides the right abstraction for component composition while allowing optimization to string rendering for static routes.

### 3.3 Signals vs VDOM on Client

| Approach | Pros | Cons | Decision |
|----------|------|------|----------|
| **Signals** (chosen) | O(1) updates, no diffing, minimal bundle | New paradigm for some developers | ✅ Client islands |
| VDOM diffing | Familiar, well-understood | O(n) diffing, larger runtime | Rejected for islands |
| Server-sent HTML | Zero client JS | No interactivity without round-trip | Rejected |

**Rationale**: Preact signals have proven this model works. runts implements the same semantics in native Rust + minimal JS.

### 3.4 Hand-written Parser vs Parser Generator

| Approach | Pros | Cons | Decision |
|----------|------|------|----------|
| **Hand-written** (chosen) | Small binary, precise errors, easy to extend | More code to maintain, longer initial development | ✅ Chosen |
| swc (Rust) | Fast, battle-tested, complete TS support | 5MB+ dependency, complex API, WASM for some features | Rejected |
| tree-sitter | Fast parsing, multiple languages | Requires grammar definition, less control | Rejected |
| nom/combine | Parser combinators in Rust | Slower, complex error messages | Rejected |

**Rationale**: Zero external dependencies is a core constraint. Hand-written parser is ~50KB and gives us full control.

### 3.5 Axum vs Actix vs Warp

| Framework | Throughput | Ergonomics | Ecosystem | Decision |
|-----------|-----------|------------|-----------|----------|
| **Axum** (chosen) | 200K+ req/s | Excellent | Tower middleware | ✅ Chosen |
| Actix | 200K+ req/s | Good | Own middleware | Close second |
| Warp | 150K+ req/s | Complex filters | Limited | Rejected |
| hyper (raw) | 250K+ req/s | Verbose | None | Too low-level |

**Rationale**: Axum's Tower integration and ergonomic API make it the best balance of performance and DX.

### 3.6 One Binary vs Separate Dev/Production Runtimes

| Approach | Pros | Cons | Decision |
|----------|------|------|----------|
| **Single binary** (chosen) | Simpler distribution, same code paths | Larger binary (includes interpreter) | ✅ Single `runts` CLI |
| Separate binaries | Smaller production binary | More complex build, potential divergence | Rejected |
| Dev as library | Can embed in editors | More complex linking | Rejected |

**Rationale**: The interpreter is small (~500KB). Including it simplifies everything.

---

## 4. Optimization Strategies

### 4.1 Static Route Fast Path

Routes with no islands and no dynamic data can skip VDOM entirely:

```rust
// Generated for static routes
pub fn static_route_index() -> Response {
    Response::builder()
        .header("Content-Type", "text/html")
        .body(Body::from(STATIC_HTML_INDEX))
        .unwrap()
}

// STATIC_HTML_INDEX is a compile-time constant
const STATIC_HTML_INDEX: &str = include_str!("../gen/static/index.html");
```

This achieves **zero-allocation, zero-parse** response generation.

### 4.2 VNode Arena Allocation

Instead of `Box`ing every VNode, use an arena:

```rust
pub struct VNodeArena {
    storage: bumpalo::Bump,
}

impl VNodeArena {
    pub fn alloc(&self, node: VNode) -> &mut VNode {
        self.storage.alloc(node)
    }
    
    pub fn reset(&self) {
        self.storage.reset();
    }
}
```

Benefits:
- Single allocation per request
- O(1) deallocation (drop the arena)
- Better cache locality

### 4.3 HTML Streaming

For large pages, stream HTML as it's generated:

```rust
pub async fn render_streamed(component: impl Fn() -> VNode) -> Body {
    let (tx, rx) = tokio::sync::mpsc::channel(100);
    
    tokio::spawn(async move {
        let vnode = component();
        render_vnode_streamed(&vnode, &tx).await;
    });
    
    Body::from_stream(rx)
}
```

Benefits:
- Time to First Byte (TTFB) is immediate
- Browser can parse HTML while server renders
- Better perceived performance

### 4.4 Island Bundle Deduplication

Multiple islands sharing the same hooks/signals get a shared runtime chunk:

```
dir/
├── islands/
│   ├── shared.js       # signal + hook runtime (shared)
│   ├── counter.js      # Counter island (uses shared)
│   └── todo-list.js    # TodoList island (uses shared)
```

Instead of:
```
dir/
├── islands/
│   ├── counter.js      # Counter + full runtime (duplicated)
│   └── todo-list.js    # TodoList + full runtime (duplicated)
```

### 4.5 Signal Batch Updates

Multiple signal writes in a single event handler are batched:

```typescript
// TypeScript
function handleClick() {
  setCount(count + 1);  // Signal write 1
  setLoading(true);      // Signal write 2
  setError(null);        // Signal write 3
}
```

```rust
// Generated Rust
fn handle_click() {
    batch(|| {
        count.set(count.get() + 1);
        loading.set(true);
        error.set(None);
    });
    // All subscribers notified once at end of batch
}
```

### 4.6 Compile-time Constant Folding

Expressions that can be evaluated at compile time are folded:

```typescript
const YEAR = 2024;
const nextYear = YEAR + 1;  // → const nextYear: f64 = 2025.0;
```

```rust
const NEXT_YEAR: f64 = 2025.0;
```

---

## 5. Memory Model

### 5.1 Server Memory Layout

```
┌─────────────────────────────────────┐  High addresses
│           Stack                     │
│  (per-request, small, fast)         │
├─────────────────────────────────────┤
│           Heap                      │
│  (VDOM arenas, request-scoped)      │
├─────────────────────────────────────┤
│           Static                    │
│  (code, constants, route table)     │
├─────────────────────────────────────┤
│           Runtime                   │
│  (signal graphs, island registries) │
├─────────────────────────────────────┤
│           Binary                    │
│  (.text, .rodata, .data)            │
└─────────────────────────────────────┘  Low addresses
```

### 5.2 Request Lifecycle Memory

```
Request starts
  → Arena allocated (1MB reserved)
  → VDOM rendered into arena
  → HTML string generated from VDOM
  → Response sent
  → Arena dropped (O(1))
Request ends
```

### 5.3 Signal Graph Memory (Client)

```
Signal A ──▶ Computed B ──▶ Effect C (DOM update)
     │            │
     ▼            ▼
Signal D ──▶ Computed E
```

- Signals: ~32 bytes each (value + version + subscriber list)
- Computed: ~48 bytes each (cache + compute fn + dependencies)
- Effects: ~40 bytes each (execute fn + dependencies)

A typical island with 5 signals, 2 computed, 3 effects: ~400 bytes.

---

## 6. Profiling and Monitoring

### 6.1 Built-in Metrics

```rust
#[derive(Default)]
pub struct RuntimeMetrics {
    pub requests_total: AtomicU64,
    pub requests_duration: Histogram,
    pub vdom_nodes_allocated: AtomicU64,
    pub signal_updates: AtomicU64,
    pub island_hydrations: AtomicU64,
    pub parser_cache_hits: AtomicU64,
}
```

### 6.2 Tracing Integration

```rust
#[tracing::instrument(skip(req))]
pub async fn handle_request(req: Request) -> Response {
    tracing::info!(method = %req.method(), path = %req.uri(), "handling request");
    
    let start = Instant::now();
    let response = route_handler(req).await;
    
    tracing::info!(duration = ?start.elapsed(), status = %response.status(), "request complete");
    response
}
```

### 6.3 Memory Profiling

```rust
#[cfg(feature = "profile")]
pub fn dump_memory_stats() {
    let stats = dhat::HeapStats::get();
    tracing::info!(
        total_bytes = stats.total_bytes,
        total_blocks = stats.total_blocks,
        "heap statistics"
    );
}
```

---

## 7. Platform-Specific Optimizations

### 7.1 Linux

- `mimalloc` allocator (faster than glibc malloc)
- `io_uring` for async file I/O (future)
- `SO_REUSEPORT` for load balancing across cores
- Transparent Huge Pages (THP)

### 7.2 macOS

- `malloc` with scribble/debug in dev mode
- `kqueue` for file watching (already used by notify crate)

### 7.3 Windows

- `I/O Completion Ports` for async I/O
- `VirtualLock` for keeping working set in memory

---

## 8. Scalability

### 8.1 Vertical Scaling

```
CPU cores: 1 → 64
Throughput: 50K → 3M req/s (linear scaling)
Memory: 20MB → 100MB (sub-linear, shared read-only data)
```

### 8.2 Horizontal Scaling

runts binaries are stateless (except for dev mode interpreter cache):

```
Load Balancer
  ├── runts instance 1 (port 3000)
  ├── runts instance 2 (port 3001)
  ├── runts instance 3 (port 3002)
  └── ...
```

No session affinity required. Each instance is independent.

### 8.3 Edge Deployment

Binary size < 2MB enables edge deployment:

| Platform | Cold Start | Notes |
|----------|-----------|-------|
| Cloudflare Workers | ~5ms | WASM compile (future) |
| Vercel Edge | ~10ms | Native binary |
| Deno Deploy | N/A | Requires JS runtime |
| Fly.io | ~10ms | Native binary + Docker |

---

## 9. Comparison with Alternatives

### 9.1 vs Fresh (Deno)

| Aspect | runts | Fresh |
|--------|-------|-------|
| Runtime | Native Rust | Deno (V8) |
| Cold start | < 10ms | ~100ms |
| Memory | < 5MB | ~50MB |
| SSR throughput | 200K req/s | 30K req/s |
| Dev reload | < 50ms | < 100ms |
| Binary size | < 2MB | N/A (needs Deno) |
| TS subset | Defined + enforced | Full TS |
| Islands | Native signals | Preact signals |
| Ecosystem | Rust | Deno/Node |

**Trade-off**: runts requires learning a supported subset; Fresh supports all of TypeScript. For performance-critical applications, runts wins. For rapid prototyping with full TS, Fresh wins.

### 9.2 vs Leptos (Rust)

| Aspect | runts | Leptos |
|--------|-------|--------|
| Language | TypeScript (subset) | Rust (full) |
| Learning curve | Low (React devs) | High (Rust + web) |
| SSR throughput | 200K req/s | 250K req/s |
| Reactivity | Signals | Signals |
| Islands | Yes | No (full-app hydration) |
| Fresh compat | Full | None |
| Bundle size | 2-10KB per island | 15-50KB full app |

**Trade-off**: Leptos is faster for full-Rust teams; runts enables Rust performance with TypeScript DX.

### 9.3 vs Next.js (Node)

| Aspect | runts | Next.js |
|--------|-------|---------|
| Runtime | Native Rust | Node.js (V8) |
| Cold start | < 10ms | ~500ms |
| Memory | < 5MB | ~100MB |
| SSR throughput | 200K req/s | 15K req/s |
| Dev reload | < 50ms | < 200ms |
| Ecosystem | Growing | Massive |
| Deployment | Binary/Docker | Vercel/Node |

**Trade-off**: Next.js has unmatched ecosystem; runts has unmatched performance.

---

## 10. Future Performance Work

### 10.1 Short Term (v0.6-0.7)

- [ ] String rendering fast path for static routes
- [ ] VNode arena allocation
- [ ] Parser cache (serialized HIR)
- [ ] Parallel file processing in build

### 10.2 Medium Term (v0.8-0.9)

- [ ] HTML streaming
- [ ] Incremental cargo builds
- [ ] Island bundle deduplication
- [ ] Connection pooling optimization
- [ ] SIMD-accelerated HTML escaping

### 10.3 Long Term (v1.0+)

- [ ] AOT compilation to machine code (no cargo)
- [ ] Custom allocator optimized for VDOM
- [ ] io_uring for file I/O
- [ ] Kernel-bypass networking (DPDK)
- [ ] GPU-accelerated rendering (unlikely but explored)

---

## 11. Measuring Performance

### 11.1 Benchmark Suite

```bash
# Run all benchmarks
runts bench

# Specific benchmarks
runts bench ssr          # Server-side rendering
runts bench hydration    # Island hydration
runts bench parse        # Parser throughput
runts bench build        # Build time
runts bench binary       # Binary size
```

### 11.2 Continuous Benchmarking

GitHub Actions runs benchmarks on every PR and reports regressions:

```yaml
- name: Benchmark
  run: cargo bench -- --save-baseline main
  
- name: Compare
  run: cargo bench -- --baseline main
```

### 11.3 Real-world Profiling

```bash
# CPU profiling
perf record -g ./target/release/my-app
perf report

# Memory profiling
dhat-heap ./target/release/my-app

# Flamegraph
cargo flamegraph --bin my-app
```
