# Task 335: `ink-react-lazy-fallback` Example — `React.lazy` with `Suspense` Fallback

**Priority:** P2-Medium
**Phase:** 27 — React Patterns
**Depends on:** 334

## Problem

`React.lazy` with `Suspense` supports a `fallback` prop shown while the lazy component loads. Task 090 covers Suspense/lazy but not an explicit fallback example.

## Ink Example

```tsx
// examples/ink-react-lazy-fallback/tui/app.tsx
import React, { Suspense, lazy } from 'react';
import { Box, Text } from 'ink';

const LazyText = lazy(() => Promise.resolve({
  default: () => <Text>Loaded!</Text>,
}));

export default function App() {
  return (
    <Box flexDirection="column">
      <Suspense fallback={<Text>Loading...</Text>}>
        <LazyText />
      </Suspense>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-react-lazy-fallback/`
- [ ] Uses `Suspense` with `fallback` prop
- [ ] Uses `React.lazy`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
