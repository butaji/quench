# runts Roadmap

> From MVP to full Fresh coverage.

---

## Phase 0: Foundation (COMPLETE ✓)

**Status**: All core infrastructure is built and functional.

### Delivered:
- [x] Recursive descent TS/TSX parser
- [x] HIR (High-level IR) with full TS construct coverage for subset
- [x] Semantic analyzer (route/island detection, hook validation, type extraction)
- [x] Rust code generator (TS → Rust source)
- [x] HIR interpreter for dev mode (zero compilation)
- [x] File-based routing with Fresh conventions
- [x] Middleware pipeline (global + route-scoped)
- [x] Islands architecture with partial hydration
- [x] Fine-grained signals (Preact signals equivalent)
- [x] Hook system (useState, useEffect, useRef, useMemo, useCallback, useReducer, useId)
- [x] VDOM for SSR
- [x] Dev server with hot reload (<100ms)
- [x] Production build pipeline (transpile + cargo build)
- [x] Client JS runtime (vanilla JS, no React/Preact)
- [x] `html!` proc macro for JSX-like syntax in Rust
- [x] Example blog application
- [x] 80+ unit tests

### Architecture:
```
Parser → HIR → Analyzer → [Interpreter | Codegen → Rust → Binary]
```

---

## Phase 1: MVP Stabilization (v0.5.x → v0.6.0)

**Goal**: Make the example blog fully functional end-to-end.

### P1.1 Codegen correctness
- [ ] Fix island component code generation (closure params, struct initialization)
- [ ] Fix array `.map()` / `.filter()` closure parameter generation
- [ ] Fix destructured array pattern from hooks (`const [a, b] = useState(...)`)
- [ ] Fix optional chaining (`obj?.prop`) codegen
- [ ] Fix nullish coalescing (`a ?? b`) codegen
- [ ] Fix spread operator in object literals
- [ ] Fix ternary expression precedence

**Acceptance**: All example blog pages render correctly in dev mode and compile to a working binary.

### P1.2 Runtime completeness
- [ ] Implement `useContext` with Context provider/consumer
- [ ] Implement `useSyncExternalStore` for external state
- [ ] Implement `useLayoutEffect` (sync after DOM mutation)
- [ ] Add `useDeferredValue` / `useTransition` (if signals don't cover it)
- [ ] Add `useImperativeHandle` equivalent for ref forwarding

**Acceptance**: All hooks from the supported subset specification work correctly in both dev and production modes.

### P1.3 Dev server polish
- [ ] WebSocket hot reload (browser auto-refresh)
- [ ] Error overlay in browser
- [ ] Source location in error messages
- [ ] Request logging with timing
- [ ] Static file serving with cache headers
- [ ] Proxy configuration for API backends

**Acceptance**: Dev server experience matches or exceeds Fresh's dev experience.

### P1.4 Testing
- [ ] Integration test: dev server renders all example routes
- [ ] Integration test: production binary serves correct HTML
- [ ] Integration test: island hydration works in headless browser
- [ ] Integration test: middleware pipeline executes in correct order
- [ ] Property-based tests for parser (fuzz TS/TSX snippets)
- [ ] Benchmark: parse + codegen throughput

**Acceptance**: >90% code coverage, all integration tests passing.

---

## Phase 2: Feature Complete Fresh Compatibility (v0.7.0)

**Goal**: 95%+ Fresh API compatibility. Real Fresh projects can migrate with minimal changes.

### P2.1 Advanced routing
- [ ] Catch-all routes (`[...slug].tsx`)
- [ ] Optional route params (`[[param]].tsx`)
- [ ] Route groups (`(group)/route.tsx`)
- [ ] Route config exports (`export const config = { routeOverride: ... }`)
- [ ] API routes with full method support (PUT, DELETE, PATCH)
- [ ] Route preloading / prefetching

### P2.2 Layout system
- [ ] Nested layouts (`_layout.tsx`)
- [ ] Layout groups
- [ ] Root layout (`routes/_app.tsx`)
- [ ] Error boundaries per layout
- [ ] Parallel routes

### P2.3 Islands enhancements
- [ ] Island-to-island communication via shared signals
- [ ] Server islands (no client JS, server-rendered only)
- [ ] Island client:media hydration (`client:media="(min-width: 768px)"`)
- [ ] Island client:only (skip SSR)
- [ ] Island CSS scoping
- [ ] Island preloading / priority hints

### P2.4 Data fetching
- [ ] `handler` with `ctx.render(data)`
- [ ] Async route components (`export default async function Page()`)
- [ ] Data loaders (island-level data fetching)
- [ ] SWR-style caching on client
- [ ] Server-side mutations (forms + actions)

### P2.5 Fresh APIs
- [ ] `Head` component for `<head>` manipulation
- [ ] `asset()` for static file references
- [ ] `IS_BROWSER` compile-time constant
- [ ] `ctx.state` for request-scoped state
- [ ] `ctx.renderNotFound()` / `ctx.renderError()`

### P2.6 Standard library expansion
- [ ] `URL` / `URLSearchParams`
- [ ] `fetch` with timeout and retry
- [ ] `FormData` parsing
- [ ] `Blob` / `File` (server-side)
- [ ] `TextEncoder` / `TextDecoder`
- [ ] `atob` / `btoa`

### P2.7 Type system
- [ ] Generic function components
- [ ] Mapped types (limited)
- [ ] Type guards (`is` keyword)
- [ ] `keyof` operator
- [ ] `typeof` for types
- [ ] Discriminated unions (full)

---

## Phase 3: Performance Optimization (v0.8.0)

**Goal**: Fastest possible server-side rendering in the Rust ecosystem.

### P3.1 Compile-time optimizations
- [ ] Incremental compilation (only changed modules)
- [ ] Parallel codegen (rayon-based)
- [ ] Lazy module loading in dev mode
- [ ] Memory-mapped file parsing for large projects
- [ ] Parser cache (serialized HIR to disk)

### P3.2 Runtime optimizations
- [ ] String rendering without VDOM (fast path for static routes)
- [ ] Arena allocation for VNode trees
- [ ] Zero-copy HTML streaming
- [ ] Connection pooling for data fetching
- [ ] HTTP/2 push for island bundles

### P3.3 Client optimizations
- [ ] Island bundle tree-shaking
- [ ] Shared runtime chunk (deduplicate signal/hook code)
- [ ] Preact signals-compatible wire format
- [ ] Islands hydration without full re-render
- [ ] Lazy island loading (dynamic import)

### P3.4 Binary size
- [ ] LTO + strip + panic=abort
- [ ] Custom allocators (mimalloc)
- [ ] Feature flags for optional runtime components
- [ ] UPX compression for distribution

**Target metrics**:
| Metric | v0.5 | v0.8 Target |
|--------|------|-------------|
| Dev reload | <100ms | <50ms |
| Production build | 30s | <10s |
| Binary size | 5MB | <2MB |
| Cold start | <50ms | <10ms |
| SSR throughput | 50K req/s | 200K req/s |
| Memory baseline | 20MB | <5MB |

---

## Phase 4: Ecosystem & Tooling (v0.9.0)

**Goal**: Production-ready toolchain with DX matching or exceeding Fresh.

### P4.1 CLI enhancements
- [ ] `runts add` command (add island, component, route)
- [ ] `runts test` command (run tests in HIR interpreter)
- [ ] `runts fmt` command (format TS/TSX)
- [ ] `runts lint` command (subset-specific linting)
- [ ] `runts deploy` command (deploy to Vercel/Netlify/Fly)
- [ ] `runts preview` command (preview production build locally)
- [ ] `runts analyze` command (bundle size analysis)

### P4.2 IDE support
- [ ] VS Code extension
  - [ ] Syntax highlighting
  - [ ] Error diagnostics
  - [ ] Go-to-definition
  - [ ] Auto-import
  - [ ] Refactoring
- [ ] LSP server for subset validation
- [ ] rust-analyzer integration for generated code

### P4.3 Debugging
- [ ] HIR-level debugger (breakpoints in TS source)
- [ ] Source maps (TS → HIR → Rust)
- [ ] Performance profiler (component render times)
- [ ] Memory profiler (leak detection in signals)

### P4.4 Templates
- [ ] Official starter templates
  - [ ] Minimal
  - [ ] Blog
  - [ ] E-commerce
  - [ ] Dashboard
  - [ ] Documentation site
- [ ] Community template registry

### P4.5 Plugin system
- [ ] Plugin API (dynamic libraries)
- [ ] Tailwind CSS plugin
- [ ] MDX plugin
- [ ] Image optimization plugin
- [ ] Internationalization plugin

---

## Phase 5: Full Coverage (v1.0.0)

**Goal**: 100% Fresh compatibility + unique runts advantages.

### P5.1 Advanced features
- [ ] Edge runtime compatibility (Cloudflare Workers, Deno Deploy)
- [ ] Streaming SSR with Suspense-like semantics
- [ ] Server-sent events (SSE) integration
- [ ] WebSocket support (server + client)
- [ ] Background jobs / cron tasks
- [ ] Database integration (Prisma-like ORM)
- [ ] Authentication / authorization patterns
- [ ] Multi-tenancy support

### P5.2 Unique runts features
- [ ] Compile-time A/B test routing
- [ ] Static site generation (SSG) mode
- [ ] Edge-side rendering (ESR)
- [ ] WASM module integration (Rust crates as islands)
- [ ] Native module system (import Rust crates directly in TS)
- [ ] GraphQL integration
- [ ] tRPC-like end-to-end types

### P5.3 Documentation
- [ ] Complete API reference
- [ ] Migration guide (Fresh → runts)
- [ ] Migration guide (Next.js → runts)
- [ ] Performance tuning guide
- [ ] Deployment guides (all platforms)
- [ ] Video tutorials
- [ ] Interactive playground

---

## Milestone Summary

| Version | Phase | Theme | Key Deliverable |
|---------|-------|-------|-----------------|
| v0.5.x | Phase 0 | Foundation | Core compiler + runtime |
| v0.6.0 | Phase 1 | Stabilization | Working example blog |
| v0.7.0 | Phase 2 | Compatibility | 95% Fresh API parity |
| v0.8.0 | Phase 3 | Performance | Fastest Rust SSR |
| v0.9.0 | Phase 4 | Ecosystem | Production toolchain |
| v1.0.0 | Phase 5 | Maturity | Full coverage + unique features |

---

## Decision Log

### Why no SWC/tree-sitter?
- Hand-written parser is <5% the binary size
- Full control over error messages for the subset
- No JS/WASM dependency
- Easier to extend for Fresh-specific constructs

### Why HIR interpreter for dev mode?
- Compilation is 5-30s; interpretation is <100ms
- Same HIR guarantees parity between dev and production
- Enables interactive debugging at HIR level
- Simpler hot reload (just swap AST node)

### Why vanilla JS instead of Preact on client?
- Islands are small; Preact runtime is 3-4KB (overhead)
- Direct DOM manipulation is faster for small islands
- No VDOM diffing = less CPU on client
- Signals handle reactivity directly

### Why Axum instead of Actix or Warp?
- Tower middleware ecosystem
- Excellent async/await ergonomics
- Fastest Rust HTTP framework benchmarks
- First-class Tokio integration

---

## Contributing

Priority areas for external contributions:

1. **Parser**: Add more TS edge cases, improve error recovery
2. **Codegen**: Fix array/string method translations, add more standard library coverage
3. **Client runtime**: Optimize signal graph, add more hydration strategies
4. **Tests**: Write integration tests, property-based tests
5. **Documentation**: Improve docs, write tutorials
6. **Examples**: Build real-world example applications
