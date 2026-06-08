# Task 199: `ink-default-props` Example — `defaultProps` on Function Components

**Priority:** P2-Medium
**Phase:** 17 — React Component Patterns
**Depends on:** 198

## Problem

`defaultProps` provides default values for component props. While less common with default parameters, it's still used in class components and some functional component patterns. No existing Ink example exercises `defaultProps`.

## Ink Example

```tsx
// examples/ink-default-props/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

interface GreetingProps {
  name: string;
  greeting?: string;
}

function Greeting({ name, greeting = 'Hello' }: GreetingProps) {
  return <Text>{greeting}, {name}!</Text>;
}

export default function App() {
  return (
    <Box flexDirection="column">
      <Greeting name="Alice" />
      <Greeting name="Bob" greeting="Hi" />
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-default-props/`
- [ ] Uses default parameter values (or `defaultProps` assignment)
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
