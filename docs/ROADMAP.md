# runts — Roadmap: MVP → Full Fresh Coverage

> **Current Version:** 0.5.0 (MVP)  
> **Strategy:** Correctness first, performance second. Ruthlessly minimal subset, then expand.

---

## JavaScript Engine Evaluation (Completed: 2026-06-06)

**Spike:** `spike/boa` branch evaluated [Boa engine](https://github.com/boa-dev/boa) as a pure-Rust alternative to rquickjs.

### Benchmark Results

| Metric | rquickjs | Boa 0.21 | Winner |
|--------|----------|----------|--------|
| Startup time (context creation) | 96µs | 86µs | Tie |
| Eval time (`1 + 2`) | 85µs | 200µs | rquickjs (2.4x faster) |
| Binary size overhead | ~1MB | ~2MB | rquickjs |
| JS feature compatibility | Full ES2020 | Full ES2020 | Tie |
| Pure Rust | No (rust-v8-based) | Yes | Boa |
| WASM compilation | No | Yes | Boa |
| Maintenance status | Stalled | Active | Boa |

### Verdict: rquickjs retained

rquickjs is the correct choice for this project:
- **Performance:** 2.4x faster eval time matters for dev hot-reload cycles
- **Binary size:** ~1MB overhead vs ~2MB - significant for CLI tool distribution
- **WASM not required:** This is a native CLI tool, not a browser/WASM target
- **Feature parity:** Both engines support all modern JS features used by ink examples

**When Boa would win:** Projects targeting WASM (browser extensions, edge workers) or requiring pure-Rust dependencies for security/compliance reasons.

---

## Phase 0: Foundation (Completed)

**Goal:** Establish the compiler pipeline, runtime, and dev/prod dichotomy.

| Milestone | Status | Notes |
|-----------|--------|-------|
| Custom TSX parser (recursive descent) | ✅ | Covers full supported subset |
| HIR (High-Level IR) | ✅ | Typed, serializable, stripped |
| Semantic analyzer | ✅ | Subset validation, hook rules, module classification |
| Rust code generator | ✅ | HIR → idiomatic Rust source |
| rquickjs dev engine | ✅ | TSX→JS→rquickjs with Yoga layout. HIR interpreter removed. |
| Dev server (Axum + file watcher) | ✅ | < 50ms hot reload |
| Signal system | ✅ | Leptos-style fine-grained reactivity |
| Hooks engine | ✅ | useState, useEffect, useRef, useMemo, useCallback, useReducer, useContext, useId |
| VDOM / SSR renderer | ✅ | VNode tree → HTML string |
| Islands architecture | ✅ | SSR placeholders + partial hydration |
| Client runtime (vanilla JS) | ✅ | Signals, effects, hydration strategies |
| File-based routing | ✅ | Static, dynamic, catch-all, layouts |
| Middleware pipeline | ✅ | Global + section + ctx.next() |
| Route handlers (GET/POST/PUT/DELETE) | ✅ | HandlerContext, ctx.render() |
| Project scaffolding (`runts init`) | ✅ | New project generator |
| Example project (`my-blog`) | ✅ | Working demo with islands |

---

## Phase 1: v0.6.0 — Feature Complete (Next)

**Goal:** Cover 98%+ of real Fresh apps. Eliminate the most common migration blockers.

### 1.1 Language Features

| Feature | Priority | Effort | Rationale |
|---------|----------|--------|-----------|
| Dynamic JSX tags (`<{tagName} />`) | P1 | Medium | Common in design systems |
| `forwardRef` + `useImperativeHandle` | P1 | Medium | Needed for form libraries |
| `useLayoutEffect` | P1 | Low | SSR noop; client sync effect |
| `ref` attribute on DOM elements | P1 | Low | Paired with forwardRef |
| `useSyncExternalStore` (full) | P2 | Medium | External store integration |

### 1.2 Framework Features

| Feature | Priority | Effort | Rationale |
|---------|----------|--------|-----------|
| `_500.tsx` error boundary page | P1 | Low | Production error handling |
| `_error.tsx` catch-all error | P1 | Low | Unified error surface |
| `routeGroups` (e.g., `(marketing)/`) | P2 | Medium | Next.js parity |
| `parallelRoutes` | P3 | High | Advanced routing |
| API routes (`routes/api/*.ts`) | P2 | Medium | REST endpoints without JSX |

### 1.3 Build & Tooling

| Feature | Priority | Effort | Rationale |
|---------|----------|--------|-----------|
| Asset pipeline (CSS bundling) | P1 | Medium | Currently no CSS processing |
| PostCSS / Tailwind integration | P2 | Medium | Tailwind is ubiquitous |
| Image optimization | P3 | High | `next/image` parity |
| Source maps (dev) | P2 | Medium | Debuggability |
| Source maps (prod) | P3 | Low | Optional |

### 1.4 Parser Migration (Foundation for v1.0)

| Feature | Priority | Effort | Rationale |
|---------|----------|--------|-----------|
| swc/oxc parser integration (opt-in) | P2 | High | Full TS spec compliance |
| Custom parser deprecation path | P2 | Low | Documentation + warnings |

**Target Release:** Q3 2026

---

## Phase 2: v0.7.0 — Production Hardening

**Goal:** Make runts safe and observable for production workloads.

### 2.1 Reliability

| Feature | Priority | Notes |
|---------|----------|-------|
| Streaming SSR | P1 | Chunked HTML response for large pages |
| Suspense boundaries | P1 | Async component resolution |
| Error boundaries (island-level) | P1 | Catch errors in specific islands |
| Retry logic for failed hydration | P2 | Auto-retry island hydration once |
| Graceful degradation (no-JS) | P2 | Islands render as static if JS fails |

### 2.2 Observability

| Feature | Priority | Notes |
|---------|----------|-------|
| Structured logging (tracing) | P1 | Already partially implemented |
| Per-route metrics | P2 | Render time, handler time |
| Island hydration metrics | P2 | Time-to-interactive per island |
| Health check endpoint | P1 | `/_health` |

### 2.3 Security

| Feature | Priority | Notes |
|---------|----------|-------|
| CSP nonce generation | P1 | Inline script nonce |
| CSRF protection helpers | P2 | Token generation |
| HTML sanitization (strict mode) | P2 | Reject dangerous attrs |

**Target Release:** Q4 2026

---

## Phase 3: v0.8.0 — Developer Experience

**Goal:** Dev mode should feel as fast as Vite, if not faster.

### 3.1 Hot Reload

| Feature | Priority | Notes |
|---------|----------|-------|
| Fine-grained HMR (CSS) | P1 | Inject styles without page refresh |
| Fine-grained HMR (island props) | P2 | Hot-swap island data |
| Preserved state on reload | P2 | Keep form inputs across reloads |
| Error overlay | P1 | Vite-style overlay in browser |

### 3.2 Debugging

| Feature | Priority | Notes |
|---------|----------|-------|
| HIR inspector CLI | P2 | `runts inspect routes/index.tsx` |
| Generated Rust viewer | P2 | `runts build --show-rust` |
| Type mismatch diagnostics | P1 | Better error messages with suggestions |
| Levenshtein-based import fixes | P2 | "Did you mean...?" |

### 3.3 Testing

| Feature | Priority | Notes |
|---------|----------|-------|
| Component test utilities | P2 | Render component to HTML string in tests |
| Route handler test helpers | P2 | Mock HandlerContext |
| Snapshot testing | P3 | HTML output snapshots |

**Target Release:** Q1 2027

---

## Phase 4: v0.9.0 — Ecosystem Integration

**Goal:** Play nicely with the existing web ecosystem.

### 4.1 Database & Backend

| Feature | Priority | Notes |
|---------|----------|-------|
| Database connection pooling | P2 | `sqlx` integration helpers |
| Prisma-style codegen | P3 | Type-safe query builder |
| Edge-compatible handlers | P2 | WASI-compatible subset |

### 4.2 Deployment

| Feature | Priority | Notes |
|---------|----------|-------|
| Docker image generation | P2 | `runts build --docker` |
| Static site export | P1 | `runts export` → plain HTML files |
| Vercel / Netlify adapter | P2 | Serverless handler wrapper |
| Fly.io / Railway template | P3 | One-click deploy |

### 4.3 Interop

| Feature | Priority | Notes |
|---------|----------|-------|
| Preact component import (WASM) | P3 | Compile islands to WASM instead of JS |
| React Server Components (RSC) | P3 | Experimental RSC subset |
| MDX support | P2 | Markdown + JSX pages |

**Target Release:** Q2 2027

---

## Phase 5: v1.0.0 — Stable

**Goal:** Declare 1.0 with a stability guarantee.

### 5.1 Requirements for v1.0

- [ ] Zero known correctness bugs in supported subset
- [ ] All Fresh tutorial examples compile and run
- [ ] Production binary < 2MB for default template
- [ ] SSR throughput > 50k req/s on commodity hardware
- [ ] Dev reload < 50ms p99
- [ ] Complete documentation (API reference, guides, examples)
- [ ] Stable CLI interface (no breaking changes after 1.0)
- [ ] LTS release schedule (6-month support windows)

### 5.2 Post-1.0 Ideas

| Feature | Notes |
|---------|-------|
| WASM compilation target | Compile entire app to WASM for edge workers |
| Multi-tenancy | Host multiple apps in one binary |
| Plugin system | Custom transpiler passes |
| Visual IDE integration | Language server protocol (LSP) |

**Target Release:** Q3 2027

---

## Decision Log

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-05 | Custom parser for v0.5 | Zero deps, fast compile, subset control. Migrate to oxc for v1.0. |
| 2026-05 | rquickjs for dev | Full JS semantics via rquickjs + Yoga. HIR interpreter removed (was becoming custom JS engine). |
| 2026-05 | Signals + VDOM hybrid | Fine-grained for islands, VDOM for SSR composition. Best of both. |
| 2026-05 | Vanilla JS client runtime | No Preact dependency in browser. < 5KB runtime. |
| 2026-05 | Panic = abort in release | Smallest binary. Errors return HTTP 500; no unwinding needed. |
| 2026-06 | rquickjs retained over Boa | Benchmark: rquickjs is 2.4x faster for eval (85µs vs 200µs). Same JS feature support. Boa advantage is pure-Rust + WASM, not needed for this project. |

---

*Last updated: 2026-05-27*
