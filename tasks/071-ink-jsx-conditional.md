# Task 071: `ink-jsx-conditional` Example — Conditional Rendering, List Rendering, Keys

**Priority:** P2-Medium
**Phase:** 6 — JSX Advanced
**Depends on:** 070

## Problem

Conditional rendering (`condition && <Component />`), list rendering with keys, and fragments are not validated by a dedicated example.

## Example

```tsx
import { Box, Text } from 'ink';

export default function App({ items, showHeader }: { items: string[]; showHeader: boolean }) {
  return (
    <Box flexDirection="column">
      {showHeader && <Text bold>Header</Text>}
      {items.length === 0 ? (
        <Text dimColor>No items</Text>
      ) : (
        <>
          {items.map((item, i) => (
            <Text key={item}>{i + 1}. {item}</Text>
          ))}
        </>
      )}
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] Conditional rendering (`&&` and ternary) produces compilable Rust
- [ ] List rendering with keys produces compilable Rust
- [ ] Fragments produce compilable Rust
- [ ] `runts build --release` produces working binary
- [ ] Parity harness 100%