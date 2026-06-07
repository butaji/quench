# Task 047: `ink-destructure-config` Example — Destructuring, Defaults, Rest

**Priority:** P1-High  
**Phase:** 6 — Data Structures  
**Depends on:** 046

## Problem

Only 3 examples use destructuring. Codegen for defaults and rest has known bugs.

## Example

```tsx
import { Box, Text } from 'ink';

interface Theme {
  title: string;
  colors?: { primary?: string; secondary?: string };
  items: string[];
}

export default function App({ config }: { config: Theme }) {
  const { title, colors = { primary: 'white' }, items } = config;
  const { primary, secondary = 'gray' } = colors;
  const [first, ...rest] = items;

  return (
    <Box flexDirection="column">
      <Text color={primary}>{title}</Text>
      <Text color={secondary}>First: {first}</Text>
      <Text>Rest ({rest.length}): {rest.join(', ')}</Text>
    </Box>
  );
}
```

## Work

Fix `gen_pat` in `quote_codegen_stmts.inc`:
- `Pat::Default` — ensure `unwrap_or` is on the binding, not the access
- `Pat::Rest` — slice array instead of cloning source

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] Object destructuring with defaults produces compilable Rust
- [ ] Array destructuring with rest produces compilable Rust
- [ ] Nested destructuring produces compilable Rust
- [ ] `runts build --release` produces working binary
- [ ] Parity harness 100%
