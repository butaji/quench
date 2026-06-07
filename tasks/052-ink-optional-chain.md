# Task 052: `ink-optional-chain` Example — `?.` (Optional Chaining)

**Priority:** P1-High  
**Phase:** 6 — Expressions & Operators  
**Depends on:** 051

## Problem

Optional chaining `?.` is **not represented in HIR** — parser falls through to `Expr::Invalid`.

## Example

```tsx
import { Box, Text } from 'ink';

interface Settings {
  theme?: { name?: string; colors?: string[] };
}

export default function App({ settings }: { settings: Settings }) {
  const themeName = settings.theme?.name;
  const primaryColor = settings.theme?.colors?.[0];

  return (
    <Box flexDirection="column">
      <Text>Name: {themeName ?? 'none'}</Text>
      <Text>Color: {primaryColor ?? 'default'}</Text>
    </Box>
  );
}
```

## Work

**This example REQUIRES Task 075 (HIR optional chaining) to be completed first.**

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] `obj?.prop`, `obj?.[key]` parse into HIR (not `Invalid`)
- [ ] Optional chaining codegen produces compilable Rust
- [ ] `runts build --release` produces working binary
- [ ] Parity harness 100%
