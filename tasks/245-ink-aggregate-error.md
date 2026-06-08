# Task 245: `ink-aggregate-error` Example — `AggregateError`

**Priority:** P2-Medium
**Phase:** 21 — Runtime API Deep Coverage
**Depends on:** 244

## Problem

`AggregateError` wraps multiple errors in a single error object. Task 148 covers error subclasses but not `AggregateError` specifically.

## Ink Example

```tsx
// examples/ink-aggregate-error/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const errors = [new Error('first'), new Error('second')];
  const aggregate = new AggregateError(errors, 'Multiple failures');

  return (
    <Box flexDirection="column">
      <Text>Message: {aggregate.message}</Text>
      <Text>Count: {aggregate.errors.length}</Text>
      <Text>First: {aggregate.errors[0]?.message}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-aggregate-error/`
- [ ] Uses `AggregateError` constructor
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for AggregateError
- [ ] Parity harness passes with 100% match in all 3 environments
