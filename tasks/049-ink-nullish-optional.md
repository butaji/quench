# Task 049: `ink-nullish-optional` Example — `??` and `?.` (Optional Chaining)

**Priority:** P1-High
**Phase:** 6 — Expressions & Operators
**Depends on:** 048

## Problem

Zero examples use nullish coalescing `??` or optional chaining `?.`. Optional chaining is **not represented in HIR** — parser falls through to `Expr::Invalid`.

## Example

```tsx
import { Box, Text } from 'ink';

interface Settings {
  theme?: { name?: string; colors?: string[] };
}

export default function App({ settings }: { settings: Settings }) {
  const themeName = settings.theme?.name ?? 'default';
  const primaryColor = settings.theme?.colors?.[0] ?? 'white';

  return (
    <Box flexDirection="column">
      <Text>Name: {themeName}</Text>
      <Text color={primaryColor}>Primary</Text>
    </Box>
  );
}
```

## Work

**Requires Task 071 (HIR optional chaining) first.** After that:
- Verify `??` codegen compiles for both `Value` and `Option` types
- Ensure `?.` desugars to conditional access

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] `obj?.prop`, `obj?.[key]` parse into HIR (not `Invalid`)
- [ ] `??` and `?.` codegen produce compilable Rust
- [ ] `runts build --release` produces working binary with 100% output match
