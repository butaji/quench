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

## Execution Order (Do Not Skip)

Follow the phases in `tasks/index.json`. Each task blocks the next.

### Phase 0: Unblock (Tasks 020–021)
**Goal:** `cargo build` passes. Linter is enforced.

- **020** — Fix `quote_codegen.rs:609` syntax error. One-line brace fix.
- **021** — Uncomment `build.rs` linter. Mechanically extract functions until 0 violations (file ≤500 lines, fn ≤40 lines, complexity ≤10).

**Acceptance:** `cargo build` exits 0. `cargo test --no-run` exits 0.

**Commit after each task.**

---

### Phase 1: rquickjs + Yoga Engine (Tasks 022–026, 033)
**Goal:** `runts dev --once` renders any example identically to deno.

Execute in this exact order:

1. **022** — `rm src/hir_runtime.rs`. Strip all `Interpreter`, `Value`, `render_tsx` references from `src/main.rs`, `src/cli.rs`. **Do not preserve HIR runtime as a fallback.** It is dead code.
2. **033** — Remove Taffy. Delete `crates/runts-ink/src/flex_layout/taffy.rs`. Make Yoga the default and only feature in `Cargo.toml`. Strip `#[cfg(feature = "taffy")]` conditionals.
3. **023** — Build `transpile_to_js(source: &str) -> String`. Use `oxc_codegen` (already in deps). Must: (a) desugar JSX to `React.createElement`, (b) erase TS types, (c) rewrite `import { Box, Text } from 'ink'` to bridge globals.
4. **024** — Wire `runts dev` to: parse `.tsx` → `transpile_to_js` → create rquickjs context → inject `js_bridge.rs` globals → inject React shim → eval bundle → call `renderToString()` → print.
5. **025** — Complete `js_bridge.rs`. Every prop used in any of the 89 examples must be supported. Use a prop dispatch table, not 300-line match blocks.
6. **026** — Wire interactive hooks (`useInput`, `useApp`, `useFocus`, etc.) through the bridge. Crossterm events → JS callbacks.

**Acceptance per task:**
- 022/033: `cargo build` passes with zero warnings.
- 023: `examples/ink-text-props/tui/app.tsx` transpiles to runnable JS.
- 024: `runts dev --once --plugin ratatui examples/ink-text-props` prints the same text as `deno run -A examples/ink-text-props/main.tsx`.
- 025: `grep -r 'unsupported prop' tests/` returns nothing.
- 026: `runts dev --once examples/ink-counter` renders and responds to `q` / arrow keys.

**Commit after each task.**

---

### Phase 2: Compile + Verification (Tasks 027–029)
**Goal:** `runts build --release` produces working binaries. One parity harness runs all 89 examples.

- **027** — Replace plugin JSON string boundary with typed HIR (`&hir::Module`). Fix `find_runts_lib_path` to use `env!("CARGO_MANIFEST_DIR")`. Verify `examples/ink-text-props` builds and runs.
- **028** — Delete all 20+ `test_parity*.sh` scripts. Create ONE `scripts/parity.sh` with `--env deno|rq|compile|all`, `--examples GLOB`, `--once`. Implement per-symbol diff (not just line diff). Output JSON summary.
- **029** — Generate one Rust test per example in `tests/rq_parity/`. Each test reads `examples/*/tui/app.tsx`, runs it through the rquickjs path, asserts expected substrings in output.

**Acceptance:**
- 027: `./target/release/runts-app` exits 0 and prints expected text.
- 028: `./scripts/parity.sh --env all` runs 89 examples and produces a JSON report.
- 029: `cargo test --test rq_parity` passes ≥90% of examples.

**Commit after each task.**

---

### Phase 3: Cleanup + Future (Tasks 030–032)
**Goal:** Repo is clean. Docs are truthful.

- **030** — Delete `crates/runts-react/`, old scripts, unused imports. Ensure `cargo build` has zero dead-code warnings.
- **031** — Update `DESIGN.md`, `README.md`, `docs/INK-ARCHITECTURE.md`, `docs/PHILOSOPHY.md`, `docs/PERFORMANCE.md`. Remove all HIR interpreter and Taffy references.
- **032** — Optional spike: evaluate Boa vs rquickjs. Document decision in `docs/ROADMAP.md`.

**Acceptance:** Workspace builds clean. Docs do not mention HIR interpreter or Taffy.

**Commit after each task. Final push.**

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
| **Do not add new shell scripts.** | 20 scripts already exist. ONE script. Parameterize it. |
| **Do not write hook polyfills in Rust.** | `useState`, `useEffect`, etc. run inside rquickjs. The bridge only exposes Rust primitives (VNode builders, event sources). |
| **Do not exceed linter limits.** | 500 lines/file, 40 lines/fn, 10 complexity. No exceptions. Extract, don't negotiate. |
| **Do not commit without `cargo build` passing.** | The build is currently broken (020). Fix first, then iterate. |

---

## Task Tracking

Every change MUST be tracked:

1. Pick the next pending task from `tasks/index.json`.
2. Read `tasks/XXX-title.md` for acceptance criteria.
3. Implement. Run `cargo build`. Run `cargo test`.
4. Update `tasks/index.json`: set `"status": "completed"`.
5. `git add -A && git commit -m "XXX: brief description"`
6. `git push origin fresh`

**Never batch 3 tasks into one commit.** Small commits are reversible commits.

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
- [ ] All tasks in `tasks/index.json` marked `completed`.
