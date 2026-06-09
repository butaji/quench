# Task 051: Parity: Side-by-Side Runner

## Goal
Build harness that runs each example in both deno (Ink) and TuiBridge, capturing ANSI output.

## Acceptance Criteria
- [ ] `scripts/parity.sh` or Rust test binary runs example in **Deno** with the ink shim Deno backend.
- [ ] Same example runs in **TuiBridge** with identical terminal dimensions.
- [ ] Both capture final frame ANSI output to file.
- [ ] Terminal size is fixed (e.g., 80×24) via env var or pty so both renderers see identical dimensions.
- [ ] Supports deterministic mode (no real-time timers, all state pre-computed or mocked).
- [ ] Runs all 10+ examples in CI.

## Dependencies
- All example tasks (041–050)

## SPEC Reference
§8 Performance Summary — parity verification
