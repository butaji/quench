# Runts Implementation Plan - Final Phase

## Executive Summary

**Goal**: Complete MVP and achieve full Fresh/Preact compatibility for production deployment.

**Current Status**: ~90% complete. Core infrastructure is solid. Remaining work focuses on:
1. Full middleware runtime execution
2. Parallel transpilation
3. Error handling polish

---

## Phase 1: Middleware Runtime (P0 - Critical)

### Current State
- Middleware extraction: ✅ Complete
- Code generation: ✅ Complete
- **Dev mode execution**: ⚠️ Partial (basic body execution, missing full pipeline)

### Implementation

```rust
// src/runtime/middleware.rs - NEW FILE
// Full middleware pipeline execution for dev mode

pub fn execute_middleware_chain(
    &self,
    middleware: &[MiddlewareInfo],
    request: &Request,
    ctx: &mut EvalContext,
) -> MiddlewareResult {
    // 1. Collect matching middleware
    // 2. Execute in order
    // 3. Handle early returns
    // 4. Pass state through chain
}
```

### Tasks
- [ ] Create `src/runtime/middleware.rs`
- [ ] Implement `execute_middleware_chain()` with full pipeline
- [ ] Handle `ctx.next()` correctly (calls next middleware)
- [ ] Handle `return ctx.next()` in handlers
- [ ] Pass `ctx.state` through middleware → handler → render
- [ ] Unit tests for middleware chaining

---

## Phase 2: Parallel Transpilation (P1 - Performance)

### Current State
- Sequential file processing in `build.rs`

### Implementation

```rust
// Use rayon for parallel processing
use rayon::prelude::*;

pub fn process_files_parallel(files: Vec<PathBuf>) -> Vec<GeneratedFile> {
    files.par_iter()
        .map(|path| transpile_file(path))
        .collect()
}
```

### Tasks
- [ ] Add `rayon` dependency to Cargo.toml
- [ ] Parallelize route transpilation
- [ ] Parallelize island transpilation
- [ ] Parallelize component transpilation
- [ ] Benchmark before/after

---

## Phase 3: Error Handling Polish (P1 - DX)

### Current State
- Basic error messages with file/line info

### Improvements

```rust
// Suggest "Did you mean..." for typos
pub fn suggest_correction(word: &str, valid: &[&str]) -> Option<String> {
    // Levenshtein distance
    // Return closest match if distance <= 3
}

// Format error with source context
pub fn format_error(e: &ParseError, source: &str) -> String {
    // Show line with caret pointing to error
    // Show relevant snippet
}
```

### Tasks
- [ ] Add Levenshtein distance utility
- [ ] Implement "Did you mean..." suggestions
- [ ] Add source context to errors
- [ ] Highlight error location in source

---

## Phase 4: Production Build Optimization (P1 - Performance)

### Tasks
- [ ] Incremental builds (only changed files)
- [ ] Binary size optimization (<2MB target)
- [ ] Startup time optimization (<5ms target)

---

## Phase 5: Documentation (P1 - DX)

### Tasks
- [ ] API documentation for public types
- [ ] Migration guide from Fresh
- [ ] Example projects (blog, dashboard, e-commerce)
- [ ] Performance benchmarking suite

---

## Implementation Timeline

| Week | Focus | Deliverables |
|------|-------|--------------|
| 1 | Middleware Runtime | Full pipeline execution, tests |
| 2 | Parallel Transpilation | rayon integration, benchmarks |
| 3 | Error Polish | Better messages, source context |
| 4 | Build Optimization | Incremental builds, size targets |
| 5 | Documentation | API docs, migration guide |

---

## Technical Decisions

### 1. Middleware State Type
```typescript
// Current: generic HashMap
ctx.state: HashMap<String, Value>

// Proposed: Typed state with inferred types
interface State {
    user?: User;
    session?: Session;
}
```

### 2. Hydration Strategy
```typescript
// Island hydration options
type HydrationStrategy = 'load' | 'visible' | 'idle' | 'interaction';
```

### 3. Signal Granularity
```rust
// Current: Fine-grained with dependency tracking
// Sufficient for 95% of use cases
```

---

## Risk Mitigation

| Risk | Mitigation |
|------|------------|
| Middleware complexity | Start simple, add features incrementally |
| Parallel transpile race conditions | Use Arc<RwLock<>> for shared state |
| Binary size bloat | Monitor with `cargo bloat`, strip aggressively |

---

## Success Criteria

- [ ] `runts dev` serves routes with full middleware execution
- [ ] `runts build` produces <2MB binary
- [ ] Build time <5s for 100 files (parallel)
- [ ] Error messages include suggestions
- [ ] Full Fresh blog example runs without modifications

---

*Last Updated: 2026-05-26*
