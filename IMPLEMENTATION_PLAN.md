# runts Implementation Plan

**Status**: Active Development  
**Priority**: Fresh Compatibility First, Then Performance  
**Goal**: 95% Fresh Pattern Coverage

---

## Executive Summary

This plan addresses the critical gaps in the runts implementation to achieve Full Fresh compatibility:

1. **Dev Server SSR** - Currently returns placeholder, needs actual component rendering
2. **Island Hydration** - No client-side JS generation for interactive components
3. **Route/Handler Wiring** - Handlers don't connect to page components
4. **Layout System** - Layouts detected but not rendered
5. **Middleware Pipeline** - Middleware not wired into Axum

---

## Phase 1: Complete Dev Server SSR (Critical)

### Problem
The dev server currently returns a placeholder HTML page instead of actually rendering components.

### Solution
Create a proper SSR pipeline:
1. Parse routes at startup
2. Match incoming requests to routes
3. Execute handler (if present) to get data
4. Render component with data
5. Wrap with layouts
6. Inject island placeholders
7. Return HTML

### Implementation

```
src/
├── dev/
│   ├── mod.rs              # Dev module
│   ├── ssr.rs              # SSR renderer
│   ├── routes.rs           # Route matching
│   ├── layouts.rs          # Layout composition
│   └── islands.rs          # Island placeholder injection
```

### Files to Create/Modify

1. **Create** `src/dev/mod.rs` - Dev SSR orchestrator
2. **Create** `src/dev/ssr.rs` - Server-side rendering
3. **Create** `src/dev/routes.rs` - Route matching and dispatch
4. **Create** `src/dev/layouts.rs` - Layout composition
5. **Modify** `src/commands/dev.rs` - Wire up SSR pipeline

### Acceptance Criteria
- [ ] `runts dev` renders actual route components
- [ ] Route handlers execute and pass data to components
- [ ] Layouts wrap page content correctly
- [ ] Islands render as static HTML with placeholder attributes
- [ ] HMR updates rendered output on file changes

---

## Phase 2: Island Hydration (Critical)

### Problem
Islands are rendered on server but never hydrated on client.

### Solution
Generate client-side TypeScript bundles for each island:
1. Parse island TSX files
2. Generate client-compatible TypeScript
3. Include Preact runtime hooks (simplified)
4. Ship bundled JS to client
5. Browser hydrates islands from SSR placeholders

### Implementation

```
src/
├── client/
│   ├── runtime.ts          # Client-side runtime
│   ├── hydrate.ts          # Island hydration
│   ├── signals.ts          # Client-side signals
│   └── hooks.ts            # Client-side hooks
```

### Island Manifest

```json
{
  "islands": {
    "Counter": {
      "hash": "a1b2c3d4",
      "props": ["initial", "step", "label"],
      "module": "/_runts/islands/Counter.a1b2c3d4.js"
    }
  }
}
```

### Files to Create

1. **Create** `crates/runts-client/src/runtime.ts` - Client runtime
2. **Create** `crates/runts-client/src/hydrate.ts` - Island hydration
3. **Create** `crates/runts-client/src/signals.ts` - Client signals
4. **Create** `crates/runts-client/src/hooks.ts` - Client hooks
5. **Create** `crates/runts-client/src/bundler.ts` - Island bundler
6. **Modify** `src/dev/ssr.rs` - Inject hydration scripts

### Acceptance Criteria
- [ ] Islands hydrate correctly on client
- [ ] Event handlers attach to DOM elements
- [ ] State persists across hydration
- [ ] Islands work in isolation

---

## Phase 3: Route/Handler Wiring (High Priority)

### Problem
Route handlers generate skeleton code but don't actually execute.

### Solution
Implement proper handler execution:
1. Extract handler from route file
2. Execute handler function (compiled to Rust)
3. Pass response data to component render
4. Handle errors gracefully

### Files to Modify

1. **Modify** `src/transpile/routegen.rs` - Generate proper Axum handlers
2. **Modify** `src/transpile/codegen.rs` - Generate component wrappers
3. **Create** `src/runtime/handler.rs` - Handler execution runtime

### Acceptance Criteria
- [ ] GET handlers execute and return data
- [ ] POST/PUT/DELETE handlers work
- [ ] `ctx.params` contains route parameters
- [ ] `ctx.render()` passes data to component
- [ ] 404/500 errors handled correctly

---

## Phase 4: Layout System (High Priority)

### Problem
`_layout.tsx` files are detected but not rendered.

### Solution
Implement layout composition:
1. Detect layout hierarchy from file paths
2. Render layouts from outermost to innermost
3. Pass children to layout components
4. Support `_app.tsx` as global wrapper

### Files to Create/Modify

1. **Create** `src/dev/layouts.rs` - Layout composition
2. **Modify** `src/transpile/routegen.rs` - Generate layout wrappers

### Acceptance Criteria
- [ ] `/blog` uses `routes/blog/_layout.tsx`
- [ ] `/blog/[slug]` uses both blog layout and root layout
- [ ] Layouts receive `children` prop
- [ ] `_app.tsx` wraps entire app

---

## Phase 5: Middleware Pipeline (Medium Priority)

### Problem
Middleware files detected but not executed.

### Solution
Wire middleware into Axum:
1. Collect middleware from `_middleware.ts` files
2. Generate middleware chain
3. Apply to Axum router
4. Support `ctx.state` for data sharing

### Files to Modify

1. **Modify** `src/transpile/middlewaregen.rs` - Generate proper middleware
2. **Modify** `src/commands/dev.rs` - Apply middleware to router

### Acceptance Criteria
- [ ] Global `_middleware.ts` runs on all requests
- [ ] `ctx.state` shares data between middleware and handlers
- [ ] Middleware can short-circuit requests
- [ ] Response headers modified by middleware

---

## Phase 6: Static Assets (Low Priority)

### Problem
Static files not properly served.

### Solution
Configure tower-http `ServeDir`:
1. Mount `static/` directory
2. Set proper cache headers
3. Support SPA fallback for client-side routing

### Files to Modify

1. **Modify** `src/commands/dev.rs` - Add static file serving

### Acceptance Criteria
- [ ] `/static/*` serves files from `static/` directory
- [ ] CSS, JS, images load correctly
- [ ] Cache headers set appropriately

---

## Implementation Order

```
Week 1: Phase 1 (Dev Server SSR) + Phase 3 (Route Wiring)
Week 2: Phase 2 (Island Hydration)  
Week 3: Phase 4 (Layouts) + Phase 5 (Middleware)
Week 4: Phase 6 (Static) + Polish + Testing
```

---

## Technical Details

### SSR Pipeline

```
Request → Middleware → Route Match → Handler Execute 
    → Component Render → Layout Compose → Island Inject 
    → HTML Response
```

### Island Placeholder Format

```html
<div 
  data-island="Counter" 
  data-id="island-abc123"
  data-props='{"initial":0,"step":1}'
  data-hash="a1b2c3d4"
>
  <!-- Server-rendered HTML -->
  <button class="counter-btn">0</button>
</div>

<script type="module">
  import { hydrate } from '/_runts/islands/Counter.a1b2c3d4.js';
  hydrate('island-abc123', { initial: 0, step: 1 });
</script>
```

### Route Pattern Matching

| File | Pattern | Method |
|------|---------|--------|
| `routes/index.tsx` | `/` | GET |
| `routes/blog/[slug].tsx` | `/blog/:slug` | GET |
| `routes/api/posts.ts` | `/api/posts` | GET, POST |
| `routes/[...catchall].tsx` | `/*` | GET |

---

## Performance Targets

| Metric | Target | Current |
|--------|--------|---------|
| HMR latency | < 100ms | N/A |
| Dev cold start | < 200ms | ~500ms |
| SSR time | < 5ms | N/A |
| Binary size | < 2MB | 2.6MB |
| Memory (idle) | < 5MB | N/A |

---

## Testing Strategy

1. **Unit Tests**: Parser, codegen, analyzer (existing)
2. **Integration Tests**: Full route rendering
3. **E2E Tests**: Browser automation with Playwright
4. **Snapshot Tests**: HTML output comparison

---

## Dependencies

### Runtime Crates
- `axum` - HTTP server
- `tokio` - Async runtime
- `notify` - File watching
- `serde` - Serialization
- `regex` - Route matching

### Dev Crates
- `swc_ecma_parser` - TypeScript parsing (optional, we use custom parser)

### Build Crates
- `cargo` - Rust compilation (external)
- `tempfile` - Temporary files for code gen

---

*Last Updated: 2026-05-26*
