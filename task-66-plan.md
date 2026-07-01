# Task 66: Sixth Review Findings - Implementation Plan

## Analysis Summary

After analyzing the codebase and the review findings, here's what I found:

### Rank 1: Replace Custom Subsystems with Crates

| # | Item | Current State | Status | Effort | Impact |
|---|------|---------------|--------|--------|--------|
| 1 | Parser (swc) | Working | ✅ Keep | - | - |
| 2 | JSON | Using `serde_json` | ✅ Done | - | - |
| 3 | Regex | Custom `RegExp` impl | ⚠️ Keep for now | High | Low |
| 4 | Diagnostics | String errors | 🔲 Pending | Medium | Medium |
| 5 | String interning | No interning | 🔲 Deferred | High | Medium |
| 6 | Ordered maps | Using `indexmap` | ✅ Done | - | - |
| 7 | BigInt | Not implemented | 🔲 Deferred | Medium | Low |
| 8 | Allocation | Rc/RefCell | 🔲 Deferred | High | Medium |
| 9 | Fast hashing | Using `rustc-hash` | ✅ Done | - | - |
| 10 | Errors (thiserror) | Custom impl | 🔲 Quick win | Low | Medium |

### Rank 2: Unify Duplicated Logic

| # | Item | Status | Notes |
|---|------|--------|-------|
| 11 | Unify call paths | 🔲 Quick win | `call_function` → wrapper around `call_value_with_this` |
| 12 | Unify ToPrimitive | 🔲 Quick win | Extract shared `to_primitive` function |
| 13 | Unify member access | 🔲 Medium | Create `get_property`/`set_property` |
| 14 | Unify param binding | 🔲 Medium | Build "bind parameters" routine |
| 15 | Unify Program | 🔲 Low | `Program::Script`/`Program::Module` |

### Rank 3: Architecture Simplifications

| # | Item | Status | Notes |
|---|------|--------|-------|
| 16 | Reactive HIR nodes | ✅ Removed | Task 56 removed them |
| 17 | ANF pass | 🔲 Deferred | Future AOT work |
| 18 | Seal public API | 🔲 Quick win | Remove extra exports |
| 19 | Environment frames | 🔲 Deferred | Architecture cleanup |
| 20 | Compiler AST transform | 🔲 Deferred | String replace works for now |

## Quick Wins to Implement

### 1. Seal public API (lib.rs) - 10 minutes
Remove extra exports from `lib.rs`, keep only:
- `Context`
- `Value`
- `Program`
- `JsError`
- Host registration traits

### 2. Unify call paths - 15 minutes
Make `call_function` a thin wrapper around `call_value_with_this`.

### 3. Convert JsError to thiserror - 30 minutes
Replace custom error enum with `thiserror` for better ergonomics.

## Medium-Effort Items

### 4. Unify ToPrimitive - 1 hour
Extract `to_primitive` function used by `==`, `+`, template literals.

### 5. Unify member access - 2 hours
Create `get_property`/`set_property` functions.

### 6. Add source spans with ariadne - 2 hours
Replace string errors with rich diagnostics.

## Deferred Items (Future Phases)

- String interning (high effort, medium impact)
- Arena allocation with bumpalo (high effort, medium impact)
- BigInt support (medium effort, low impact for Ink)
- Raw-pointer environment replacement (medium effort)

## Recommendation

**Start with the quick wins** (items 18, 11, 10 from Rank 3/2/1) to reduce noise without risk, then tackle the medium-effort items that improve correctness (items 12, 13, 4).
