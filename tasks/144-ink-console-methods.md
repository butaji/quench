# Task 144: `ink-console-methods` Example — `console.log`, `error`, `warn`, `info`, `time`, `timeEnd`

**Priority:** P1-High
**Phase:** 12 — Runtime API Completion
**Depends on:** 118

## Problem

`console` methods beyond basic `log` are commonly used for debugging and timing. No existing Ink example exercises `console.error`, `warn`, `info`, `time`, `timeEnd`, `table`.

## Ink Example

```tsx
// examples/ink-console-methods/tui/app.tsx
import React, { useEffect } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  useEffect(() => {
    console.time('render');
    console.log('App mounted');
    console.info('Info message');
    console.warn('Warning message');
    console.error('Error message');
    console.timeEnd('render');
  }, []);

  const data = [
    { name: 'Alice', age: 30 },
    { name: 'Bob', age: 25 },
  ];
  console.table(data);

  return (
    <Box flexDirection="column">
      <Text>Console methods exercised</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-console-methods/`
- [ ] Uses `console.log`, `error`, `warn`, `info`, `time`, `timeEnd`, `table`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path maps console methods to Rust print macros or no-ops
- [ ] Parity harness passes with 100% match in all 3 environments
