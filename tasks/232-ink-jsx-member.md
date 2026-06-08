# Task 232: `ink-jsx-member` Example — JSX Member Expressions (`<My.Component />`)

**Priority:** P1-High
**Phase:** 20 — JSX Advanced Patterns
**Depends on:** 231

## Problem

JSX member expressions (`<MyComponents.DatePicker />`) allow referencing components through object property access. No existing Ink example exercises this pattern.

## Ink Example

```tsx
// examples/ink-jsx-member/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const Components = {
  Header: ({ title }: { title: string }) => <Text bold>{title}</Text>,
  Body: ({ content }: { content: string }) => <Text>{content}</Text>,
};

export default function App() {
  return (
    <Box flexDirection="column">
      <Components.Header title="Welcome" />
      <Components.Body content="This is a member expression component." />
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-jsx-member/`
- [ ] Uses JSX member expression `<Components.Header />`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for JSX member expressions
- [ ] Parity harness passes with 100% match in all 3 environments
