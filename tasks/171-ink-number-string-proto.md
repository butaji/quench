# Task 171: `ink-number-string-proto` Example — Number/String Prototype Methods

**Priority:** P1-High
**Phase:** 16 — Runtime API Completion
**Depends on:** 168

## Problem

Common `Number.prototype` methods (`toFixed`, `toPrecision`, `toExponential`) and `String.prototype` methods (`charAt`, `charCodeAt`, `substring`, `slice`, `toLowerCase`, `toUpperCase`) are not explicitly covered by any Ink example.

## Ink Example

```tsx
// examples/ink-number-string-proto/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const num = 3.14159;
const fixed = num.toFixed(2);
const precision = num.toPrecision(3);
const exp = (1234).toExponential(2);

const text = 'Hello TypeScript';
const char = text.charAt(0);
const code = text.charCodeAt(0);
const sub = text.substring(0, 5);
const slice = text.slice(-10);
const lower = text.toLowerCase();
const upper = text.toUpperCase();

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Fixed: {fixed}</Text>
      <Text>Precision: {precision}</Text>
      <Text>Exp: {exp}</Text>
      <Text>Char: {char}</Text>
      <Text>Code: {code}</Text>
      <Text>Sub: {sub}</Text>
      <Text>Slice: {slice}</Text>
      <Text>Lower: {lower}</Text>
      <Text>Upper: {upper}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-number-string-proto/`
- [ ] Uses `toFixed`, `toPrecision`, `toExponential`
- [ ] Uses `charAt`, `charCodeAt`, `substring`, `slice`, `toLowerCase`, `toUpperCase`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
