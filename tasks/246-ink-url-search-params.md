# Task 246: `ink-url-search-params` Example — `URLSearchParams`

**Priority:** P2-Medium
**Phase:** 21 — Runtime API Deep Coverage
**Depends on:** 245

## Problem

`URLSearchParams` parses and manipulates URL query strings. Task 218 covers `URL` but not `URLSearchParams`.

## Ink Example

```tsx
// examples/ink-url-search-params/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const params = new URLSearchParams('?name=Alice&age=30');
  params.append('city', 'NYC');

  return (
    <Box flexDirection="column">
      <Text>Name: {params.get('name')}</Text>
      <Text>Age: {params.get('age')}</Text>
      <Text>City: {params.get('city')}</Text>
      <Text>Query: {params.toString()}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-url-search-params/`
- [ ] Uses `URLSearchParams` constructor and methods
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for URLSearchParams
- [ ] Parity harness passes with 100% match in all 3 environments
