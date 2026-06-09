# Task 377: `ink-jsx-generic-type-args` Example — JSX Generic Type Arguments

**Priority:** P1-High
**Phase:** 31 — Advanced JSX + React Edge Cases
**Depends on:** 376

## Problem

JSX supports generic type arguments on components: `<Component<T> prop={value} />`. This exercises the HIR JSX parser's handling of type arguments attached to JSX element names, which must be preserved or erased correctly depending on the target.

## HIR Coverage

This example validates that `Expr::JsxElement` (or `JsxMember`/`JsxNamespacedName`) correctly captures and erases generic type arguments without producing `Expr::Invalid`.

## Compile-Path Codegen

- `quote_codegen.rs` JSX element codegen must handle `type_arguments` on the opening element name.
- Type arguments are erased at codegen time (no runtime impact).
- Generated Rust must compile.

## Ink Example

```tsx
// examples/ink-jsx-generic-type-args/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

interface ItemProps<T> {
  value: T;
  label: string;
}

function Item<T extends string | number>(props: ItemProps<T>) {
  return (
    <Text>{props.label}: {String(props.value)}</Text>
  );
}

export default function App() {
  return (
    <Box flexDirection="column">
      <Item<string> value="hello" label="String item" />
      <Item<number> value={42} label="Number item" />
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-jsx-generic-type-args/`
- [ ] Uses JSX generic type arguments (`<Component<T> />`)
- [ ] HIR parser captures type arguments without producing `Expr::Invalid`
- [ ] Codegen erases type arguments and emits compilable Rust
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
