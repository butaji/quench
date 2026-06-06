# Task: Verify js_bridge.rs Completeness Against All Used Props

**Priority:** P1-High  
**Phase:** 1 — rquickjs + Yoga Engine  
**Depends on:** 025, 035

## Problem

`js_bridge.rs` appears comprehensive but has **never been systematically verified** against every prop used across all 89+ examples. A prop could be silently ignored, mapped to the wrong Yoga style, or crash on an unexpected value type.

## Extract All Props from Examples

```bash
grep -rhoE '\b[a-z][a-zA-Z]+=' examples/*/tui/app.tsx | sed 's/=$//' | sort | uniq -c | sort -rn
```

Top props by usage:
1. `color` (166)
2. `flexDirection` (126)
3. `padding` (121)
4. `borderStyle` (58)
5. `width` (34)
6. `marginTop` (28)
7. `backgroundColor` (19)
8. `justifyContent` (18)
9. `paddingX` / `paddingY` (30 combined)
10. `gap` (8)
11. `position`, `overflowX`, `overflowY`, `flexWrap`, `flexGrow`, `display` (1-2 each)

## Verification Checklist

For each prop, verify in `crates/runts-ink/src/js_bridge/box_props.rs`:

- [ ] Prop name is handled in `apply_box_props`.
- [ ] Setter accepts the value types used in examples (string, number, boolean).
- [ ] Setter maps to the correct `InkBox` field.
- [ ] Serialize function in `serialize_box_props` round-trips correctly.
- [ ] Unit test exists in `crates/runts-ink/tests/js_bridge_props.rs` or inline.

For text props in `text_props.rs`:

- [ ] `bold`, `italic`, `underline`, `strikethrough`, `inverse`, `dimColor`
- [ ] `color`, `backgroundColor`
- [ ] `wrap` (if used)

For hooks in `hooks.rs`:

- [ ] `useInput` — registered, callback receives `(input, key)` shape.
- [ ] `useApp` — returns `{ exit }`.
- [ ] `useStdin` — returns `{ isRawModeSupported, setRawMode }`.
- [ ] `useStdout` / `useStderr` — returns `{ write }`.
- [ ] `useWindowSize` — returns `{ width, height }` from env or crossterm.
- [ ] `useFocus` — returns `{ isFocused, focus }`.
- [ ] `useFocusManager` — returns `{ focusNext, focusPrevious }`.
- [ ] `useCursor` — returns `{ setCursorPosition }`.
- [ ] `useAnimation` — returns `{ start, stop, isRunning }`.

## Steps

1. Generate a prop coverage matrix script:
   ```bash
   #!/bin/bash
   for prop in color flexDirection padding borderStyle width marginTop backgroundColor justifyContent paddingX paddingY gap position overflowX overflowY flexWrap flexGrow display; do
     in_bridge=$(grep -c "\"$prop\"" crates/runts-ink/src/js_bridge/box_props.rs || true)
     in_examples=$(grep -rhc "$prop=" examples/*/tui/app.tsx | awk '{s+=$1} END{print s}')
     echo "$prop: bridge=$in_bridge examples=$in_examples"
   done
   ```

2. For any prop with `bridge=0` and `examples>0`: **CRITICAL BUG** — implement immediately.

3. For any prop with `bridge>0` but no unit test: add a test.

4. Run all 89 examples through `runts dev --once` and capture any bridge errors.

## Acceptance Criteria

- [ ] Prop coverage matrix shows 100% of example-used props are in bridge.
- [ ] Every bridge setter has a corresponding unit test.
- [ ] Running all examples through `runts dev --once` produces zero bridge-related errors.
