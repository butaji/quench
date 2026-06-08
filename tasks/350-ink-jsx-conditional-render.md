# Task 350: `ink-jsx-conditional-render` Example — Ternary / Logical AND Returning JSX

**Priority:** P1-High
**Phase:** 27 — JSX Expression Patterns
**Depends on:** 349

## Problem

Conditional expressions returning JSX (`condition && <Text>...</Text>`, `condition ? <A/> : <B/>`) are fundamental React patterns. No dedicated example exercises both ternary and logical AND in JSX children.

## Ink Example

```tsx
// examples/ink-jsx-conditional-render/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const show = true;
  const hide = false;

  return (
    <Box flexDirection="column">
      {show && <Text>Shown via &&</Text>}
      {hide && <Text>Hidden</Text>}
      {show ? <Text>Ternary true</Text> : <Text>Ternary false</Text>}
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-jsx-conditional-render/`
- [ ] Uses logical AND (`&&`) in JSX children
- [ ] Uses ternary (`?:`) in JSX children
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
