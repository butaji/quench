# runts — Development Roadmap

## Versioning Strategy

- **Minor versions (0.x.0):** Feature milestones with breaking changes to supported subset
- **Patch versions (0.x.y):** Bug fixes, performance improvements, compatibility fixes
- **1.0.0:** Full Fresh feature parity + stable ABI

---

## Phase 0: Foundation ✅ COMPLETE

**Goal:** Working transpiler with core runtime

- [x] Recursive descent TS/TSX parser
- [x] HIR (High-level IR) definition
- [x] Type analyzer with inference
- [x] Code generator (HIR → Rust)
- [x] `html!` proc macro for JSX
- [x] VDOM types and rendering
- [x] Signal system (fine-grained reactivity)
- [x] Hook implementations (useState, useEffect, useRef, useMemo, useCallback, useReducer, useContext, useId)
- [x] Component system with `#[component]` macro
- [x] Islands architecture (detection, SSR placeholders, hydration)
- [x] File-based routing
- [x] Route handlers (GET, POST, PUT, DELETE, PATCH)
- [x] Middleware pipeline
- [x] Layout system (`_layout.tsx`)
- [x] Error pages (`_404.tsx`, `_500.tsx`)
- [x] Dev server with HIR interpreter
- [x] File watcher + hot reload
- [x] Production build command
- [x] CLI (`init`, `dev`, `build`, `transpile`, `add`)
- [x] Example blog application

**Status:** All complete. MVP is functional.

---

## Phase 1: Stability & Polish (v0.5.x → v0.6.0) 🔄 CURRENT

**Goal:** Fix critical bugs, improve DX, solidify codegen correctness

### 1.1 Codegen Correctness
- [ ] Fix type name preservation (PascalCase types, not snake_case)
- [ ] Fix destructuring codegen for nested patterns
- [ ] Fix `prev` reference in array update closures
- [ ] Fix import resolution for relative paths in generated code
- [ ] Fix component name collision in generated modules
- [ ] Ensure all generated code passes `cargo check` on first build

### 1.2 Dev Mode Hardening
- [ ] Graceful parser error recovery (skip bad file, continue)
- [ ] Error overlay in browser (HTML error page with source context)
- [ ] Source map generation for debugging
- [ ] Console output forwarding from island JS to dev server
- [ ] HMR for CSS/static assets

### 1.3 Fresh Compatibility Gaps
- [ ] `ctx.render()` with explicit component reference
- [ ] HandlerContext full API parity
- [ ] Route groups `(group)/page.tsx`
- [ ] Catch-all routes `[...slug].tsx`
- [ ] Optional route segments `[[param]]`

### 1.4 Testing Infrastructure
- [ ] Parser test suite (100+ cases)
- [ ] Codegen snapshot tests
- [ ] End-to-end dev server tests
- [ ] Production build integration tests
- [ ] Fresh compatibility test suite (port Fresh examples)

**Target Date:** 2 weeks
**Deliverable:** `v0.6.0` — stable MVP

---

## Phase 2: Performance & Scale (v0.6.0 → v0.7.0)

**Goal:** Production-grade performance and large-project support

### 2.1 Incremental Builds
- [ ] File content hashing (SHA-256)
- [ ] `.runts/cache/` directory with parsed HIR cache
- [ ] Dependency graph for change invalidation
- [ ] Parallel codegen with `rayon`
- [ ] Only re-transpile changed + dependent files

### 2.2 Build Optimization
- [ ] Dead code elimination for unused islands
- [ ] Tree-shaking of unused routes
- [ ] CSS extraction and minification
- [ ] Asset optimization (image compression hashes)
- [ ] Service worker generation (optional)

### 2.3 Runtime Performance
- [ ] String pooling for common HTML tags
- [ ] VDOM allocation pooling (bump allocator)
- [ ] Signal dependency tracking optimization
- [ ] SSR streaming (flush chunks as generated)
- [ ] HTTP/2 push for critical assets

### 2.4 Large Project Support
- [ ] Workspace/monorepo support
- [ ] Shared component libraries
- [ ] Cross-package island imports
- [ ] Build report (bundle analysis)

**Target Date:** 4 weeks
**Deliverable:** `v0.7.0` — production ready

---

## Phase 3: Advanced Features (v0.7.0 → v0.8.0)

**Goal:** Match advanced Fresh/Preact capabilities

### 3.1 Async Components & Suspense
- [ ] `async function Component()` support
- [ ] Streaming SSR with Suspense boundaries
- [ ] `fallback` prop for loading states
- [ ] Data fetching at component level

### 3.2 Plugin System
- [ ] Pre-transpile plugin hooks
- [ ] Post-transpile plugin hooks
- [ ] Custom codegen plugins
- [ ] Markdown → TSX plugin (MDX equivalent)

### 3.3 Form Handling
- [ ] `form` action handlers
- [ ] Server-side validation
- [ ] CSRF token generation
- [ ] Progressive enhancement (form works without JS)

### 3.4 Authentication
- [ ] Cookie session middleware
- [ ] JWT verification helpers
- [ ] OAuth integration patterns
- [ ] Route guards (auth-required middleware)

### 3.5 Database Integration
- [ ] First-class Prisma-client-rust support
- [ ] Supabase client integration
- [ ] Connection pooling in middleware

**Target Date:** 6 weeks
**Deliverable:** `v0.8.0` — advanced framework

---

## Phase 4: Ecosystem (v0.8.0 → v0.9.0)

**Goal:** Rich developer ecosystem

### 4.1 IDE Integration
- [ ] LSP server for `.tsx` files
- [ ] Type error reporting in VS Code
- [ ] Auto-import for components
- [ ] Go-to-definition across TS/Rust boundary
- [ ] Inline hints for generated Rust types

### 4.2 Testing
- [ ] `runts test` command (unit + integration)
- [ ] Component testing with headless browser
- [ ] Route handler testing with mock requests
- [ ] Snapshot testing for rendered HTML

### 4.3 Deployment
- [ ] Docker image generation
- [ ] Static site export (`runts build --static`)
- [ ] Vercel/Netlify adapter
- [ ] Cloudflare Workers adapter (WASM target)
- [ ] Kubernetes Helm chart

### 4.4 Documentation
- [ ] Interactive documentation site
- [ ] API reference (auto-generated)
- [ ] Video tutorial series
- [ ] Migration guide from Fresh
- [ ] Migration guide from Next.js

**Target Date:** 8 weeks
**Deliverable:** `v0.9.0` — ecosystem maturity

---

## Phase 5: 1.0 Release (v0.9.0 → v1.0.0)

**Goal:** Stable, trusted, production-proven

### 5.1 Stability
- [ ] Semantic versioning guarantee
- [ ] No breaking changes without migration path
- [ ] Backward compatibility policy
- [ ] LTS release channel

### 5.2 Performance Benchmarks
- [ ] SSR throughput: 100k req/s on modest hardware
- [ ] Build time: <10s for 1000-file project
- [ ] Binary size: <1MB for hello-world
- [ ] Memory: <10MB RSS at idle

### 5.3 Production Validation
- [ ] Run in production at 3+ real companies
- [ ] Load-tested to 10k concurrent connections
- [ ] 30-day uptime SLA demonstration

### 5.4 Community
- [ ] 1000+ GitHub stars
- [ ] 10+ community plugins
- [ ] Active Discord/forum
- [ ] Monthly release cadence

**Target Date:** 12 weeks from v0.9.0
**Deliverable:** `v1.0.0` — production stable

---

## Post-1.0 Vision

### Language Expansion
- [ ] Vue SFC support (alternative to TSX)
- [ ] SolidJS-style fine-grained reactivity option
- [ ] Rust-native component syntax (alternative to TSX)

### Platform Expansion
- [ ] iOS/Android via Tauri/Capacitor
- [ ] Desktop apps via Tauri
- [ ] WebAssembly edge functions
- [ ] Embedded systems (no_std target)

### AI Integration
- [ ] AI-powered component generation
- [ ] Automatic test generation
- [ ] Smart migration assistant (Fresh → runts)
- [ ] Performance bottleneck detection

---

## Milestone Summary

| Version | Theme | Key Deliverable |
|---------|-------|-----------------|
| v0.5.0 | MVP | Working transpiler + dev server |
| v0.6.0 | Stability | Bug-free codegen, test suite |
| v0.7.0 | Performance | Incremental builds, <2s builds |
| v0.8.0 | Advanced | Suspense, plugins, auth, forms |
| v0.9.0 | Ecosystem | LSP, testing, deployment adapters |
| v1.0.0 | Stable | Production SLA, LTS, community |

---

## Contributing Focus Areas

**Good first issues:**
- Parser error message improvements
- Additional test cases
- Documentation improvements
- Example projects

**High impact:**
- Incremental build caching
- LSP server
- Plugin system architecture
- Deployment adapters

**Research:**
- WASM target for edge functions
- Alternative reactivity backends (Leptos-style)
- Server Components architecture
