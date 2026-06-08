# Task 198: `ink-dangerously-set-inner-html` Example — `dangerouslySetInnerHTML`

**Priority:** P2-Medium
**Phase:** 17 — React JSX Edge Cases
**Depends on:** 197

## Problem

`dangerouslySetInnerHTML` is a React JSX prop for injecting raw HTML. While Ink is a terminal renderer (not HTML), testing this prop exercises HIR JSX attribute handling for object-valued props with nested objects. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-dangerously-set-inner-html/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  // Note: Ink does not support dangerouslySetInnerHTML.
  // This example exercises the JSX attribute parser.
  const content = { __html: 'Raw content' };

  return (
    <Box flexDirection="column">
      <Text>This example exercises JSX object prop parsing.</Text>
      <Text>Content: {content.__html}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-dangerously-set-inner-html/`
- [ ] Exercises JSX object attribute parsing (`{ __html: '...' }`)
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles JSX object attribute values
- [ ] Parity harness passes with 100% match in all 3 environments
