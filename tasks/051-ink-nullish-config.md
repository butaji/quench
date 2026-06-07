# Task 051: `ink-nullish-config` Example — `??` (Nullish Coalescing)

**Priority:** P1-High  
**Phase:** 6 — Expressions & Operators  
**Depends on:** 050

## Problem

Zero examples use nullish coalescing `??`.

## Example

```tsx
import { Box, Text } from 'ink';

interface Settings {
  theme?: { name?: string };
}

export default function App({ settings }: { settings: Settings }) {
  const themeName = settings.theme?.name ?? 'default';

  return (
    <Box>
      <Text>Theme: {themeName}</Text>
    </Box>
  );
}
```

## Work

Ensure `gen_logical_expr` for `NullishCoalescing` compiles for both `Value` and `Option` types.

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] `??` produces compilable Rust
- [ ] `runts build --release` produces working binary
- [ ] Parity harness 100%
