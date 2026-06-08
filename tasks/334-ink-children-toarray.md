# Task 334: `ink-children-toarray` Example — `Children.toArray`

**Priority:** P2-Medium
**Phase:** 27 — React Children API
**Depends on:** 333

## Problem

`React.Children.toArray(children)` flattens and assigns keys to children. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-children-toarray/tui/app.tsx
import React, { Children } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const items = Children.toArray([
    <Text key="a">A</Text>,
    <Text key="b">B</Text>,
    null,
    <Text key="c">C</Text>,
  ]);

  return (
    <Box flexDirection="column">
      <Text>Count: {items.length}</Text>
      {items}
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-children-toarray/`
- [ ] Uses `Children.toArray` with nulls
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
