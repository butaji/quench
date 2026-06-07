# Task 080: `ink-use-layout-effect` Example — `useLayoutEffect`

**Priority:** P1-High
**Phase:** 10 — Extended TS/TSX Coverage
**Depends on:** 078

## Problem

`useLayoutEffect` is a critical React hook for DOM measurements before paint. It is not exercised by any existing Ink example, yet it's commonly needed when `useEffect` causes visual flicker.

## Ink Example

```tsx
// examples/ink-use-layout-effect/tui/app.tsx
import React, { useLayoutEffect, useRef, useState } from 'react';
import { Box, Text, useStdout } from 'ink';

export default function App() {
  const { stdout } = useStdout();
  const [dims, setDims] = useState({ cols: 80, rows: 24 });
  const measured = useRef(false);

  useLayoutEffect(() => {
    if (!measured.current && stdout) {
      measured.current = true;
      setDims({ cols: stdout.columns, rows: stdout.rows });
    }
  }, [stdout]);

  return (
    <Box flexDirection="column">
      <Text>Terminal: {dims.cols}x{dims.rows}</Text>
      <Text>Measured before first render</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-use-layout-effect/`
- [ ] Uses `useLayoutEffect` hook
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
