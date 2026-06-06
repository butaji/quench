# runts-ink: Execution Guide

> **Architecture:** rquickjs (dev engine) + Yoga (layout) + Ratatui (render).
> **HIR interpreter:** DELETED. Do not restore.
> **Taffy:** Being removed. Yoga is the sole layout engine.
> **Goal:** 100% look&feel parity across 3 environments for all 89 Ink examples.

---

## The 3 Environments

| # | Environment | What it is | How to invoke |
|---|-------------|-----------|---------------|
| 1 | **deno** | Reference TypeScript runtime (npm:ink) | `deno run -A main.tsx` |
| 2 | **rq** (runts dev) | TSX → JS (oxc_codegen) → rquickjs + Yoga bridge → render | `runts dev --once --plugin ratatui ./example` |
| 3 | **compile** (runts build) | TSX → HIR → Rust codegen → `cargo build --release` | `runts build --release --plugin ratatui ./example` |

**Parity means:** The rendered text output (after ANSI normalization) is identical across all 3 environments for every example.

---

## Workflow

All work is tracked in `tasks/`. Check `tasks/index.json` for the current task breakdown, priorities, and statuses. Each task has a matching `tasks/xxx-title.md` file with acceptance criteria and implementation notes.

**The rule:** Pick one pending task, implement it fully, verify acceptance criteria, commit, push. Do not batch multiple tasks into a single commit.

### Before you start any task

1. Read `tasks/index.json` to find the next pending task.
2. Read its matching `tasks/xxx-title.md` for acceptance criteria.
3. Run `cargo build` to confirm the current state.

### After you finish a task

1. Verify acceptance criteria from the task file.
2. Update `tasks/index.json` to mark the task completed.
3. `git add -A && git commit -m "brief description"`
4. `git push origin fresh`

---

## Phases (Conceptual)

These phases describe the overall flow. The exact task files in `tasks/` map to these phases and may be updated as work progresses.

### Phase 0: Unblock
**Goal:** `cargo build` passes. Linter is enforced.

- Fix the build syntax error (brace mismatch in codegen).
- Re-enable the linter in `build.rs` and mechanically fix all violations until the build passes with zero linter errors.
- The linter limits are non-negotiable: file ≤ 500 lines, function ≤ 40 lines, complexity ≤ 10. Extract, don't negotiate.

**Acceptance:** `cargo build` exits 0. `cargo test --no-run` exits 0.

---

### Phase 1: rquickjs + Yoga Engine
**Goal:** `runts dev --once` renders any example identically to deno.

Execute in this order:

1. **Delete the HIR runtime.** Remove `src/hir_runtime.rs` and all references (`Interpreter`, `Value`, `render_tsx`, `RuntimeError`) from `src/main.rs`, `src/cli.rs`, and anywhere else. Do not preserve it as a fallback — it is dead code.
2. **Remove Taffy.** Delete the Taffy module and feature flag. Make Yoga the default and only layout feature in `Cargo.toml`. Strip all `#[cfg(feature = "taffy")]` conditionals.
3. **Build the TSX→JS transpiler.** Use `oxc_codegen` (already in deps). It must: (a) desugar JSX to `React.createElement`, (b) erase TS type annotations, (c) rewrite `import { Box, Text } from 'ink'` to bridge globals.
4. **Wire `runts dev` to rquickjs.** The dev command should: parse `.tsx` → transpile to JS → create rquickjs context → inject `js_bridge.rs` globals → inject React shim → eval bundle → call `renderToString()` → print output.
5. **Complete the JS bridge.** Every prop used in any of the 89 examples must be supported in `js_bridge.rs`. Use a prop dispatch table or macro — do not write 300-line match blocks.
6. **Wire interactive hooks.** `useInput`, `useApp`, `useFocus`, etc. run inside rquickjs. The bridge only exposes Rust primitives (VNode builders, event sources). Crossterm events route to JS callbacks.

**Acceptance per step:**
- After deletion steps: `cargo build` passes with zero warnings.
- Transpiler: `examples/ink-text-props/tui/app.tsx` produces runnable JS.
- Dev command: `runts dev --once --plugin ratatui examples/ink-text-props` prints the same text as `deno run -A examples/ink-text-props/main.tsx`.
- Bridge: `grep -r 'unsupported prop' tests/` returns nothing.
- Interactive: `runts dev --once examples/ink-counter` renders and responds to `q` / arrow keys.

---

### Phase 2: Compile + Verification
**Goal:** `runts build --release` produces working binaries. One parity harness runs all 89 examples.

- **Fix the compile path.** Replace the plugin JSON string boundary with typed HIR transfer. Fix `find_runts_lib_path` to use `env!("CARGO_MANIFEST_DIR")`. Verify that static examples build and run.
- **Create one parity harness.** Delete all existing `test_parity*.sh` scripts. Create a single `scripts/parity.sh` with `--env deno|rq|compile|all`, `--examples GLOB`, `--once`. It must implement per-symbol diff and output a JSON summary.
- **Add per-example unit tests.** Generate one Rust test per example in `tests/rq_parity/`. Each test reads `examples/*/tui/app.tsx`, runs it through the rquickjs path, and asserts expected substrings in output.
- **Fix HIR test failures.** `cargo test --bin runts` currently has 113 failures in compile-path tests. Categorize each: fix if compile path needs it, `#[ignore]` if out of scope, delete if testing removed subsystems.

**Acceptance:**
- Compile path produces a binary that exits 0 and prints expected text.
- Parity harness runs all 89 examples and produces a JSON report.
- Unit tests cover ≥90% of examples.
- `cargo test --bin runts` exits 0 (or only expected ignored failures).

---

### Phase 3: Coverage Gaps
**Goal:** No feature is untested or unexercised.

- **Re-enable disabled spec tests.** Four test modules are commented out in `src/transpile/tests/mod.rs`: `spec_control_flow`, `spec_data_structures`, `spec_vars_functions`, `spec_jsx`. These cover large swaths of the TS/TSX subset. Uncomment them, fix helper visibility issues (likely `runts_hir::*` imports), and get them compiling.
- **Add missing Ink examples.** Several features are implemented in `js_bridge.rs` but never exercised by an example:
  - `useAnimation` — add `examples/ink-animation/`
  - `usePaste` — add `examples/ink-paste/`
  - `measureElement` / `useBoxMetrics` — add `examples/ink-measure/`
  - `minWidth`, `maxWidth`, `zIndex`, `flexShrink`, `alignSelf` — add `examples/ink-advanced-layout/`
- **Verify bridge completeness.** Generate a prop-coverage matrix: for every prop used in every example, confirm it is handled in `js_bridge.rs` and has a unit test.

**Acceptance:**
- Zero commented-out test modules.
- All Ink hooks and layout props are exercised by at least one example.
- Prop coverage matrix shows 100% coverage.

---

### Phase 4: Cleanup
**Goal:** Repo is clean. Docs are truthful.

- Delete dead code: `crates/runts-react/`, old scripts, unused imports. Ensure `cargo build` has zero dead-code warnings.
- Update all docs (`DESIGN.md`, `README.md`, `docs/*.md`). Remove all HIR interpreter and Taffy references. Describe rquickjs + Yoga accurately.
- Optional: evaluate Boa vs rquickjs and document decision.

**Acceptance:** Workspace builds clean. Docs do not mention HIR interpreter or Taffy.

---

## Known Coverage Gaps (Current State)

### Disabled Test Modules
Four comprehensive test modules are commented out in `src/transpile/tests/mod.rs`:

| Module | Coverage |
|--------|----------|
| `spec_control_flow` | `if`/`else`, `switch`, `for`, `while`, `do-while`, `try`/`catch`, `break`/`continue`, ternary |
| `spec_data_structures` | Arrays, objects, destructuring, pattern coverage |
| `spec_vars_functions` | Variables, arrow functions, async functions, function params, bindings |
| `spec_jsx` | JSX elements, attributes, children, fragments, inline styles, event handlers |

Without these, there is **zero automated coverage** for control flow, data structures, variables/functions, and JSX in the compile path.

### Stale HIR Test Failures
`cargo test --bin runts` has **113 failures** in enabled modules. Root causes:
- Tests expect old HIR shapes from before `crates/runts-hir` refactor.
- `quote_codegen` panics on `Expr::Invalid` for `do-while`, `throw`, labeled statements, `Math.PI`, `Date.now()`.
- Parser converter intentionally skips features (JSX, optional chaining, class expressions) but tests expect them to work.

These are **compile-path only** — the dev path bypasses HIR entirely.

### Missing Ink Example Coverage
Features implemented in `js_bridge.rs` but **not exercised by any example**:

| Feature | Where Used |
|---------|-----------|
| `useAnimation` | No example |
| `usePaste` | No example |
| `measureElement` / `useBoxMetrics` | No example |
| `useRef` | No example |
| `minWidth` / `minHeight` | No example |
| `maxWidth` / `maxHeight` | No example |
| `zIndex` | No example |
| `flexBasis` | No example |
| `flexShrink` | No example |
| `alignSelf` | No example |
| `alignContent` | No example |
| `columnGap` / `rowGap` | No example |

---

## Parity Harness Specification

The single script (`scripts/parity.sh`) MUST:

1. **Run each example in all 3 environments** (or subset via `--env`).
2. **Normalize output** before comparison:
   - Strip ANSI escape codes
   - Normalize trailing whitespace
   - Normalize line endings to `\n`
3. **Compare symbol-by-symbol**, not line-by-line. A "symbol" is a whitespace-separated token or a box-drawing character.
4. **Report per-example:**
   ```json
   {
     "example": "ink-counter",
     "deno": { "status": "ok", "similarity": 100.0 },
     "rq": { "status": "ok", "similarity": 98.5 },
     "compile": { "status": "ok", "similarity": 98.5 }
   }
   ```
5. **Handle interactive examples:** Detect `useInput`, `useFocus`, `useStdin` in source. For these, capture only the **initial static frame** (pipe `echo "q"` or timeout 2s).
6. **Exit 0** if all similarities ≥ 95%, else exit 1.

---

## DO NOT (Anti-patterns)

| Trap | Why |
|------|-----|
| **Do not restore or expand the HIR interpreter.** | It was 3,087 lines of a broken custom JS engine. rquickjs gives 100% JS semantics for ~1MB. |
| **Do not keep Taffy as a fallback.** | Yoga is the same engine Ink uses. Two layout engines = 2× bug surface. Delete Taffy. |
| **Do not add new shell scripts.** | Multiple scripts already exist. ONE script. Parameterize it. |
| **Do not write hook polyfills in Rust.** | `useState`, `useEffect`, etc. run inside rquickjs. The bridge only exposes Rust primitives (VNode builders, event sources). |
| **Do not exceed linter limits.** | 500 lines/file, 40 lines/fn, 10 complexity. No exceptions. Extract, don't negotiate. |
| **Do not commit without `cargo build` passing.** | The build may be broken at any time. Fix first, then iterate. |
| **Do not batch multiple tasks in one commit.** | Small commits are reversible commits. One task = one commit = one push. |
| **Do not leave test modules commented out.** | Disabled tests are invisible decay. Fix or delete, but don't hide them. |
| **Do not add examples that require Rust code.** | Examples are pure TS/TSX only. Any Rust goes in crates/, not `examples/`. |

---

## Quick Debug Flow

```bash
# 1. Check build
cargo build

# 2. Test one example against deno
deno run -A examples/ink-text-props/main.tsx > /tmp/deno.txt
runts dev --once --plugin ratatui examples/ink-text-props > /tmp/rq.txt
diff /tmp/deno.txt /tmp/rq.txt

# 3. Run parity harness
./scripts/parity.sh --env rq --examples ink-text-props --verbose

# 4. Check test coverage gaps
cargo test --bin runts 2>&1 | grep "FAILED" | wc -l
grep "^//" src/transpile/tests/mod.rs

# 5. Check linter
# (build.rs runs automatically during cargo build)
```

---

## Success Criteria (Final Checklist)

- [ ] `cargo build` passes with 0 errors, 0 warnings.
- [ ] `scripts/parity.sh --env all` passes 89/89 examples (≥95% similarity).
- [ ] `cargo test --test rq_parity` passes ≥90% of examples.
- [ ] `cargo test --bin runts` exits 0 (or only expected ignored failures).
- [ ] Zero commented-out test modules in `src/transpile/tests/mod.rs`.
- [ ] No file > 500 lines, no fn > 40 lines, no complexity > 10.
- [ ] No references to HIR interpreter, Taffy, or `render_tsx` in codebase.
- [ ] All Ink hooks and layout props exercised by at least one example.
- [ ] Docs accurately describe rquickjs + Yoga architecture.
- [ ] All tasks in `tasks/index.json` marked completed.
