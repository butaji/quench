# Task 204: `ink-jsx-spread-attribute` Example — JSX Spread Attributes `{...props}`

**Priority:** P1-High
**Phase:** 17 — JSX Advanced Patterns
**Depends on:** 203

## Problem

JSX spread attributes (`<Component {...props} />`) dynamically pass all properties of an object as props. Task 047 covers spread in object literals and Task 048 covers spread props, but no task explicitly covers JSX spread attributes in element position.

## Ink Example

```tsx
// examples/ink-jsx-spread-attribute/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const boxProps = { borderStyle: 'round' as const, padding: 1 };
const textProps = { color: 'green' as const, bold: true };

export default function App() {
  return (
    <Box flexDirection="column" {...boxProps}>
      <Text {...textProps}>Spread attributes work!</Text>
      <Text>Normal text</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-jsx-spread-attribute/`
- [ ] Uses JSX spread attribute `{...props}`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for JSX spread attributes
- [ ] Parity harness passes with 100% match in all 3 environments
