# Task 154: `ink-hoc` Example — Higher-Order Components

**Priority:** P1-High
**Phase:** 14 — React Pattern Coverage
**Depends on:** 153

## Problem

Higher-Order Components (HOCs) are a React pattern for component composition via functions that take a component and return a new component. No existing Ink example explicitly exercises them.

## Ink Example

```tsx
// examples/ink-hoc/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

interface WithPrefixProps {
  prefix: string;
}

function withPrefix<P extends object>(
  WrappedComponent: React.ComponentType<P>
): React.FC<P & WithPrefixProps> {
  return ({ prefix, ...props }: P & WithPrefixProps) => (
    <Box flexDirection="column">
      <Text>{prefix}</Text>
      <WrappedComponent {...props as P} />
    </Box>
  );
}

function Message({ text }: { text: string }) {
  return <Text>{text}</Text>;
}

const PrefixedMessage = withPrefix(Message);

export default function App() {
  return (
    <Box flexDirection="column">
      <PrefixedMessage prefix="INFO:" text="Hello World" />
    </Box>
  );
}
```

## Acceptance Criteria

- [x] Example exists at `examples/ink-hoc/`
- [x] Uses HOC pattern with generic type parameter
- [x] Renders identically in deno and `runts dev` (100% output match)
- [x] Compile path generates compilable Rust
- [x] Parity harness passes with 100% match in all 3 environments

## Implementation Notes

Created comprehensive example demonstrating:
- `withLoading` HOC - shows loading state
- `withCounter` HOC - adds increment functionality
- `withLogger` HOC - adds logging (via useEffect)
- `withBorder` HOC - wraps component with border
- `withColor` HOC - wraps text with color
- Chained HOCs (withLoading + withBorder, withLogger + withCounter)

Added `test_ink_hoc` test to `src/transpile/tests/rq_parity/mod.rs`.
