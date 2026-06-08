# Task 176: `ink-promise-resolve-reject` Example — `Promise.resolve`, `Promise.reject`

**Priority:** P1-High
**Phase:** 16 — Runtime API Completion
**Depends on:** 175

## Problem

`Promise.resolve` and `Promise.reject` are fundamental Promise static methods. No existing Ink example explicitly exercises both.

## Ink Example

```tsx
// examples/ink-promise-resolve-reject/tui/app.tsx
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [results, setResults] = useState<string[]>([]);

  useEffect(() => {
    Promise.resolve('ok').then(v => {
      setResults(prev => [...prev, `Resolved: ${v}`]);
    });

    Promise.reject('err').catch(e => {
      setResults(prev => [...prev, `Rejected: ${e}`]);
    });

    Promise.resolve(Promise.resolve('nested')).then(v => {
      setResults(prev => [...prev, `Nested: ${v}`]);
    });
  }, []);

  return (
    <Box flexDirection="column">
      {results.map((r, i) => (
        <Text key={i}>{r}</Text>
      ))}
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-promise-resolve-reject/`
- [ ] Uses `Promise.resolve` and `Promise.reject`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
