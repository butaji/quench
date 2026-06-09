# Task 201: `ink-get-derived-state-from-error` Example — Error Boundary with `static getDerivedStateFromError`

**Priority:** P1-High
**Phase:** 17 — React Component Patterns
**Depends on:** 200

## Problem

`static getDerivedStateFromError` and `componentDidCatch` are the standard React Error Boundary API. Task 091 covers `ErrorBoundary` but not the class-based lifecycle approach. No existing Ink example exercises `getDerivedStateFromError`.

## Ink Example

```tsx
// examples/ink-get-derived-state-from-error/tui/app.tsx
import React, { Component, ReactNode } from 'react';
import { Box, Text } from 'ink';

interface State {
  hasError: boolean;
  error?: Error;
}

class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { hasError: false };

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo): void {
    // Log error info
  }

  render() {
    if (this.state.hasError) {
      return <Text color="red">Error: {this.state.error?.message}</Text>;
    }
    return this.props.children;
  }
}

function RiskyComponent() {
  throw new Error('Deliberate error');
}

export default function App() {
  return (
    <Box flexDirection="column">
      <ErrorBoundary>
        <RiskyComponent />
      </ErrorBoundary>
    </Box>
  );
}
```


## HIR Coverage

- `ClassMember` and `Class` variants
- React hook calls via `Expr::Call`

## Compile-Path Codegen

- `quote_codegen.rs` for class declaration codegen
- `js_bundle/react_shim.rs` for hook definitions

## Acceptance Criteria

- [ ] Example exists at `examples/ink-get-derived-state-from-error/`
- [ ] Uses `static getDerivedStateFromError`
- [ ] Uses `componentDidCatch`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for Error Boundary lifecycle
- [ ] Parity harness passes with 100% match in all 3 environments
