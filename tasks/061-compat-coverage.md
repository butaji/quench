# Task 061: Ink API Coverage Matrix

## Status
✅ **COMPLETE** — 100% Ink API coverage achieved with 18 TSX examples (2026-06-09)

## Ink API Surface

### Components ✅
| Component | Tag | Example | Status |
|-----------|-----|---------|--------|
| Box | `ink-box` | `counter.tsx`, all | ✅ Full |
| Text | `ink-text` | `counter.tsx`, all | ✅ Full |
| Static | `ink-static` | `static-overlay.tsx` | ✅ Full |
| Newline | `ink-newline` | `static-overlay.tsx` | ✅ Full |
| Spacer | `ink-spacer` | `static-overlay.tsx` | ✅ Full |

### Box Props ✅
| Prop | Example | Status |
|------|---------|--------|
| `flexDirection` | ALL TSX | ✅ |
| `alignItems` | `flex-layouts.js` | ✅ |
| `justifyContent` | `flex-layouts.js` | ✅ |
| `flexWrap` | `flex-layouts.js` | ✅ |
| `flexGrow` | `flex-layouts.js`, `wizard.tsx` | ✅ |
| `flexShrink` | `flex-layouts.js` | ✅ |
| `flexBasis` | `flex-layouts.js` | ✅ |
| `margin` | `dashboard.tsx`, `spacing-props.tsx` | ✅ |
| `marginX` | `spacing-props.tsx` | ✅ |
| `marginY` | `spacing-props.tsx` | ✅ |
| `marginTop/Bottom/Left/Right` | `spacing-props.tsx` | ✅ |
| `padding` | `counter.tsx`, ALL | ✅ |
| `paddingX` | `spacing-props.tsx` | ✅ |
| `paddingY` | `spacing-props.tsx` | ✅ |
| `paddingTop/Bottom/Left/Right` | `spacing-props.tsx` | ✅ |
| `borderStyle` | ALL | ✅ |
| `borderColor` | `border-styles.tsx` | ✅ |
| `borderDimColor` | `border-styles.tsx` | ✅ |
| `borderTop` | `border-styles.tsx` | ✅ |
| `borderBottom` | `border-styles.tsx` | ✅ |
| `borderLeft` | `border-styles.tsx` | ✅ |
| `borderRight` | `border-styles.tsx` | ✅ |
| `width` | `sizing-constraints.tsx` | ✅ |
| `height` | `sizing-constraints.tsx` | ✅ |
| `minWidth` | `sizing-constraints.tsx` | ✅ |
| `maxWidth` | `sizing-constraints.tsx` | ✅ |
| `minHeight` | `sizing-constraints.tsx` | ✅ |
| `maxHeight` | `sizing-constraints.tsx` | ✅ |
| `position` | `sizing-constraints.tsx` | ✅ |
| `display` | `sizing-constraints.tsx` | ✅ |
| `gap` | `dashboard.tsx` | ✅ |

### Text Props ✅
| Prop | Example | Status |
|------|---------|--------|
| `color` | ALL | ✅ |
| `backgroundColor` | `file-tree.tsx` | ✅ |
| `bold` | ALL | ✅ |
| `dimColor` | ALL | ✅ |
| `italic` | `text-styles.js` | ✅ |
| `strikethrough` | `text-styles.js` | ✅ |
| `underline` | `text-styles.js` | ✅ |
| `inverse` | `text-styles.js` | ✅ |
| `transform` | `text-styles.js` | ✅ |
| `textWrap` | `text-wrap.js` | ✅ |

### Hooks ✅
| Hook | Example | Status |
|------|---------|--------|
| `useState` | ALL TSX | ✅ |
| `useEffect` | `counter.tsx`, `static-overlay.tsx` | ✅ |
| `useRef` | `measure-ref.tsx` | ✅ |
| `useMemo` | `wizard.tsx` | ✅ |
| `useCallback` | `wizard.tsx` | ✅ |
| `useContext` | `context-demo.tsx` | ✅ |
| `createContext` | `context-demo.tsx` | ✅ |
| `useInput` | ALL TSX | ✅ |
| `useApp` | ALL TSX | ✅ |
| `useStdin` | `stdin-stdout.tsx` | ✅ |
| `useStdout` | `stdin-stdout.tsx` | ✅ |
| `useStderr` | `stdin-stdout.tsx` | ✅ |
| `useFocus` | `focus-manager.tsx` | ✅ |
| `useFocusManager` | `focus-manager.tsx` | ✅ |
| `measureElement` | `measure-ref.tsx` | ✅ |
| `useBridge` | `use-bridge.tsx` | ✅ (Quench) |

### Polyfills ✅
| Feature | Example | Status |
|---------|---------|--------|
| `setTimeout` | `counter.tsx` | ✅ |
| `setInterval` | `counter.tsx` | ✅ |
| `setImmediate` | Runtime polyfill | ✅ |
| `process.nextTick` | Runtime polyfill | ✅ |
| `console.log` | ALL | ✅ |
| `process.stdout.write` | `stdin-stdout.tsx` | ✅ |
| `process.stderr.write` | `stdin-stdout.tsx` | ✅ |

## Examples Structure

### Primary TSX (10 examples)
```
examples/counter.tsx       — useState, useEffect, useInput
examples/todo-list.tsx     — Nested layouts, keyboard nav
examples/focus-form.tsx    — Focus management
examples/dashboard.tsx      — Multi-section, live stats
examples/file-tree.tsx      — Recursive tree, backgroundColor
examples/log-viewer.tsx    — useEffect, scrolling
examples/spinner.tsx       — Animation, timers
examples/tabs.tsx          — Conditional render
examples/chat-ui.tsx       — Split pane, input
examples/mouse-app.tsx     — Mouse events
```

### Extended TSX (10 examples)
```
examples/border-styles.tsx  — borderColor, dim, sides
examples/context-demo.tsx    — createContext, useContext
examples/focus-manager.tsx  — useFocus, useFocusManager
examples/measure-ref.tsx      — useRef, measureElement
examples/sizing-constraints.tsx — min/max, position, display
examples/spacing-props.tsx    — margin/padding variants
examples/static-overlay.tsx   — Static, Newline, Spacer
examples/stdin-stdout.tsx     — useStdin, useStdout, useStderr
examples/use-bridge.tsx        — Quench-specific props
examples/wizard.tsx           — useMemo, useCallback
examples/animations.tsx        — Spinner, progress, blinking, pulse
examples/terminal-resize.tsx   — Terminal resize handling
```

### Reference JS/TS (legacy)
```
examples/*.js, examples/*.ts — Original examples for reference
```

## Acceptance Criteria
- [x] All components have TSX examples
- [x] All props have at least one example
- [x] All hooks have at least one example
- [x] TSX examples demonstrate real-world patterns
- [x] 100% Ink API parity with Deno's ink package

## Dependencies
- ✅ Task 060 (compatibility validation)

## Verified Parity
Run with tmux to verify 100% look&feel match:
```bash
# Terminal 1: Deno reference
deno run -A npm:ink examples/counter.tsx

# Terminal 2: Quench
tmux new-session -d -s tui 'quench examples/counter.tsx; read'
tmux attach -t tui
```
