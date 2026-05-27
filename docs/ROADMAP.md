# runts Roadmap: MVP → Full Fresh Coverage

> **Current Version:** 0.5.0  
> **Target:** Full framework-level Fresh/Preact compatibility

---

## Phase 0: Foundation (✅ Complete — v0.1–v0.5)

| Milestone | Status | Notes |
|-----------|--------|-------|
| Recursive descent TS/TSX parser | ✅ | Zero dependencies, ~2K LOC |
| HIR (High-Level IR) | ✅ | Typed AST with full expression coverage |
| Semantic analyzer | ✅ | Island/route/hook detection |
| Rust code generator | ✅ | HIR → idiomatic Rust |
| `html!` macro | ✅ | Compile-time JSX validation |
| File-based routing | ✅ | Static, dynamic `[slug]`, catch-all `[...path]` |
| Layout system | ✅ | Nested `_layout.tsx` composition |
| Middleware pipeline | ✅ | `ctx.next()`, `ctx.render()`, `ctx.state` |
| Islands architecture | ✅ | SSR + hydration manifest |
| HIR interpreter | ✅ | Dev mode direct execution |
| Dev server (Axum) | ✅ | Hot reload <50ms |
| Production build | ✅ | Transpile + cargo build |
| Error pages (`_404`, `_500`) | ✅ | Fallback chain with suggestions |
| Signals (`useSignal`, `useComputed`) | ✅ | Fine-grained reactivity |
| Hooks (`useState`–`useId`) | ✅ | Full Preact hook surface |

---

## Phase 1: Correctness & Compatibility (v0.6 — Current Focus)

### 1.1 Middleware & Handler Edge Cases
- [x] `ctx.next()` passing through middleware chain
- [x] `return ctx.render()` vs `return new Response()`
- [x] Middleware state propagation to handlers
- [ ] Async middleware ordering guarantees
- [ ] Error handling in middleware (`throw` → 500)

### 1.2 Component Patterns
- [x] Destructured props (`{ name, ...rest }`)
- [x] Default prop values
- [ ] Forward refs (alternative: direct element refs)
- [ ] Component lazy loading (static splitting only)

### 1.3 Type System Hardening
- [x] Interface → struct generation
- [x] Union → Option/enum conversion
- [ ] Generic monomorphization (basic cases)
- [ ] Recursive type detection & bailout
- [ ] Mapped type approximation (`Record`, `Pick`, `Omit`)

### 1.4 Fresh Parity
- [x] `PageProps` typed params
- [x] `HandlerContext` with `render()`/`next()`
- [x] `Handler` export object
- [ ] `_app.tsx` root wrapper
- [ ] Route groups (`(group)/` prefix)
- [ ] Plugin system (build hooks)

**Target:** 98% of Fresh starter projects compile without modification.

---

## Phase 2: Performance & Scale (v0.7)

### 2.1 Runtime Optimizations
- [ ] StringBuilder SSR for non-island content (eliminate VNode alloc)
- [ ] Arena allocation for per-request VNode trees
- [ ] Zero-copy prop serialization (borrowed strings where possible)
- [ ] Signal batching across multiple updates

### 2.2 Build Performance
- [x] Parallel transpilation (rayon)
- [ ] Incremental compilation (only changed files)
- [ ] HIR caching across builds (persist to `.runts/cache/`)
- [ ] Watch-mode for production builds (`runts build --watch`)

### 2.3 Binary Size
- [ ] Dead code elimination for unused components
- [ ] Conditional island inclusion (only ship used islands)
- [ ] `build-std` + panic handler override for embedded targets
- [ ] Target: `<1MB` binary for simple apps (currently ~1.5–2MB)

### 2.4 Startup Time
- [ ] Precompiled route trie (build time regex → static dispatch)
- [ ] Lazy island registration (pay-for-what-you-use)
- [ ] Target: `<2ms` cold start for simple apps

---

## Phase 3: Advanced Features (v0.8)

### 3.1 Streaming & Suspense
- [ ] HTML streaming ( chunked `Transfer-Encoding`)
- [ ] Suspense boundaries in JSX (`<Suspense fallback={...}>`)
- [ ] Async component SSR (await data before flush)

### 3.2 Error Boundaries
- [ ] `Result`-based error boundaries in components
- [ ] Per-island error isolation
- [ ] Graceful degradation (fallback UI for crashed islands)

### 3.3 Form Handling
- [ ] `useFormStatus` hook equivalent
- [ ] Server Actions (compile form handlers to POST routes)
- [ ] CSRF protection built-in

### 3.4 Internationalization
- [ ] `useLocale()` hook
- [ ] Compile-time string extraction
- [ ] RTL layout support

---

## Phase 4: Ecosystem (v0.9)

### 4.1 Tooling
- [ ] VS Code extension (syntax highlighting for `html!`)
- [ ] Language server protocol (LSP) for type errors in TS→Rust
- [ ] `runts test` — test runner for components (headless SSR)

### 4.2 Deployment
- [ ] Docker image generation (`runts deploy --docker`)
- [ ] Static site export (`runts export` — pre-render all routes)
- [ ] Serverless adapter (AWS Lambda / Vercel / Cloudflare Workers)

### 4.3 Libraries
- [ ] `runts-tailwind` — Tailwind CSS integration
- [ ] `runts-markdown` — MDX-like compilation
- [ ] `runts-auth` — OAuth/session middleware pack

---

## Phase 5: 1.0 Stability (v1.0)

### 5.1 Backwards Compatibility
- [ ] Stable TS subset guarantee (no breaking changes to supported syntax)
- [ ] Migration guide for each minor version
- [ ] Deprecation cycle for removed features

### 5.2 Documentation
- [ ] Full API reference (generated from Rust docs)
- [ ] Interactive tutorial (built with runts)
- [ ] Video course: "From Fresh to runts"

### 5.3 Production Hardening
- [ ] Fuzz-tested parser (against real-world TS code)
- [ ] Memory safety audit (Miri validation)
- [ ] Performance regression suite
- [ ] Binary reproducibility (deterministic builds)

---

## Timeline Estimate

| Phase | Version | Estimated Duration | Key Deliverable |
|-------|---------|-------------------|-----------------|
| Foundation | v0.5 | ✅ Done | Working compiler + dev server |
| Correctness | v0.6 | 4–6 weeks | 98% Fresh parity |
| Performance | v0.7 | 3–4 weeks | <1MB binary, <2ms startup |
| Advanced | v0.8 | 4–6 weeks | Streaming, error boundaries |
| Ecosystem | v0.9 | 6–8 weeks | LSP, static export, adapters |
| Stable | v1.0 | 4–6 weeks | Production guarantee |

**Total to v1.0:** ~6–8 months of focused engineering.

---

## Decision Log

| Date | Decision | Rationale |
|------|----------|-----------|
| 2024-05 | Hand-written parser vs SWC | Zero deps, faster compile, subset-only |
| 2024-05 | HIR interpreter for dev | <50ms reload vs 30s Rust compile |
| 2024-06 | Signals instead of VDOM diff | Fine-grained updates, smaller client bundle |
| 2024-06 | Axum over hyper directly | Tower middleware ecosystem |
| 2024-07 | Exclude class components | 95%+ of modern Preact is functional |
| 2024-08 | Parallel transpilation via rayon | I/O-bound, near-linear speedup |

