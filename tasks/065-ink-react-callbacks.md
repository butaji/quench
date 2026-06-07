# Task 065: `ink-react-callbacks` Example — `useMemo`, `useCallback`, `forwardRef`

**Priority:** P2-Medium
**Phase:** 6 — React Advanced
**Depends on:** 064

## Problem

`useMemo`, `useCallback`, and `forwardRef` are not validated by any example.

## Example

```tsx
import React, { useMemo, useCallback, useRef, forwardRef } from 'react';
import { Box, Text } from 'ink';

const FancyText = forwardRef(({ label }: { label: string }, ref: any) => (
  <Text ref={ref}>{label}</Text>
));

export default function App() {
  const ref = useRef<any>(null);
  const [items] = useState([1, 2, 3, 4, 5]);

  const doubled = useMemo(() => items.map(x => x * 2), [items]);
  const handleClick = useCallback(() => { /* no-op */ }, []);

  return (
    <Box flexDirection="column">
      <FancyText ref={ref} label="Hello" />
      <Text>{doubled.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] `useMemo`, `useCallback`, `forwardRef` all work in rquickjs
- [ ] `runts build --release` produces working binary
- [ ] Parity harness 100%