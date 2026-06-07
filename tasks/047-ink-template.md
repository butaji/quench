# Task 047: `ink-template` Example — Template Literals, Multiline, Tagged

**Priority:** P1-High  
**Phase:** 6 — Data Structures  
**Depends on:** 046

## Problem

10 examples use template literals but edge cases (multiple interpolations, multiline, nested braces) are not validated.

## Example

```tsx
import { Box, Text } from 'ink';

export default function App({ name, count }: { name: string; count: number }) {
  const greeting = `Hello, ${name}!`;
  const status = `You have ${count} message${count === 1 ? '' : 's'}`;
  const multiline = `Line 1
Line 2
Line 3`;

  return (
    <Box flexDirection="column">
      <Text>{greeting}</Text>
      <Text>{status}</Text>
      <Text>{multiline}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] Template literals with multiple interpolations produce compilable Rust
- [ ] Multiline templates produce compilable Rust
- [ ] `runts build --release` produces working binary with 100% output match
