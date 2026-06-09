# Task 336: `ink-generic-function-component` Example — Generic Function Components

**Priority:** P1-High
**Phase:** 27 — React Type Patterns
**Depends on:** 335

## Problem

Generic function components (`function List<T>({ items }: { items: T[] })`) combine React components with TypeScript generics. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-generic-function-component/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

interface ListProps<T> {
  items: T[];
  renderItem: (item: T) => string;
}

function List<T>({ items, renderItem }: ListProps<T>) {
  return (
    <Box flexDirection="column">
      {items.map((item, i) => (
        <Text key={i}>{renderItem(item)}</Text>
      ))}
    </Box>
  );
}

export default function App() {
  return (
    <List
      items={[{ name: 'Alice' }, { name: 'Bob' }]}
      renderItem={item => item.name}
    />
  );
}
```


## HIR Coverage

- Standard `Expr`/`Stmt` variants

## Compile-Path Codegen

- Standard `quote_codegen` expression + statement codegen

## Acceptance Criteria

- [ ] Example exists at `examples/ink-generic-function-component/`
- [ ] Uses generic function component
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases generics without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
