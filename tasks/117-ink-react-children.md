# Task 117: `ink-react-children` Example — `Children` API, `cloneElement`, `isValidElement`

**Priority:** P2-Medium
**Phase:** 11 — React API Coverage
**Depends on:** 078

## Problem

`React.Children`, `React.cloneElement`, and `React.isValidElement` are core React APIs for manipulating JSX children. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-react-children/tui/app.tsx
import React, { Children, cloneElement, isValidElement } from 'react';
import { Box, Text } from 'ink';

interface ListProps {
  children: React.ReactNode;
  prefix: string;
}

function PrefixedList({ children, prefix }: ListProps) {
  const count = Children.count(children);
  return (
    <Box flexDirection="column">
      <Text>Items: {count}</Text>
      {Children.map(children, (child, i) => {
        if (isValidElement(child)) {
          return cloneElement(child, { key: i });
        }
        return child;
      })}
    </Box>
  );
}

export default function App() {
  return (
    <PrefixedList prefix="- ">
      <Text>Apple</Text>
      <Text>Banana</Text>
      <Text>Cherry</Text>
    </PrefixedList>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-react-children/`
- [ ] Uses `Children.count`, `Children.map`
- [ ] Uses `cloneElement` and `isValidElement`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
