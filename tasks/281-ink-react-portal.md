# Task 281: `ink-react-portal` Example — `ReactDOM.createPortal`

**Priority:** P3-Low
**Phase:** 23 — React Patterns
**Depends on:** 280

## Problem

`createPortal` renders children into a DOM node outside the parent hierarchy. Ink is terminal-based so true portals are not applicable, but the API may appear in shared component code.

## Ink Example

```tsx
// examples/ink-react-portal/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const container = { nodeType: 1 };

  return (
    <Box flexDirection="column">
      {ReactDOM.createPortal(
        <Text>Portal content</Text>,
        container as any
      )}
    </Box>
  );
}
```


## HIR Coverage

- React hook calls via `Expr::Call`

## Compile-Path Codegen

- `js_bundle/react_shim.rs` for hook definitions

## Acceptance Criteria

- [ ] Example exists at `examples/ink-react-portal/`
- [ ] Uses `ReactDOM.createPortal`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path either supports portal or produces clear error
- [ ] Parity harness passes with 100% match in all 3 environments
