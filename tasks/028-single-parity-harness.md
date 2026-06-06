# Task 028: Create Single Canonical Parity Harness (deno vs rquickjs vs compile)

**Priority:** P1-High  
**Phase:** 2 — Compile + Verification  
**ETA:** 2–3 hours  
**Depends on:** 024, 027

## The Problem

10 old shell scripts still exist in repo root. EXECUTE.md demands ONE script.

## Steps

1. Delete ALL old `test_parity*.sh`, `test_ink_parity*.sh` from repo root.
2. Create `scripts/parity.sh` with CLI:
   ```bash
   ./scripts/parity.sh --env deno|rq|compile|all --examples GLOB --once
   ```
3. Extract shared logic to `scripts/lib/parity_lib.sh`.
4. Implement per-symbol diff and JSON summary.
5. For interactive examples: capture initial frame only, pipe `q\n` for auto-exit.

## Acceptance Criteria

- [ ] Exactly one script: `scripts/parity.sh`.
- [ ] `--env all` runs against all 88 examples.
- [ ] Per-symbol diff + JSON summary.
- [ ] Exit code 0 if all pass, else 1.
