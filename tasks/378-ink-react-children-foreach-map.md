# Task 378: `ink-react-children-foreach-map` Example — `Children.forEach`, `Children.map`, `Children.count`

**Priority:** P2-Medium
**Phase:** 31 — Advanced JSX + React Edge Cases
**Depends on:** 377

## Problem

Task 117 covers `Children` API broadly but only tasks 333–334 explicitly test `Children.only` and `Children.toArray`. The remaining `Children` helpers (`forEach`, `map`, `count`) are common in React component libraries and need explicit parity coverage.

## HIR Coverage

This example validates standard `Expr::Call` + `Expr::Member` HIR variants for `React.Children.forEach`, `.map`, and `.count`. No new HIR variants are required.

## Compile-Path Codegen

- `quote_codegen_exprs.inc` must emit compilable Rust for chained `React.Children.*` method calls.
- The React shim must expose `Children.forEach`, `Children.map`, `Children.count`.

## Ink Example

```tsx
// examples/ink-react-children-foreach-map/tui/app.tsx
import React, { Children } from 'react';
import { Box, Text } from 'ink';

function Summary({ children }: { children: React.ReactNode }) {
  const count = Children.count(children);
  const labels: string[] = [];

  Children.forEach(children, (child) => {
    if (React.isValidElement(child)) {
      labels.push(String(child.props.label ?? 'unknown'));
    }
  });

  const mapped = Children.map(children, (child, index) => {
    return <Text key={index}>- {index}: mapped</Text>;
  });

  return (
    <Box flexDirection="column">
      <Text>Count: {count}</Text>
      <Text>Labels: {labels.join(', ')}</Text>
      <Box flexDirection="column">{mapped}</Box>
    </Box>
  );
}

export default function App() {
  return (
    <Summary>
      <Text label="a">A</Text>
      <Text label="b">B</Text>
      <Text label="c">C</Text>
    </Summary>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-react-children-foreach-map/`
- [ ] Uses `Children.forEach`, `Children.map`, and `Children.count`
- [ ] React shim exposes all three helpers
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
