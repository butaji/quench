# Task 194: `ink-props-with-children` Example — `React.PropsWithChildren`

**Priority:** P1-High
**Phase:** 17 — React Type Patterns
**Depends on:** 193

## Problem

`React.PropsWithChildren<T>` is the standard utility for adding `children` to a props interface. No existing Ink example exercises this utility type.

## Ink Example

```tsx
// examples/ink-props-with-children/tui/app.tsx
import React, { PropsWithChildren } from 'react';
import { Box, Text } from 'ink';

interface CardProps {
  title: string;
}

function Card({ title, children }: PropsWithChildren<CardProps>) {
  return (
    <Box flexDirection="column" borderStyle="round" padding={1}>
      <Text bold>{title}</Text>
      <Text></Text>
      {children}
    </Box>
  );
}

export default function App() {
  return (
    <Card title="Welcome">
      <Text>This is card content.</Text>
      <Text color="green">More content here.</Text>
    </Card>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-props-with-children/`
- [ ] Uses `PropsWithChildren<T>` utility type
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases `PropsWithChildren` without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
