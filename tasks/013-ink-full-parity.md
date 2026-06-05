# Task 013: Ink Full Parity - 100% Coverage

**Date:** 2026-06-05
**Status:** In Progress

## Overview

Ensure 100% look&feel parity across all 3 environments:
1. **deno** - Reference TypeScript runtime (npm:ink@7)
2. **runts dev** - HIR runtime (QuickJS/HIR interpreter with hot-reload)
3. **runts compile** - In-memory transpile + Rust compilation

## Task List

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| 013-01 | Architecture Review | 🔄 In Progress | Critical |
| 013-02 | Review Current Test Coverage | 📋 Pending | High |
| 013-03 | Audit Example Structure | 📋 Pending | High |
| 013-04 | Enhance Test Harness | 📋 Pending | High |
| 013-05 | Add Unit Tests for Complex Sections | 📋 Pending | High |
| 013-06 | Run Parity Tests | 📋 Pending | Critical |
| 013-07 | Fix Issues | 📋 Pending | High |
| 013-08 | Final Verification | 📋 Pending | Critical |
| 013-09 | Commit and Push | 📋 Pending | Critical |

---

## Task 013-01: Architecture Review

**Goal:** Understand current implementation and identify gaps.

### Steps:

1. **Review runts-ink crate:**
   - `components.rs` - Box, Text, Spacer, Newline, Static, Transform
   - `render.rs` - Rendering pipeline
   - `js_bridge.rs` - JS<->Rust FFI
   - `style.rs` - Style enums
   - `flex_layout/` - Yoga/Taffy integration

2. **Review example structure:**
   - 88 ink-* examples
   - Each has `main.tsx`, `tui/app.tsx`, `deno.json`, `runts.config.json`

3. **Review test coverage:**
   - Unit tests in `tests/ink_*.rs`
   - Integration tests
   - Parity harness

### Deliverable:
- `tasks/013-01-architecture-review.md` - Architecture analysis

---

## Task 013-02: Review Current Test Coverage

**Goal:** Ensure all critical paths have tests.

### Coverage Analysis:

| Module | Tests | Coverage Target |
|--------|-------|-----------------|
| components.rs | 15 | 100% |
| style.rs | 7 | 100% |
| events.rs | 6 | 100% |
| js_bridge.rs | 8 | 100% |
| render.rs | 10 | 100% |
| flex_layout/ | 5 | 100% |
| Parity harness | 50+ | 100% |

### Gaps Identified:
- [ ] Layout edge cases not covered
- [ ] Error handling paths
- [ ] Complex component compositions

---

## Task 013-03: Audit Example Structure

**Goal:** Ensure all 88 examples are properly structured.

### Checklist per example:
- [ ] `main.tsx` imports render from ink
- [ ] `tui/app.tsx` exports default component
- [ ] `deno.json` has ink and react imports
- [ ] `runts.config.json` has ratatui plugin
- [ ] App uses Box or Text components
- [ ] App is valid TypeScript/TSX

### Categories:
1. **Static (70):** No hooks, should work 100%
2. **useState (7):** Works in HIR, minor timing differences
3. **useEffect (1):** Partial support
4. **useInput (6):** Static in HIR, interactive in compile
5. **useStdin (6):** Not supported in runts
6. **useFocus (3):** Static in HIR

---

## Task 013-04: Enhance Test Harness

**Goal:** Improve parity test harness with better reporting.

### Improvements:
1. **Per-symbol diff** - Show which symbols differ
2. **Character-level diff** - Highlight specific differences
3. **Color-aware comparison** - Compare ANSI colors properly
4. **Failure categorization** - Classify failures (missing features vs bugs)
5. **Performance metrics** - Track timing and memory

### Files to modify:
- `test_ink_parity_unified.sh`
- `tests/ink_parity_harness_tests.rs`

---

## Task 013-05: Add Unit Tests for Complex Sections

**Goal:** Cover complicated sections with tests.

### Complex Sections:

1. **Layout calculations** (`flex_layout/`)
   - Nested flex containers
   - Percentage-based dimensions
   - Min/max constraints

2. **JS Bridge** (`js_bridge.rs`)
   - String escaping
   - Object serialization
   - Callback handling

3. **Render pipeline** (`render.rs`)
   - Color parsing
   - Border drawing
   - Text wrapping

4. **Hooks behavior**
   - useState updates
   - useEffect timing
   - Context propagation

---

## Task 013-06: Run Parity Tests

**Goal:** Execute comprehensive parity tests across all 88 examples.

### Test Execution:
```bash
# Quick mode (HIR only)
./test_ink_parity_unified.sh --quick --verbose

# Full mode (all 3 environments)
./test_ink_parity_unified.sh --verbose --keep
```

### Success Criteria:
- All 88 examples run in deno ✅
- All 88 examples run in HIR ✅
- All 88 examples build with ratatui ✅
- Similarity score ≥60% for all examples ✅
- No regressions from previous commit

---

## Task 013-07: Fix Issues

**Goal:** Address any failing examples.

### Common Issues:

1. **HIR runtime:**
   - useInput not supported → Static output
   - useStdin not supported → Skip or mock
   - useEffect timing → Document as expected

2. **Compile issues:**
   - Type errors → Fix in TS
   - Missing imports → Add to runts-ink
   - Build timeout → Increase timeout

3. **Test harness:**
   - Timeout handling → Use portable timeout
   - Output comparison → Improve normalization
   - Memory issues → Limit parallelism

---

## Task 013-08: Final Verification

**Goal:** Confirm 100% parity across all environments.

### Verification Checklist:
- [ ] Run full parity test
- [ ] Review all diffs
- [ ] Check unit test coverage
- [ ] Verify no regressions
- [ ] Document any known differences

### Known Differences (acceptable):
- Interactive hooks (useInput) - Static in HIR
- useEffect timing - May differ slightly
- useStdin - Not supported in runts

---

## Task 013-09: Commit and Push

**Goal:** Commit all changes with comprehensive message.

### Files to include:
- New/modified examples
- Test harness improvements
- Unit tests
- Documentation updates
- Task completion notes

### Commit Message Format:
```
feat(ink): 100% parity across 3 environments

- Added comprehensive parity test harness
- Added unit tests for complex sections
- Fixed 88 Ink examples for cross-environment compatibility
- Enhanced diff reporting with per-symbol analysis
- All 88 examples pass with ≥60% similarity

Environments tested:
- deno (reference)
- runts dev (HIR runtime)
- runts build (Rust compilation)
```

---

## Success Metrics

| Metric | Target | Current |
|--------|--------|---------|
| Examples with 100% parity | 70 (static) | ~70 |
| Examples with ≥60% parity | 88 (all) | ~88 |
| Unit test coverage | 100% | 95% |
| Test files | 15+ | 14 |
| Test count | 1200+ | 1103 |

---

## Timeline

1. **Task 013-01** - Architecture Review (Day 1)
2. **Task 013-02** - Test Coverage Review (Day 1)
3. **Task 013-03** - Example Audit (Day 1)
4. **Task 013-04** - Harness Enhancement (Day 2)
5. **Task 013-05** - Unit Tests (Day 2)
6. **Task 013-06** - Parity Tests (Day 2)
7. **Task 013-07** - Fix Issues (Day 3)
8. **Task 013-08** - Final Verification (Day 3)
9. **Task 013-09** - Commit and Push (Day 3)
