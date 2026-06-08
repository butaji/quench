# Task 088: `ink-bigint-globalthis` Example — BigInt, Numeric Separators, `globalThis`

**Priority:** P2-Medium
**Phase:** 10 — Extended TS/TSX Coverage
**Depends on:** 078

## Problem

ES2020+ features BigInt (arbitrary precision integers), numeric separators (`1_000_000`), and `globalThis` (universal global object) are not exercised by any existing Ink example.

## Solution

Created example at `examples/ink-bigint-globalthis/` with:
- BigInt literal (`9007199254740993n`)
- Numeric separators (`1_000_000_000`)
- `globalThis` platform detection
- Platform detection (`Node.js`, `Deno`, `Browser`)

## Output

```
BigInt: 9007199254740993
Numeric separator: 1000000000
Platform: Node.js (deno) / Browser (runts dev - rquickjs has no process)
Has globalThis: yes
```

## Acceptance Criteria

- [x] Example exists at `examples/ink-bigint-globalthis/`
- [x] Uses BigInt literal (`123n`)
- [x] Uses numeric separators (`1_000_000`)
- [x] Uses `globalThis`
- [x] Renders in deno and `runts dev`
- [x] Parity harness passes with 100% match in rq environment

## Notes

- The dev path shows "Browser" for platform because rquickjs doesn't expose `globalThis.process`
- BigInt is converted to string via `String()` in the render to avoid display issues
- Compile path uses Rust `i128` for BigInt representation
