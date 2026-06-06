# Task 023: Consolidate 20+ Shell Scripts into ONE Canonical Parity Harness

**Priority:** P1-High  
**Phase:** 1 — Structural Integrity  
**ETA:** 2–3 hours  
**Depends on:** 020

## The Problem

The repo contains a **graveyard of parity scripts**:

```
test_parity.sh
test_parity_complete.sh
test_parity_harness.sh
test_parity_unified.sh
test_parity_v6.sh
test_parity_v7.sh
test_ink_parity.sh
test_ink_parity_comprehensive.sh
test_ink_parity_final.sh
test_ink_parity_unified.sh
test_parity_complete.sh
run_parity_tests.sh
run_parity_tests_comprehensive.sh
```

EXECUTE.md states: *"Parity test must be run by a single script, harnessing all the executing, tracing TUI/CLI apps output to files, and providing per symbol diff results."*

We are violating the spec.

## Why This Matters

- Nobody knows which script is canonical.
- Copy-paste drift means fixes in `v7` don't exist in `complete`.
- CI cannot run 20 scripts.
- New contributors are paralyzed.

## Steps

### Step 1: Choose the survivor

Keep **one** script: `scripts/parity.sh`

Delete all others. If you need history, it's in git.

### Step 2: Design the CLI

```bash
./scripts/parity.sh [OPTIONS]

OPTIONS:
  --env ENV         One of: deno, hir, compile, all (default: all)
  --examples GLOB   Space-separated list, or "all" (default: all)
  --no-compile      Skip compile step (faster iteration)
  --verbose         Show full output on failure
  --keep            Keep /tmp/runts_parity_* dirs for inspection
  --parallel N      Number of parallel jobs (default: 4)
  --diff-tool       Use 'diff', 'delta', or 'code --diff' (default: diff)
  --threshold N     Minimum similarity % to pass (default: 95)

EXAMPLES:
  ./scripts/parity.sh --env hir --examples ink-counter ink-todo
  ./scripts/parity.sh --env all --no-compile
  ./scripts/parity.sh --env deno --examples ink-*
```

### Step 3: Extract shared library

Create `scripts/lib/parity_lib.sh` containing:

```bash
# Normalization
normalize_output() { /* strip ANSI, trim whitespace, normalize newlines */ }
calc_similarity() { /* character-level similarity % */ }
extract_symbols() { /* break output into semantic symbols for diff */ }

# Environment runners
run_deno() { /* deno run -A main.tsx */ }
run_hir() { /* runts hir-render tui/app.tsx */ }
run_compile() { /* runts build --release + run binary */ }

# Reporting
print_summary() { /* ASCII table of results */ }
generate_diff() { /* per-symbol diff */ }
```

### Step 4: Implement per-symbol diff

EXECUTE.md demands *"per symbol diff results"*.

For each example, after normalization:
1. Split output into symbols (words, box-drawing characters, whitespace blocks).
2. Run `diff` on the symbol stream, not raw text.
3. Report: `"ink-counter: deno vs HIR — 42/42 symbols match (100%)"`.

### Step 5: Handle interactive examples

Interactive examples (`useInput`, `useFocus`, etc.) cannot run automatically in deno (they wait for stdin).

Policy:
- Detect interactive examples by grepping for `useInput`, `useFocus`, `useStdin`.
- For interactive examples: compare **initial static render only**.
- Pipe `echo "q"` or use `timeout 2s` to capture first frame.
- Document in the report: `"ink-counter: interactive — comparing initial frame only"`.

### Step 6: Output format

Generate a machine-readable summary:

```json
{
  "timestamp": "2026-06-06T12:00:00Z",
  "results": [
    {
      "example": "ink-counter",
      "deno": { "status": "ok", "similarity": 100.0, "path": "/tmp/.../deno_ink-counter.txt" },
      "hir": { "status": "ok", "similarity": 100.0, "path": "/tmp/.../hir_ink-counter.txt" },
      "compile": { "status": "ok", "similarity": 98.5, "path": "/tmp/.../compile_ink-counter.txt" }
    }
  ]
}
```

## Acceptance Criteria

- [ ] Exactly one script exists: `scripts/parity.sh`.
- [ ] All other parity scripts are deleted.
- [ ] `scripts/parity.sh --env all` runs against all 89 examples.
- [ ] Per-symbol diff is generated for every mismatch.
- [ ] JSON summary is written to stdout and a file.
- [ ] Exit code 0 if all similarities ≥ threshold, else 1.

## Notes

- Do not try to preserve features from all 20 scripts. Pick the best ideas, write one clean script.
- The script should be POSIX-compatible where possible, but bash 4+ is acceptable.
- Use `set -euo pipefail`.
