# Task 152: `ink-use-insertion-effect` Example — `useInsertionEffect`

**Priority:** P1-High
**Phase:** 14 — React 18 Hook Coverage
**Depends on:** 080

## Problem

`useInsertionEffect` (React 18) is a hook for injecting styles before DOM mutations. It's the last major React 18 hook not yet covered by any Ink example.

## Ink Example

```tsx
// examples/ink-use-insertion-effect/tui/app.tsx
import React, { useInsertionEffect, useState } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [ready, setReady] = useState(false);

  useInsertionEffect(() => {
    setReady(true);
  }, []);

  return (
    <Box flexDirection="column">
      <Text>Insertion effect: {ready ? 'ready' : 'pending'}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [x] Example exists at `examples/ink-use-insertion-effect/`
- [x] Uses `useInsertionEffect` hook
- [x] Renders identically in deno and `runts dev` (100% output match)
- [x] Compile path generates compilable Rust
- [x] Parity harness passes with 100% match in all 3 environments

## Implementation Notes

Created comprehensive example demonstrating:
- `useInsertionEffect` hook from React 18
- Effect ordering with useInsertionEffect, useLayoutEffect, and useEffect
- Tracking insertion effect run count with useRef
- Documented behavior in TUI context (no DOM, behaves like useLayoutEffect)

Added `useInsertionEffect` to React shim (`src/transpile/js_bundle/react_shim.rs`) - in TUI context it runs like `useLayoutEffect`.

Added `test_ink_use_insertion_effect` test to `src/transpile/tests/rq_parity/mod.rs`.
