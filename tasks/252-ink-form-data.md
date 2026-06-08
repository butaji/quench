# Task 252: `ink-form-data` Example — FormData API

**Priority:** P2-Medium
**Phase:** 22 — Web APIs + Event System
**Depends on:** 251

## Problem

`FormData` builds sets of key/value pairs representing form fields. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-form-data/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const form = new FormData();
  form.append('name', 'Alice');
  form.append('age', '30');
  form.set('city', 'NYC');

  return (
    <Box flexDirection="column">
      <Text>Name: {form.get('name')}</Text>
      <Text>Age: {form.get('age')}</Text>
      <Text>City: {form.get('city')}</Text>
      <Text>Has name: {String(form.has('name'))}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-form-data/`
- [ ] Uses `FormData.append`, `set`, `get`, `has`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for FormData
- [ ] Parity harness passes with 100% match in all 3 environments
