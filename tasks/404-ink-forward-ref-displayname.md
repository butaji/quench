# Task 404: `ink-forward-ref-displayname` Example — forwardRef with displayName

**Priority:** P2-Medium
**Phase:** 32 — Core Language + React + Runtime Edge Cases
**Depends on:** 403

## Problem

`forwardRef` components can have a `displayName` set on the returned component. No existing Ink example explicitly exercises this pattern.

## HIR Coverage

- `Expr::Call` for `React.forwardRef`.
- `Expr::Assign` for `displayName` property assignment.

## Compile-Path Codegen

- `js_bundle/react_shim.rs` must support `forwardRef` with `displayName`.
- The returned component object must expose `displayName` property.

## Ink Example

```tsx
// examples/ink-forward-ref-displayname/tui/app.tsx
import React, { forwardRef } from 'react';
import { Box, Text } from 'ink';

interface FancyInputProps {
  label: string;
}

const FancyInput = forwardRef<any, FancyInputProps>(({ label }, ref) => {
  return (
    <Box>
      <Text>{label}</Text>
    </Box>
  );
});

FancyInput.displayName = 'FancyInput';

export default function App() {
  return (
    <Box flexDirection="column">
      <FancyInput label="Demo" />
      <Text>DisplayName: {FancyInput.displayName}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-forward-ref-displayname/`
- [ ] Uses `forwardRef` with `displayName` assignment
- [ ] React shim exposes `displayName` on forwardRef components
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
