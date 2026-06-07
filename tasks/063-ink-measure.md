# Task 063: `ink-measure` Example — `measureElement`, `useBoxMetrics`

**Priority:** P2-Medium  
**Phase:** 6 — Ink Advanced  
**Depends on:** 062

## Problem

`measureElement` and `useBoxMetrics` have zero example coverage.

## Example

```tsx
import { Box, Text, useRef, measureElement } from 'ink';
import { useEffect, useState } from 'react';

export default function App() {
  const ref = useRef<any>(null);
  const [dims, setDims] = useState({ width: 0, height: 0 });

  useEffect(() => {
    if (ref.current) {
      const d = measureElement(ref.current);
      setDims(d);
    }
  }, []);

  return (
    <Box ref={ref} width={10} height={3} borderStyle="round">
      <Text>W:{dims.width} H:{dims.height}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] `measureElement` exercised
- [ ] `runts build --release` produces working binary with 100% output match
