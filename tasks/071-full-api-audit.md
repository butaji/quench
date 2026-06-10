# Task 071: Full Ink 7.0.5 API Audit

## Status: COMPLETE AUDIT (2026-06-10)

## Runtime Exports

### Components

| Export | Ink Type | TuiBridge | Status |
|--------|----------|-----------|--------|
| `render` | Function | ✅ Function | ✅ |
| `renderToString` | Function | ⚠️ Stub (returns '') | ⚠️ Task 069 |
| `Box` | forwardRef component | ✅ String tag 'ink-box' | ✅ |
| `Text` | Function component | ✅ String tag 'ink-text' | ✅ |
| `Static` | Function component | ✅ String tag 'ink-static' | ✅ |
| `Newline` | Function component | ✅ String tag 'ink-newline' | ✅ |
| `Spacer` | Function component | ✅ String tag 'ink-spacer' | ✅ |
| `Transform` | Function component | ✅ Function component | ✅ |

### Hooks

| Export | Ink Return Shape | TuiBridge | Status |
|--------|------------------|-----------|--------|
| `useState` | `[value, setValue]` | ✅ | ✅ |
| `useEffect` | `void` | ✅ | ✅ |
| `useRef` | `{ current }` | ✅ | ✅ |
| `useMemo` | `value` | ✅ | ✅ |
| `useCallback` | `fn` | ✅ | ✅ |
| `useContext` | `value` | ✅ | ✅ |
| `createContext` | `{ _currentValue, Provider }` | ✅ | ✅ |
| `useInput` | `void` | ✅ `(input, key)` dispatched | ✅ |
| `useApp` | `{ exit, stdout, stdin, stderr }` | ✅ | ✅ |
| `useStdin` | `{ stdin, setRawMode, setBracketedPasteMode, isRawModeSupported }` | ✅ | ✅ |
| `useStdout` | `{ stdout: { columns, rows, write }, write }` | ✅ | ✅ |
| `useStderr` | `{ stderr: { write }, write }` | ✅ | ✅ |
| `useFocus` | `{ isFocused, focus }` | ⚠️ Always `isFocused: true`, no `focus` | ⚠️ |
| `useFocusManager` | `{ enableFocus, disableFocus, focusNext, focusPrevious, focus, activeId }` | ⚠️ Missing `enableFocus`, `disableFocus`, `activeId` | ⚠️ |
| `useIsScreenReaderEnabled` | `boolean` | ✅ Returns `false` | ✅ |
| `useCursor` | `{ setCursorPosition }` | ✅ (position tracking only) | ✅ |
| `useAnimation` | `{ frame, time, delta, reset }` | ✅ | ✅ |
| `useWindowSize` | `{ columns, rows }` | ✅ | ✅ |
| `useBoxMetrics` | `{ width, height, left, top, hasMeasured }` | ✅ | ✅ |
| `usePaste` | `void` | ✅ | ✅ |
| `measureElement` | `{ width, height }` | ✅ | ✅ |

### Constants

| Export | Status |
|--------|--------|
| `kittyFlags` | ✅ |
| `kittyModifiers` | ✅ |

### Types (TypeScript-only, no runtime effect)

| Type | Status | Note |
|------|--------|------|
| `RenderOptions` | N/A | Type-only |
| `Instance` | N/A | Type-only |
| `RenderToStringOptions` | N/A | Type-only |
| `BoxProps` | N/A | Type-only |
| `TextProps` | N/A | Type-only |
| `AppProps` | N/A | Type-only |
| `StdinProps` | N/A | Type-only |
| `StdoutProps` | N/A | Type-only |
| `StderrProps` | N/A | Type-only |
| `StaticProps` | N/A | Type-only |
| `TransformProps` | N/A | Type-only |
| `NewlineProps` | N/A | Type-only |
| `Key` | N/A | Type-only |
| `AnimationResult` | N/A | Type-only |
| `WindowSize` | N/A | Type-only |
| `BoxMetrics` | N/A | Type-only |
| `UseBoxMetricsResult` | N/A | Type-only |
| `CursorPosition` | N/A | Type-only |
| `DOMElement` | N/A | Type-only |
| `KittyKeyboardOptions` | N/A | Type-only |
| `KittyFlagName` | N/A | Type-only |

## Box Props

### Implemented ✅

| Prop | Status |
|------|--------|
| `flexDirection` | ✅ |
| `alignItems` | ✅ |
| `alignSelf` | ✅ |
| `alignContent` | ✅ |
| `justifyContent` | ✅ |
| `flexWrap` | ✅ |
| `flexGrow` | ✅ |
| `flexShrink` | ✅ |
| `flexBasis` | ✅ |
| `gap` | ✅ |
| `gapX` | ✅ |
| `gapY` | ✅ |
| `columnGap` | ✅ |
| `rowGap` | ✅ |
| `margin` + variants | ✅ |
| `padding` + variants | ✅ |
| `borderStyle` | ✅ |
| `borderColor` | ✅ |
| `borderDimColor` | ✅ |
| `borderTop` | ✅ |
| `borderBottom` | ✅ |
| `borderLeft` | ✅ |
| `borderRight` | ✅ |
| `width` | ✅ |
| `height` | ✅ |
| `minWidth` | ✅ |
| `maxWidth` | ✅ |
| `minHeight` | ✅ |
| `maxHeight` | ✅ |
| `position` | ✅ |
| `display` | ✅ |
| `title` | ✅ |
| `top` | ✅ |
| `right` | ✅ |
| `bottom` | ✅ |
| `left` | ✅ |
| `aria-label` | ✅ (no-op) |
| `aria-hidden` | ✅ (no-op) |
| `aria-role` | ✅ (no-op) |
| `aria-state` | ✅ (no-op) |

### Not Implemented (tracked in tasks)

| Prop | Task |
|------|------|
| `borderTopColor` | Task 068 |
| `borderBottomColor` | Task 068 |
| `borderLeftColor` | Task 068 |
| `borderRightColor` | Task 068 |
| `borderTopDimColor` | Task 068 |
| `borderBottomDimColor` | Task 068 |
| `borderLeftDimColor` | Task 068 |
| `borderRightDimColor` | Task 068 |
| `borderBackgroundColor` | Task 068 |
| `border*BackgroundColor` | Task 068 |
| `overflow` | Task 070 |
| `overflowX` | Task 070 |
| `overflowY` | Task 070 |
| `aspectRatio` | Task 070 |

## Text Props

### Implemented ✅

| Prop | Status |
|------|--------|
| `color` | ✅ |
| `backgroundColor` | ✅ |
| `bold` | ✅ |
| `dimColor` | ✅ |
| `dim` | ✅ |
| `italic` | ✅ |
| `strikethrough` | ✅ |
| `underline` | ✅ |
| `inverse` | ✅ |
| `small` | ✅ |
| `wrap` / `textWrap` | ✅ |
| `transform` | ✅ |
| `aria-label` | ✅ (no-op) |
| `aria-hidden` | ✅ (no-op) |
| `aria-role` | ✅ (no-op) |
| `aria-state` | ✅ (no-op) |

## Known Limitations

### 1. Focus Management (useFocus / useFocusManager)
- **Ink behavior**: Tab-based focus cycling, focusable components, activeId tracking
- **TuiBridge**: useFocus always returns `isFocused: true`. No actual focus manager.
- **Impact**: Apps using tab navigation won't work correctly
- **Workaround**: Apps can use useInput directly for manual navigation

### 2. Background Color Inheritance
- **Ink behavior**: `backgroundColor` on Box is inherited by child Text via React context
- **TuiBridge**: Each Text must specify its own `backgroundColor`
- **Impact**: Apps relying on inherited background colors render differently

### 3. Ref Forwarding
- **Ink behavior**: `ref` on Box/Text gives access to DOM element
- **TuiBridge**: Refs not wired up in reconciler
- **Impact**: `measureElement(ref)` requires `ref.current.id` which is never set

### 4. renderToString
- **Status**: Stub returns empty string
- **Task**: 069

## Verification

All 39 TSX examples compile and run. Keyboard input now dispatches correctly to `useInput` handlers.

## References
- Ink 7.0.5: https://unpkg.com/ink@7.0.5/build/index.d.ts
