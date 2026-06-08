# Task 175: `ink-regexp-test-exec` Example — `RegExp.prototype.test`, `exec`

**Priority:** P1-High
**Phase:** 16 — Runtime API Completion
**Depends on:** 174

## Problem

`RegExp.prototype.test` and `exec` are the core RegExp instance methods. Task 099 covers `matchAll` but not `test`/`exec`.

## Ink Example

```tsx
// examples/ink-regexp-test-exec/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const pattern = /\d+/;
const text = 'abc123def456';

const isMatch = pattern.test(text);
const match = pattern.exec(text);
const globalPattern = /\d+/g;
const allMatches: string[] = [];
let m: RegExpExecArray | null;
while ((m = globalPattern.exec(text)) !== null) {
  allMatches.push(m[0]);
}

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Test: {isMatch ? 'yes' : 'no'}</Text>
      <Text>Exec: {match ? match[0] : 'none'}</Text>
      <Text>All: {allMatches.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-regexp-test-exec/`
- [ ] Uses `RegExp.prototype.test` and `exec`
- [ ] Uses `exec` in a loop with global flag
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
