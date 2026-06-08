# Task 233: `ink-jsx-namespace` Example — JSX Namespaced Elements (`<ns:tag />`)

**Priority:** P2-Medium
**Phase:** 20 — JSX Advanced Patterns
**Depends on:** 232

## Problem

JSX namespaced elements (`<svg:circle />`) are used in XML-like vocabularies. No existing Ink example exercises this pattern.

## Ink Example

```tsx
// examples/ink-jsx-namespace/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

// Note: Ink does not use XML namespaces, but this exercises the parser.
const SvgCircle = () => <Text>(SVG namespace example)</Text>;

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Namespace example:</Text>
      <SvgCircle />
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-jsx-namespace/`
- [ ] Documents JSX namespace syntax
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles or documents JSX namespace behavior
- [ ] Parity harness passes with 100% match in all 3 environments
