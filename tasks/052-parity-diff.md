# Task 052: Parity: ANSI Diff & Match

## Status
✅ **Done**


## Goal
Compare deno and TuiBridge ANSI outputs and enforce 100% look&feel match.

## Acceptance Criteria
- [ ] Diff tool strips escape sequences or compares rendered cell grids.
- [ ] Any mismatch fails CI with visual diff (side-by-side or unified).
- [ ] Acceptable tolerance: zero cell differences (100% match required).
- [ ] Integration test: known-good identical outputs pass; single char diff fails.
- [ ] CI gate: all examples must pass parity before merge.

## Dependencies
- Task 051

## SPEC Reference
§10 Examples Matrix; §6 Performance
