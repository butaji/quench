# Task 343: `ink-hoc-generic` Example — Higher-Order Components with Generics

**Priority:** P2-Medium
**Phase:** 27 — React Patterns
**Depends on:** 342

## Problem

Higher-order components (HOCs) with generics preserve component prop types while adding behavior. Task 154 covers basic HOCs; no example covers generic HOCs.

## Ink Example

```tsx
// examples/ink-hoc-generic/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function withLabel<P extends { label: string }>(
  Component: React.ComponentType<P>
): React.ComponentType<P> {
  return function Wrapped(props: P) {
    return (
      <Box flexDirection="column">
        <Text bold>{props.label}</Text>
        <Component {...props} />
      </Box>
    );
  };
}

function Message({ text }: { label: string; text: string }) {
  return <Text>{text}</Text>;
}

const LabeledMessage = withLabel(Message);

export default function App() {
  return (
    <LabeledMessage label="Important" text="Hello from HOC" />
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-hoc-generic/`
- [ ] Uses generic HOC with prop type preservation
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases generics without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
