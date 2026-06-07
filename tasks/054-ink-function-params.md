# Task 054: `ink-function-params` Example — Default Params, Rest Params

**Priority:** P1-High
**Phase:** 6 — Functions & Async
**Depends on:** 053

## Problem

Zero examples use default parameters or rest parameters in function declarations.

## Example

```tsx
import { Box, Text } from 'ink';

function greet(name = 'World', ...titles: string[]) {
  const prefix = titles.length > 0 ? titles.join(' ') + ' ' : '';
  return `${prefix}${name}`;
}

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>{greet()}</Text>
      <Text>{greet('Alice')}</Text>
      <Text>{greet('Bob', 'Dr.', 'Prof.')}</Text>
    </Box>
  );
}
```

## Work

Parser's `func_expr_params` ignores defaults and rest. Need to:
1. Parse default values into `Param.default`
2. Parse rest parameter into HIR
3. Codegen defaults as `unwrap_or(default)`
4. Codegen rest as `Vec` parameter

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] Default parameters produce compilable Rust
- [ ] Rest parameters (`...args`) produce compilable Rust
- [ ] `runts build --release` produces working binary with 100% output match
