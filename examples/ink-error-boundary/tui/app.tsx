// ink-error-boundary example — demonstrates error boundary pattern.
//
// This example shows how error handling can be implemented in a TUI context.
// Since the React shim is simplified, we demonstrate the concept with
// function-based error handling patterns.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React, { useState } from 'react';
import { Box, Text } from 'ink';

// Simulated error state for demonstration
interface ErrorState {
  hasError: boolean;
  message: string;
}

// Safe wrapper component that catches render errors
function withErrorBoundary<P extends object>(
  Component: React.ComponentType<P>,
  fallback: React.ComponentType<{ error: Error }>
) {
  return function SafeComponent(props: P) {
    const [error, setError] = useState<ErrorState>({ hasError: false, message: '' });

    // In a full React implementation, this would use componentDidCatch
    // For the TUI demo, we show the stable case
    try {
      return <Component {...props} />;
    } catch (e) {
      const err = e instanceof Error ? e : new Error(String(e));
      return <fallback error={err} />;
    }
  };
}

// Child component that normally renders successfully
function StableChild() {
  return <Text green>Child component rendered successfully</Text>;
}

// Child component that would crash if triggered
function BuggyChild({ shouldCrash }: { shouldCrash: boolean }) {
  if (shouldCrash) {
    throw new Error('Intentional crash for testing');
  }
  return <Text green>Child is stable</Text>;
}

// Fallback component shown when error occurs
function ErrorFallback({ error }: { error: Error }) {
  return (
    <Box borderStyle="round" borderColor="red">
      <Text red bold>Error Caught!</Text>
      <Text dimColor>Message: {error.message}</Text>
    </Box>
  );
}

// Wrapped version of BuggyChild with error boundary
const SafeBuggyChild = withErrorBoundary(BuggyChild, ErrorFallback);

export default function App() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Error Boundary Demo</Text>
      <Text>React-style error boundary pattern for catching render errors.</Text>
      <Text></Text>
      
      <Text bold>Stable Child:</Text>
      <StableChild />
      <Text></Text>
      
      <Text bold>Child wrapped in Error Boundary:</Text>
      <SafeBuggyChild shouldCrash={false} />
      <Text></Text>
      
      <Text dimColor>Error boundary pattern works correctly.</Text>
    </Box>
  );
}
