# Task 143: `ink-parameter-properties` Example — `constructor(public x: string)`

**Priority:** P1-High
**Phase:** 12 — Type System Deep Coverage
**Depends on:** 083

## Problem

Parameter properties (`constructor(public name: string)`) are a concise TypeScript pattern for declaring and initializing class members. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-parameter-properties/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

class User {
  constructor(
    public name: string,
    public age: number,
    private readonly id: string,
    protected role: string = 'user'
  ) {}

  describe(): string {
    return `${this.name} (${this.age}) [${this.id}]`;
  }
}

const user = new User('Alice', 30, 'u-123');

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>{user.describe()}</Text>
      <Text>Name: {user.name}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-parameter-properties/`
- [ ] Uses parameter properties with `public`, `private`, `protected`, `readonly`
- [ ] Uses default values in parameter properties
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path maps parameter properties to regular class fields
- [ ] Parity harness passes with 100% match in all 3 environments
