# Task: Add Missing Ink Examples for Full Feature Coverage

**Priority:** P1-High  
**Phase:** 3 — Coverage Gaps  
**Status:** ✅ COMPLETED
**Depends on:** 024, 025

## Problem

91 examples exist but several Ink features were **not exercised by any example**, meaning js_bridge.rs supported them but they were never verified in parity tests.

## Examples Added

| Example | Feature Covered | Status |
|---------|----------------|--------|
| `examples/ink-paste/` | `usePaste` hook | ✅ Added |
| `examples/ink-ref/` | `useRef` hook | ✅ Added |
| `examples/ink-flex-shrink/` | `flexShrink` prop | ✅ Added |

## Remaining Gaps

| Feature | Example Needed | Why |
|---------|---------------|-----|
| `useAnimation` | `ink-animation` | Timer-based re-rendering, not covered |
| `measureElement` / `useBoxMetrics` | `ink-measure` | Layout readback for dynamic positioning |

## Acceptance Criteria

- [x] `examples/ink-paste/` created with `tui/app.tsx`, `main.tsx`, `deno.json`.
- [x] `examples/ink-ref/` created with `tui/app.tsx`, `main.tsx`, `deno.json`.
- [x] `examples/ink-flex-shrink/` created with `tui/app.tsx`, `main.tsx`, `deno.json`.
- [x] Each example runs with `deno run -A main.tsx` without error.
- [x] Each example renders something visible and deterministic.
- [x] `runts dev --once --plugin ratatui <example>` produces identical output to deno.
