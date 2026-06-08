# Task 163: `ink-ref-callback` Example — Callback Refs, `useRef` with Initial Value

**Priority:** P1-High
**Phase:** 14 — React Pattern Coverage
**Depends on:** 162

## Problem

Callback refs (`ref={(el) => { ... }}`) and `useRef` with initial values are used in 3+ existing examples but have **no dedicated task**. Refs are essential for DOM measurements and imperative operations.

## Ink Example

```tsx
// examples/ink-ref-callback/tui/app.tsx
import React, { useRef, useCallback, useState } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [height, setHeight] = useState(0);
  const containerRef = useRef<Box>(null);

  const measureRef = useCallback((node: any) => {
    if (node !== null) {
      setHeight(node.height ?? 1);
    }
  }, []);

  return (
    <Box flexDirection="column" ref={containerRef}>
      <Text>Container ref set: {containerRef.current ? 'yes' : 'no'}</Text>
      <Box ref={measureRef}>
        <Text>Measured height: {height}</Text>
      </Box>
    </Box>
  );
}
```

## Acceptance Criteria

- [x] Example exists at `examples/ink-ref-callback/`
- [x] Uses `useRef` with initial value
- [x] Uses callback ref pattern
- [x] Renders identically in deno and `runts dev` (100% output match)
- [x] Compile path generates compilable Rust
- [x] Parity harness passes with 100% match in all 3 environments

## Implementation Notes

Created comprehensive example demonstrating:
- `useRef` with initial value (number, string, boolean)
- Callback ref pattern with `useCallback`
- Multiple refs of different types
- Imperative handle pattern
- Counter using ref for mutable value

Added `test_ink_ref_callback` test to `src/transpile/tests/rq_parity/mod.rs`.
