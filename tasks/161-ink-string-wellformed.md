# Task 161: `ink-string-wellformed` Example — `isWellFormed`, `toWellFormed`

**Priority:** P2-Medium
**Phase:** 14 — Runtime API Completion
**Depends on:** 160

## Problem

`String.prototype.isWellFormed()` and `toWellFormed()` (ES2023) detect and fix lone surrogates in strings. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-string-wellformed/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const valid = 'hello';
const invalid = 'hello \uD800'; // lone surrogate

const isValidWellFormed = valid.isWellFormed();
const isInvalidWellFormed = invalid.isWellFormed();
const fixed = invalid.toWellFormed();

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Valid well-formed: {isValidWellFormed ? 'yes' : 'no'}</Text>
      <Text>Invalid well-formed: {isInvalidWellFormed ? 'yes' : 'no'}</Text>
      <Text>Fixed: {fixed}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-string-wellformed/`
- [ ] Uses `isWellFormed()` and `toWellFormed()`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
