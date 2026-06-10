# Task 051: Parity: Side-by-Side Runner

## Goal
Build harness that runs each example in both deno (Ink) and TuiBridge, capturing ANSI output.

## Acceptance Criteria
- [ ] `scripts/parity.sh` runs each example in **Deno** with `npm:ink` (reference ANSI output).
- [ ] Same example runs in **TuiBridge** with our rquickjs shim (actual ANSI output).
- [ ] Both use identical terminal dimensions (e.g., 80×24 via env var or pty).
- [ ] Both capture final frame ANSI output to file.
- [ ] Supports deterministic mode (mocked timers, pre-computed state) for stable snapshots.
- [ ] Runs all 10+ examples in CI.

## Dependencies
- All example tasks (041–050)

## SPEC Reference
§10 Examples Matrix
