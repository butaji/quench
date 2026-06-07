# Task: Verify js_bridge.rs Completeness Against All Used Props

**Priority:** P1-High  
**Phase:** 3 — Coverage Gaps  
**Status:** ✅ COMPLETED
**Depends on:** 025, 035

## Problem

`js_bridge.rs` appeared comprehensive but had **never been systematically verified** against every prop used across all 91 examples.

## Verification Method

Generated a prop coverage matrix by extracting all props from `examples/*/tui/app.tsx` and confirming each is handled in `crates/runts-ink/src/js_bridge/box_props.rs`:

```bash
grep -rhoE '\b[a-z][a-zA-Z]+=' examples/*/tui/app.tsx | sed 's/=$//' | sort | uniq -c | sort -rn
```

## Results

**100% coverage:** Every prop used in any example is handled in the bridge.

| Category | Status |
|----------|--------|
| Box props (`color`, `flexDirection`, `padding`, `borderStyle`, `width`, `marginTop`, `backgroundColor`, `justifyContent`, `paddingX`, `paddingY`, `gap`, `position`, `overflowX`, `overflowY`, `flexWrap`, `flexGrow`, `display`, `minWidth`, `minHeight`, `maxWidth`, `maxHeight`, `zIndex`, `flexBasis`, `flexShrink`, `alignSelf`, `alignContent`, `columnGap`, `rowGap`) | ✅ All handled |
| Text props (`bold`, `italic`, `underline`, `strikethrough`, `inverse`, `dimColor`, `color`, `backgroundColor`, `wrap`) | ✅ All handled |
| Hooks (`useInput`, `useApp`, `useStdin`, `useStdout`, `useStderr`, `useWindowSize`, `useFocus`, `useFocusManager`, `useCursor`, `useAnimation`, `usePaste`, `useRef`) | ✅ All wired |

## New Hooks Added During Verification

- `useRef` — mutable refs outside React state
- `usePaste` — bracketed paste event handling

## Acceptance Criteria

- [x] Prop coverage matrix shows 100% of example-used props are in bridge.
- [x] Every bridge setter has a corresponding unit test.
- [x] Running all 91 examples through `runts dev --once` produces zero bridge-related errors.
- [x] `useRef` and `usePaste` hooks added to React shim and bridge.
