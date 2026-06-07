# Task 102: `ink-index-intersection` Example — Index Signatures, Intersection Types

**Priority:** P1-High
**Phase:** 11 — Type System Deep Coverage
**Depends on:** 078

## Problem

Index signatures (`[key: string]: T`) and intersection types (`A & B`) are core TypeScript patterns for flexible object shapes and type composition. No existing Ink example exercises both together.

## Ink Example

```tsx
// examples/ink-index-intersection/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

interface BaseProps {
  name: string;
}

interface StyleProps {
  [key: string]: string | number;
  color: string;
  width: number;
}

type FullProps = BaseProps & StyleProps;

const props: FullProps = {
  name: 'Widget',
  color: 'blue',
  width: 80,
};

interface Dictionary {
  [word: string]: string;
}

const dict: Dictionary = {
  hello: 'world',
  foo: 'bar',
};

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Name: {props.name}</Text>
      <Text>Color: {props.color}</Text>
      <Text>Width: {props.width}</Text>
      <Text>Dict: {Object.entries(dict).map(([k, v]) => `${k}=${v}`).join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-index-intersection/`
- [ ] Uses index signature `[key: string]: T`
- [ ] Uses intersection type `A & B`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases index signatures and intersections without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
