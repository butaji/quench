# Task 028: Create Single Canonical Parity Harness (deno vs rquickjs vs compile)

**Priority:** P1-High  
**Phase:** 2 — Compile + Verification  
**ETA:** 2–3 hours  
**Depends on:** 024, 027

## The Problem

Multiple old shell scripts existed in repo root. EXECUTE.md demands exactly ONE script at `scripts/parity.sh`.

## Steps

1. Delete ALL old `test_parity*.sh`, `test_ink_parity*.sh` from repo root and `tests/parity/`.
2. Create `scripts/parity.sh` with CLI:
   ```bash
   ./scripts/parity.sh --env deno|rq|compile|all --examples GLOB --once
   ```
3. Extract shared logic to `scripts/lib/parity_lib.sh`.
4. Implement per-symbol diff via `scripts/lib/symbol_diff.py`.
5. Implement JSON summary output.
6. For interactive examples: capture initial frame only, pipe `q\n` for auto-exit.

## Acceptance Criteria

- [x] Exactly one script: `scripts/parity.sh`.
- [x] `--env all` runs against all 91 examples.
- [x] Per-symbol diff + JSON summary.
- [x] Exit code 0 if all pass, else 1.
- [x] No old `test_parity*.sh` or `test_ink_parity*.sh` scripts remain.
