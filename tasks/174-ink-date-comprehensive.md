# Task 174: `ink-date-comprehensive` Example — `getTime`, `getFullYear`, `getMonth`, `getDate`, `toISOString`, `toUTCString`

**Priority:** P1-High
**Phase:** 16 — Runtime API Completion
**Depends on:** 173

## Problem

Comprehensive `Date` methods (`getTime`, `getFullYear`, `getMonth`, `getDate`, `toISOString`, `toUTCString`) are commonly used. Task 118 covers basic Date but not the full API surface.

## Ink Example

```tsx
// examples/ink-date-comprehensive/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const now = new Date('2024-06-15T10:30:00Z');
const time = now.getTime();
const year = now.getFullYear();
const month = now.getMonth() + 1;
const date = now.getDate();
const iso = now.toISOString();
const utc = now.toUTCString();

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Time: {time}</Text>
      <Text>Year: {year}</Text>
      <Text>Month: {month}</Text>
      <Text>Date: {date}</Text>
      <Text>ISO: {iso}</Text>
      <Text>UTC: {utc}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-date-comprehensive/`
- [ ] Uses `getTime`, `getFullYear`, `getMonth`, `getDate`, `toISOString`, `toUTCString`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
