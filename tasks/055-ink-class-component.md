# Task 055: `ink-class-component` Example — Classes, `extends`, `super`

**Priority:** P2-Medium
**Phase:** 6 — Classes & OOP
**Depends on:** 054

## Problem

Zero examples use classes. `Stmt::Class` codegen returns `None`.

## Example

```tsx
import React, { Component } from 'react';
import { Box, Text } from 'ink';

interface Props { initial: number; }
interface State { count: number; }

class Counter extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { count: props.initial };
  }

  render() {
    return (
      <Box>
        <Text>Count: {this.state.count}</Text>
      </Box>
    );
  }
}

export default Counter;
```

## Work

Implement `gen_class` in `quote_codegen_stmts.inc`:
- Convert TS class to Rust struct + impl block
- Handle `extends` via composition
- Handle `constructor` as `new()`
- Handle instance methods

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] `Stmt::Class` codegen produces compilable Rust (not `None`)
- [ ] Constructor and methods generate valid Rust
- [ ] `runts build --release` produces working binary with 100% output match
