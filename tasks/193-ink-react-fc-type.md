# Task 193: `ink-react-fc-type` Example — `React.FC`, `React.FunctionComponent`

**Priority:** P1-High
**Phase:** 17 — React Type Patterns
**Depends on:** 192

## Problem

`React.FC` and `React.FunctionComponent` are common React type patterns that include implicit `children` prop typing. No existing Ink example exercises these type annotations.

## Ink Example

```tsx
// examples/ink-react-fc-type/tui/app.tsx
import React, { FC, FunctionComponent } from 'react';
import { Box, Text } from 'ink';

interface Props {
  title: string;
}

const Header: FC<Props> = ({ title }) => (
  <Text bold color="cyan">{title}</Text>
);

const SubHeader: FunctionComponent<Props> = ({ title }) => (
  <Text dimColor>{title}</Text>
);

export default function App() {
  return (
    <Box flexDirection="column">
      <Header title="Main Title" />
      <SubHeader title="Subtitle" />
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-react-fc-type/`
- [ ] Uses `React.FC<Props>` type annotation
- [ ] Uses `React.FunctionComponent<Props>` type annotation
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases `FC` type without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
