# Task 072: `ink-enum-types` Example — Enums, Interfaces, Type Aliases, `as`, `satisfies`

**Priority:** P2-Medium
**Phase:** 6 — TypeScript Types
**Depends on:** 071

## Problem

Zero examples use enums, `as` assertions, or `satisfies`. Interfaces and type aliases are common but only type-erased.

## Example

```tsx
import { Box, Text } from 'ink';

enum Status { Idle = 'idle', Loading = 'loading', Done = 'done', Error = 'error' }

interface Task { name: string; status: Status; }
type TaskList = Task[];

export default function App({ tasks }: { tasks: TaskList }) {
  const summary = tasks satisfies TaskList;
  const statusColors = {
    [Status.Idle]: 'gray',
    [Status.Loading]: 'yellow',
    [Status.Done]: 'green',
    [Status.Error]: 'red',
  } as Record<string, string>;

  return (
    <Box flexDirection="column">
      {summary.map((task, i) => (
        <Text key={i} color={statusColors[task.status]}>
          {task.name}: {task.status}
        </Text>
      ))}
    </Box>
  );
}
```

## Work

**Requires Task 076 (enums in HIR) and Task 075 (type assertions in HIR) to be completed first.**

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] Enums parse into HIR and codegen produces compilable Rust
- [ ] `as` and `satisfies` parse into HIR and are erased in codegen
- [ ] Interfaces and type aliases are erased (no runtime code)
- [ ] `runts build --release` produces working binary
- [ ] Parity harness 100%