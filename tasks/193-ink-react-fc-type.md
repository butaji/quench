# Task 193: `ink-react-fc-type` Example — `React.FC`, `React.FunctionComponent`

**Priority:** P1-High
**Phase:** 17 — React Type Patterns
**Depends on:** 192

## Problem

`React.FC` and `React.FunctionComponent` are common React type patterns that include implicit `children` prop typing. No existing Ink example exercises these type annotations.

## Ink Example

```tsx
// examples/ink-react-fc-type/tui/app.tsx
import React, { FC, FunctionComponent } from 'react';
import { Box, Text } from 'ink';

interface Props {
  title: string;
}

const Header: FC<Props> = ({ title }) => (
  <Text bold color="cyan">{title}</Text>
);

const SubHeader: FunctionComponent<Props> = ({ title }) => (
  <Text dimColor>{title}</Text>
);

export default function App() {
  return (
    <Box flexDirection="column">
      <Header title="Main Title" />
      <SubHeader title="Subtitle" />
    </Box>
  );
}
```


## HIR Coverage

- React hook calls via `Expr::Call`

## Compile-Path Codegen

- `js_bundle/react_shim.rs` for hook definitions

## Acceptance Criteria

- [x] Example exists at `examples/ink-react-fc-type/`
- [x] Uses `React.FC<Props>` type annotation
- [x] Uses `React.FunctionComponent<Props>` type annotation
- [x] Renders identically in deno and `runts dev` (100% output match)
- [x] Compile path erases `FC` type without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments

## Notes

- Dev path (deno/rq) produces 100% parity.
- Compile path does not produce a working binary due to a known architectural limitation: the ratatui codegen cannot inline or resolve custom component calls (`<Header />`, `<SubHeader />`). This is the same limitation affecting `ink-hoc` and other examples with custom components. The FC type annotations themselves are correctly erased without runtime impact.
- Added `test_ink_react_fc_type` to `src/transpile/tests/rq_parity/mod.rs`.
