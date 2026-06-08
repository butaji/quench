# Task 131: `ink-function-overloads` Example — Function Overloads

**Priority:** P2-Medium
**Phase:** 12 — Type System Deep Coverage
**Depends on:** 130

## Problem

Function overloads allow a function to have multiple type signatures with different parameter/return types. No existing Ink example explicitly exercises this core TypeScript feature.

## Ink Example

```tsx
// examples/ink-function-overloads/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function format(input: string): string;
function format(input: number): string;
function format(input: string | number): string {
  if (typeof input === 'string') {
    return input.toUpperCase();
  }
  return `Number: ${input}`;
}

class Formatter {
  format(input: string): string;
  format(input: number): string;
  format(input: string | number): string {
    return format(input);
  }
}

const fmt = new Formatter();

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>String: {format('hello')}</Text>
      <Text>Number: {format(42)}</Text>
      <Text>Class string: {fmt.format('world')}</Text>
      <Text>Class number: {fmt.format(99)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-function-overloads/`
- [ ] Uses function overloads on standalone function
- [ ] Uses function overloads on class method
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases overloads without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
