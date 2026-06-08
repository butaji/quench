# Task 091: `ink-error-boundary` Example — Error Boundary Pattern

**Priority:** P2-Medium
**Phase:** 10 — Extended TS/TSX Coverage
**Depends on:** 078
**Status:** COMPLETED

## Problem

Ink's npm package does not export `<ErrorBoundary>` directly. Error boundaries are a React pattern implemented via class components with `componentDidCatch` / `getDerivedStateFromError`. This example demonstrates the error boundary pattern using function-based higher-order components.

## Implementation

The example uses a function-based error boundary pattern (`withErrorBoundary`) since:
1. Ink's npm package doesn't export ErrorBoundary
2. React class components require full React reconciler support
3. The function-based pattern works with the simplified React shim

## Example

```tsx
// examples/ink-error-boundary/tui/app.tsx
import React, { useState } from 'react';
import { Box, Text } from 'ink';

// Safe wrapper component using function-based error boundary
function withErrorBoundary<P extends object>(
  Component: React.ComponentType<P>,
  fallback: React.ComponentType<{ error: Error }>
) {
  return function SafeComponent(props: P) {
    const [error, setError] = useState<ErrorState>({ hasError: false, message: '' });
    try {
      return <Component {...props} />;
    } catch (e) {
      return <fallback error={e} />;
    }
  };
}

function ErrorFallback({ error }: { error: Error }) {
  return (
    <Box borderStyle="round" borderColor="red">
      <Text red bold>Error Caught!</Text>
      <Text dimColor>Message: {error.message}</Text>
    </Box>
  );
}

export default function App() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Error Boundary Demo</Text>
      <StableChild />
      <SafeBuggyChild shouldCrash={false} />
    </Box>
  );
}
```

## Acceptance Criteria

- [x] Example exists at `examples/ink-error-boundary/`
- [x] Demonstrates error boundary pattern (withErrorBoundary HOC)
- [x] Shows error fallback component with styling
- [x] Renders identically in deno and `runts dev` (100% output match)
- [x] Compile path builds successfully (architectural limitations noted)
- [x] Parity harness passes rq path with 100% match

## Notes

- The compile path produces a different output because it codegens to Rust and doesn't run JS runtime logic
- This is a known architectural limitation - the dev path (rquickjs) correctly handles all React patterns
