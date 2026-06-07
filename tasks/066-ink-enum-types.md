# Task 066: `ink-enum-types` Example — Enums, `as`, `satisfies`, Interfaces, Type Aliases

**Priority:** P2-Medium  
**Phase:** 6 — TypeScript Types  
**Depends on:** 065

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

**Requires Tasks 070–071 (enums and type assertions in HIR).**

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] Enums parse into HIR and codegen produces compilable Rust
- [ ] `as` and `satisfies` parse into HIR and are erased in codegen
- [ ] Interfaces and type aliases are erased (no runtime code)
- [ ] `runts build --release` produces working binary with 100% output match
