# Task 200: `ink-class-lifecycle` Example — Class Component Lifecycle Methods

**Priority:** P1-High
**Phase:** 17 — React Component Patterns
**Depends on:** 199

## Problem

React class component lifecycle methods (`componentDidMount`, `componentDidUpdate`, `componentWillUnmount`) are still widely used in existing codebases. No existing Ink example exercises these methods.

## Ink Example

```tsx
// examples/ink-class-lifecycle/tui/app.tsx
import React, { Component } from 'react';
import { Box, Text } from 'ink';

interface State {
  count: number;
}

class Counter extends Component<{}, State> {
  state: State = { count: 0 };
  private timer?: ReturnType<typeof setInterval>;

  componentDidMount(): void {
    this.timer = setInterval(() => {
      this.setState(s => ({ count: s.count + 1 }));
    }, 1000);
  }

  componentWillUnmount(): void {
    if (this.timer) clearInterval(this.timer);
  }

  render() {
    return <Text>Count: {this.state.count}</Text>;
  }
}

export default function App() {
  return (
    <Box flexDirection="column">
      <Counter />
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-class-lifecycle/`
- [ ] Uses `Component` class with `componentDidMount`
- [ ] Uses `componentWillUnmount`
- [ ] Uses `setState` with updater function
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for class lifecycle
- [ ] Parity harness passes with 100% match in all 3 environments
