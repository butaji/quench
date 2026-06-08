# Task 153: `ink-render-props` Example — Render Props Pattern

**Priority:** P1-High
**Phase:** 14 — React Pattern Coverage
**Depends on:** 117

## Problem

The render props pattern (`<Component render={(data) => <Child />} />) is a classic React composition pattern. No existing Ink example explicitly exercises it.

## Ink Example

```tsx
// examples/ink-render-props/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

interface MouseTrackerProps {
  render: (pos: { x: number; y: number }) => React.ReactNode;
}

function MouseTracker({ render }: MouseTrackerProps) {
  return <>{render({ x: 10, y: 20 })}</>;
}

export default function App() {
  return (
    <Box flexDirection="column">
      <MouseTracker render={({ x, y }) => (
        <Text>Position: ({x}, {y})</Text>
      )} />
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-render-props/`
- [ ] Uses render props pattern with function prop
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments