# Task 045: `ink-destructure` Example — Destructuring, Defaults, Rest

**Priority:** P1-High  
**Phase:** 6 — Data Structures  
**Depends on:** 044

## Problem

Only 3 examples use destructuring. Defaults and rest patterns are not validated.

## Example

```tsx
import { Box, Text } from 'ink';

interface Theme {
  title: string;
  colors?: { primary?: string; secondary?: string };
  padding?: [number, number?];
  items: string[];
}

export default function App({ config }: { config: Theme }) {
  const { title, colors = { primary: 'white' }, items } = config;
  const { primary = 'white', secondary = 'gray' } = colors;
  const [top, bottom = top] = padding || [0];
  const [first, ...rest] = items;

  return (
    <Box flexDirection="column">
      <Text color={primary}>{title}</Text>
      <Text color={secondary}>First: {first}</Text>
      <Text>Rest ({rest.length}): {rest.join(', ')}</Text>
      <Text>Padding: {top} {bottom}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] Object destructuring with defaults produces compilable Rust
- [ ] Array destructuring with rest produces compilable Rust
- [ ] Nested destructuring produces compilable Rust
- [ ] `runts build --release` produces working binary with 100% output match
