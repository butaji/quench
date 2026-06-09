# Task 222: `ink-super-call` Example — `super()` in Class Constructors

**Priority:** P1-High
**Phase:** 20 — Advanced Language Features
**Depends on:** 221

## Problem

`super()` calls in class constructors pass arguments to the parent constructor. While class components are covered, a dedicated example for `super()` with arguments exercises HIR constructor codegen.

## Ink Example

```tsx
// examples/ink-super-call/tui/app.tsx
import React, { Component } from 'react';
import { Box, Text } from 'ink';

class BaseComponent extends Component<{ label: string }> {
  render() {
    return <Text>{this.props.label}</Text>;
  }
}

class DerivedComponent extends BaseComponent {
  constructor(props: { label: string }) {
    super(props);
  }

  render() {
    return (
      <Box flexDirection="column">
        <Text bold>Derived:</Text>
        {super.render()}
      </Box>
    );
  }
}

export default function App() {
  return (
    <Box flexDirection="column">
      <DerivedComponent label="Hello from super" />
    </Box>
  );
}
```


## HIR Coverage

- `ClassMember` and `Class` variants

## Compile-Path Codegen

- `quote_codegen.rs` for class declaration codegen

## Acceptance Criteria

- [ ] Example exists at `examples/ink-super-call/`
- [ ] Uses `super(props)` in constructor
- [ ] Uses `super.render()` in method override
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for `super()` calls
- [ ] Parity harness passes with 100% match in all 3 environments
