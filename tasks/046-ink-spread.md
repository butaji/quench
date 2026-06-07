# Task 046: `ink-spread` Example — Object Spread, Array Spread, JSX Spread

**Priority:** P1-High  
**Phase:** 6 — Data Structures  
**Depends on:** 045

## Problem

Only 4 examples use spread. Object spread, array spread, and JSX spread attributes are not validated together.

## Example

```tsx
import { Box, Text } from 'ink';

export default function App({ base }: { base: { color: string; bold: boolean } }) {
  const merged = { ...base, dimColor: true };
  const arr1 = ['a', 'b'];
  const arr2 = [...arr1, 'c', 'd'];

  return (
    <Box flexDirection="column">
      <Text {...merged}>Spread props</Text>
      <Text>Array: {arr2.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] Object spread in literals produces compilable Rust
- [ ] Array spread produces compilable Rust
- [ ] JSX spread attributes produce compilable Rust
- [ ] `runts build --release` produces working binary with 100% output match
