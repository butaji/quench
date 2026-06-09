# Task 352: `ink-template-literal-union` Example — Template Literal Types with Unions

**Priority:** P2-Medium
**Phase:** 28 — Advanced Type System Patterns
**Depends on:** 351

## Problem

Template literal types with unions (`type EventName<T> = `${T}Changed` | `${T}Updated``) generate string literal unions. No existing Ink example exercises this pattern.

## Ink Example

```tsx
// examples/ink-template-literal-union/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

type Entity = 'user' | 'post';
type EventName = `${Entity}Created` | `${entity}Deleted`;

export default function App() {
  const event: EventName = 'userCreated';

  return (
    <Box flexDirection="column">
      <Text>Event: {event}</Text>
    </Box>
  );
}
```


## HIR Coverage

- `Expr` variants for operators, literals, and call expressions
- Type erasure (no runtime HIR needed)

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Type erasure at parse time (no runtime codegen)

## Acceptance Criteria

- [ ] Example exists at `examples/ink-template-literal-union/`
- [ ] Uses template literal type with union
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases template literal types without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
