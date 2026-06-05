# Tasks: Ink Parity - 100% Coverage

## Task Index

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| 012-01 | Audit all 88 Ink examples for compatibility | 🔄 In Progress | Critical |
| 012-02 | Run comprehensive parity test across all examples | 📋 Pending | Critical |
| 012-03 | Fix failing examples (unsupported hooks) | 📋 Pending | High |
| 012-04 | Enhance test harness with better diff reporting | 📋 Pending | High |
| 012-05 | Add unit tests for unsupported features | 📋 Pending | High |
| 012-06 | Create missing examples for edge cases | 📋 Pending | Medium |
| 012-07 | Run final parity test and verify 100% | 📋 Pending | Critical |
| 012-08 | Commit and push | 📋 Pending | Critical |

---

## Task 012-01: Audit All 88 Ink Examples

**Description:** Review each Ink example to understand:
- What features/hooks it uses
- Whether it will work in HIR runtime
- Whether it will work in compiled binary
- Any known limitations

**Steps:**
1. List all examples
2. Categorize by feature usage
3. Identify examples that use unsupported hooks
4. Document expected behavior per environment

**Output:** `tasks/012-01-audit.md`

---

## Task 012-02: Run Comprehensive Parity Test

**Description:** Execute the full parity test harness across all 88 examples in all 3 environments.

**Steps:**
1. Run `./run_parity_tests.sh --per-symbol --output-dir results/`
2. Collect output from all environments
3. Calculate similarity scores
4. Generate diff reports

**Expected Output:**
- `results/summary.txt` - Overall summary
- `results/diff_*.txt` - Per-example diffs
- `results/deno_*.txt` - Deno output
- `results/hir_*.txt` - HIR output
- `results/compile_*.txt` - Compile output

---

## Task 012-03: Fix Failing Examples

**Description:** Address examples that fail parity testing.

**Common Issues:**
1. `useInput` not supported in HIR runtime
2. `useStdin` not supported
3. `useFocus` not supported
4. Complex hooks interactions

**Solutions:**
1. For HIR runtime limitations - Document and skip
2. For compile-time issues - Fix in runts-ink
3. For missing features - Add to runts-ink

---

## Task 012-04: Enhance Test Harness

**Description:** Improve the parity test harness with:

1. **Better Diff Reporting**
   - Show character-level differences
   - Highlight ANSI color differences
   - Show layout positioning

2. **Improved Failure Analysis**
   - Categorize failures (missing features vs bugs)
   - Suggest fixes for common issues
   - Track regression over time

3. **Performance Metrics**
   - Cold start time per environment
   - Memory usage
   - Output size

**Files to Modify:**
- `run_parity_tests.sh`

---

## Task 012-05: Add Unit Tests

**Description:** Add comprehensive unit tests for:

1. **HIR Runtime** (`src/hir_runtime.rs`)
   - Component rendering
   - Layout calculations
   - Hook state management

2. **Ink Components** (`crates/runts-ink/`)
   - Style application
   - Border rendering
   - Text wrapping

3. **Test Harness** (`tests/ink_parity_harness_tests.rs`)
   - Similarity calculation
   - Output normalization
   - Diff generation

**Target:** Maintain 100% test coverage on critical paths.

---

## Task 012-06: Create Missing Examples

**Description:** Add examples for edge cases not covered:

1. **Complex Layouts**
   - Nested flex containers
   - Dynamic sizing
   - Percentage-based dimensions

2. **Advanced Styling**
   - Gradient text (if supported)
   - Custom borders
   - Multi-line text

3. **Hook Combinations**
   - Multiple useState
   - useEffect + useMemo
   - Context + hooks

---

## Task 012-07: Final Parity Test

**Description:** Run the definitive parity test to confirm 100% coverage.

**Success Criteria:**
- All 88 examples run in deno ✅
- All 88 examples run in HIR runtime ✅
- All 88 examples build with ratatui ✅
- Similarity score ≥60% for all examples ✅
- No regressions from previous commit

---

## Task 012-08: Commit and Push

**Description:** Commit all changes and push to repository.

**Steps:**
1. Review all changes
2. Write comprehensive commit message
3. Tag with version (e.g., v0.1.1-ink-parity)
4. Push to remote

**Files to Commit:**
- New/modified examples
- Test harness improvements
- Unit tests
- Documentation updates
