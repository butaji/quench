# Task 100: `ink-utility-types` Example — Built-in Utility Types

**Priority:** P1-High
**Phase:** 11 — Type System Deep Coverage
**Depends on:** 078

## Problem

TypeScript's built-in utility types (`Partial`, `Required`, `Readonly`, `Pick`, `Omit`, `Record`, `Exclude`, `Extract`, `NonNullable`, `Parameters`, `ReturnType`) are used in virtually every TS codebase. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-utility-types/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

interface User {
  name: string;
  age: number;
  email?: string;
}

type RequiredUser = Required<User>;
type UserPreview = Pick<User, 'name' | 'age'>;
type UserWithoutEmail = Omit<User, 'email'>;
type UserRecord = Record<string, User>;
type Names = NonNullable<string | null | undefined>;

function getName(user: User): string {
  return user.name;
}

type NameReturn = ReturnType<typeof getName>;
type NameParams = Parameters<typeof getName>;

const preview: UserPreview = { name: 'Alice', age: 30 };
const record: UserRecord = { alice: preview };

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Name: {preview.name}</Text>
      <Text>Age: {preview.age}</Text>
      <Text>Record keys: {Object.keys(record).join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-utility-types/`
- [ ] Uses `Partial`, `Required`, `Readonly`, `Pick`, `Omit`, `Record`, `Exclude`, `Extract`, `NonNullable`, `Parameters`, `ReturnType`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases all utility types without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
