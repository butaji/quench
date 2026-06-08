# Task 366: `ink-date-locale` Example — `Date.prototype.toLocaleDateString` / `toLocaleTimeString` / `toDateString` / `toTimeString`

**Priority:** P2-Medium
**Phase:** 29 — Date Methods Completion
**Depends on:** 365

## Problem

`Date.prototype.toLocaleDateString`, `toLocaleTimeString`, `toDateString`, `toTimeString`, `toUTCString`, `toISOString`, `toString` are core date formatting methods. Tasks 118/174 cover some Date methods; no dedicated example covers all locale formatting methods.

## Ink Example

```tsx
// examples/ink-date-locale/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const date = new Date('2024-01-15T10:30:00');

  return (
    <Box flexDirection="column">
      <Text>toISOString: {date.toISOString()}</Text>
      <Text>toDateString: {date.toDateString()}</Text>
      <Text>toTimeString: {date.toTimeString()}</Text>
      <Text>toUTCString: {date.toUTCString()}</Text>
      <Text>toLocaleDateString: {date.toLocaleDateString('en-US')}</Text>
      <Text>toLocaleTimeString: {date.toLocaleTimeString('en-US')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-date-locale/`
- [ ] Uses `toLocaleDateString`, `toLocaleTimeString`, `toDateString`, `toTimeString`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
