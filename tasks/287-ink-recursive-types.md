# Task 287: `ink-recursive-types` Example — Recursive Type Aliases and Interfaces

**Priority:** P2-Medium
**Phase:** 24 — Type System Deep Coverage
**Depends on:** 286

## Problem

Recursive type aliases and interfaces (`type Tree = { value: string; children: Tree[] }`) are common in tree-shaped data structures. No existing Ink example exercises recursive types.

## Ink Example

```tsx
// examples/ink-recursive-types/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

interface TreeNode {
  value: string;
  children: TreeNode[];
}

const tree: TreeNode = {
  value: 'root',
  children: [
    { value: 'a', children: [{ value: 'a1', children: [] }] },
    { value: 'b', children: [] },
  ],
};

function count(node: TreeNode): number {
  return 1 + node.children.reduce((sum, child) => sum + count(child), 0);
}

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Root: {tree.value}</Text>
      <Text>Total nodes: {count(tree)}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Type erasure (no runtime HIR needed)

## Compile-Path Codegen

- Type erasure at parse time (no runtime codegen)

## Acceptance Criteria

- [ ] Example exists at `examples/ink-recursive-types/`
- [ ] Uses recursive `interface` or `type` alias
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases recursive types without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
