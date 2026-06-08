# Task 150: `ink-import-types` Example — `type T = import("./mod").Type`

**Priority:** P1-High
**Phase:** 14 — Type System Deep Coverage
**Depends on:** 142

## Problem

`import` types (`type T = import("./module").SomeType`) allow referencing types from other modules without importing values. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-import-types/types.ts
export interface User {
  name: string;
  age: number;
}
export type ID = string | number;

// examples/ink-import-types/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

type UserType = import('./types.js').User;
type IDType = import('./types.js').ID;

const user: UserType = { name: 'Alice', age: 30 };
const id: IDType = 'user-123';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Name: {user.name}</Text>
      <Text>ID: {id}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [x] Example exists at `examples/ink-import-types/`
- [x] Uses `import("./path").TypeName` syntax
- [x] Renders identically in deno and `runts dev` (100% output match)
- [x] Compile path erases `import` types without runtime impact
- [x] Parity harness passes with 100% match in all 3 environments

## Implementation Notes

Created multi-file example demonstrating:
- `import("../types.ts").TypeName` syntax for importing types
- Multiple type imports (User, Product, ID, Status, Config)
- Runtime usage of typed variables
- Union types, interfaces, and string literal types via import

Added `test_ink_import_types` test to `src/transpile/tests/rq_parity/mod.rs` with expected output assertions.
