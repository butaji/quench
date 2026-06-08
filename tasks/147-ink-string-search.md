# Task 147: `ink-string-search` Example — `startsWith`, `endsWith`, `includes`, `repeat`

**Priority:** P1-High
**Phase:** 12 — Runtime API Completion
**Depends on:** 146

## Problem

`String.prototype.startsWith`, `endsWith`, `includes`, and `repeat` are ES2015+ string methods. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-string-search/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const text = 'Hello, TypeScript World!';
const starts = text.startsWith('Hello');
const ends = text.endsWith('World!');
const has = text.includes('TypeScript');
const missing = text.includes('Rust');
const repeated = '=-='.repeat(5);
const padded = 'hi'.padStart(6, ' ');

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Starts with Hello: {starts ? 'yes' : 'no'}</Text>
      <Text>Ends with World!: {ends ? 'yes' : 'no'}</Text>
      <Text>Includes TypeScript: {has ? 'yes' : 'no'}</Text>
      <Text>Includes Rust: {missing ? 'yes' : 'no'}</Text>
      <Text>Repeated: {repeated}</Text>
      <Text>Padded: [{padded}]</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-string-search/`
- [ ] Uses `startsWith`, `endsWith`, `includes`, `repeat`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
