# Task 279: `ink-react-profiler` Example — `React.Profiler`

**Priority:** P2-Medium
**Phase:** 23 — React Patterns
**Depends on:** 278

## Problem

`React.Profiler` measures rendering performance of a React subtree. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-react-profiler/tui/app.tsx
import React, { Profiler } from 'react';
import { Box, Text } from 'ink';

function onRender(id: string, phase: string, actualDuration: number) {
  // eslint-disable-next-line no-console
  console.log(`${id} ${phase}: ${actualDuration}ms`);
}

export default function App() {
  return (
    <Profiler id="App" onRender={onRender}>
      <Box flexDirection="column">
        <Text>Profiler example</Text>
      </Box>
    </Profiler>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-react-profiler/`
- [ ] Uses `React.Profiler` component
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles Profiler JSX element
- [ ] Parity harness passes with 100% match in all 3 environments
