# Task 113: `ink-string-modern` Example — Modern String Methods

**Priority:** P2-Medium
**Phase:** 11 — Runtime API Coverage
**Depends on:** 078

## Problem

Modern string methods (`padStart`, `padEnd`, `replaceAll`, `trimStart`, `trimEnd`, `at`) are standard ES2019+ features. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-string-modern/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const id = '42'.padStart(6, '0');
const label = 'App'.padEnd(10, '.');
const text = 'hello world hello'.replaceAll('hello', 'hi');
const spaced = '  trim  '.trimStart().trimEnd();
const word = 'hello';
const first = word.at(0);
const last = word.at(-1);

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>ID: {id}</Text>
      <Text>Label: {label}</Text>
      <Text>Replaced: {text}</Text>
      <Text>Trimmed: {spaced}</Text>
      <Text>First: {first}, Last: {last}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-string-modern/`
- [ ] Uses `padStart`, `padEnd`, `replaceAll`, `trimStart`, `trimEnd`, `at`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
