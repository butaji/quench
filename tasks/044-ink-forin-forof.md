# Task 044: `ink-forin-forof` Example — `for-in`, `for-of`, Iterators

**Priority:** P1-High  
**Phase:** 6 — Control Flow  
**Depends on:** 043

## Problem

Zero examples use `for-in` or `for-of`.

## Example

```tsx
import { Box, Text } from 'ink';

export default function App({ obj, arr }: { obj: Record<string, number>; arr: string[] }) {
  const keys: string[] = [];
  for (const k in obj) {
    keys.push(`${k}=${obj[k]}`);
  }

  const items: string[] = [];
  for (const item of arr) {
    items.push(item.toUpperCase());
  }

  return (
    <Box flexDirection="column">
      <Text bold>Keys:</Text>
      {keys.map((k, i) => <Text key={i}>{k}</Text>)}
      <Text bold>Items:</Text>
      {items.map((item, i) => <Text key={i}>{item}</Text>)}
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] `for-in` and `for-of` produce compilable Rust
- [ ] `runts build --release` produces working binary with 100% output match
