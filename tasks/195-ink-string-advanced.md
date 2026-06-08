# Task 195: `ink-string-advanced` Example — `String` Prototype Methods (localeCompare, normalize, codePointAt, fromCodePoint)

**Priority:** P2-Medium
**Phase:** 17 — Runtime API Deep Coverage
**Depends on:** 194

## Problem

Advanced `String` prototype methods (`localeCompare`, `normalize`, `codePointAt`, `fromCodePoint`, `concat`, `charAt`, `charCodeAt`) are not covered by any existing task. Task 113 covers `String` modern methods but not these specific ones.

## Ink Example

```tsx
// examples/ink-string-advanced/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const a = 'hello';
  const b = 'world';
  const comparison = a.localeCompare(b);
  const normalized = 'caf\u00e9'.normalize('NFC');
  const cp = 'A'.codePointAt(0);
  const fromCp = String.fromCodePoint(65);
  const concat = a.concat(' ', b);
  const char = a.charAt(0);
  const charCode = a.charCodeAt(0);

  return (
    <Box flexDirection="column">
      <Text>Compare: {comparison}</Text>
      <Text>Normalized: {normalized}</Text>
      <Text>CodePoint: {cp}</Text>
      <Text>FromCodePoint: {fromCp}</Text>
      <Text>Concat: {concat}</Text>
      <Text>Char: {char}</Text>
      <Text>CharCode: {charCode}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-string-advanced/`
- [ ] Uses `localeCompare`, `normalize`, `codePointAt`, `fromCodePoint`, `concat`, `charAt`, `charCodeAt`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
