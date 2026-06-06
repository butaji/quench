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

**Acceptance:**
- Compile path produces a binary that exits 0 and prints expected text.
- Parity harness runs all 89 examples and produces a JSON report.
- Unit tests cover ≥90% of examples.

---

### Phase 3: Cleanup
**Goal:** Repo is clean. Docs are truthful.

- Delete dead code: `crates/runts-react/`, old scripts, unused imports. Ensure `cargo build` has zero dead-code warnings.
- Update all docs (`DESIGN.md`, `README.md`, `docs/*.md`). Remove all HIR interpreter and Taffy references. Describe rquickjs + Yoga accurately.
- Optional: evaluate Boa vs rquickjs and document decision.

**Acceptance:** Workspace builds clean. Docs do not mention HIR interpreter or Taffy.

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

# 4. Check linter
# (build.rs runs automatically during cargo build)
```

---

## Success Criteria (Final Checklist)

- [ ] `cargo build` passes with 0 errors, 0 warnings.
- [ ] `scripts/parity.sh --env all` passes 89/89 examples (≥95% similarity).
- [ ] `cargo test --test rq_parity` passes ≥90% of examples.
- [ ] No file > 500 lines, no fn > 40 lines, no complexity > 10.
- [ ] No references to HIR interpreter, Taffy, or `render_tsx` in codebase.
- [ ] Docs accurately describe rquickjs + Yoga architecture.
- [ ] All tasks in `tasks/index.json` marked completed.
