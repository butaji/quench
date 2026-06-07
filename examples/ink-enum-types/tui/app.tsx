// ink-enum-types example — demonstrates type aliases, interfaces, and type assertions.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: Actual enums (enum keyword) require oxc transformer config.
// This example uses const objects which behave like string enums.

import React from 'react';
import { Box, Text } from 'ink';

// Const object simulating string enum
const Status = {
  Idle: 'idle',
  Loading: 'loading',
  Done: 'done',
  Error: 'error',
} as const;
type Status = typeof Status[keyof typeof Status];

// Const object simulating numeric enum
const Priority = {
  Low: 1,
  Medium: 2,
  High: 3,
} as const;
type Priority = typeof Priority[keyof typeof Priority];

// Interface
interface Task {
  name: string;
  status: Status;
  priority: Priority;
}

// Type alias
type TaskList = Task[];

// Union type
type Result = Success | Error;

// Interface for union members
interface Success {
  kind: 'success';
  value: number;
}

interface ErrorResult {
  kind: 'error';
  message: string;
}

type Error = ErrorResult;

// Type assertion
function processValue(val: unknown): string {
  const str = val as string;
  return str.toUpperCase();
}

// Record type with as
const statusColors: Record<string, string> = {
  [Status.Idle]: 'gray',
  [Status.Loading]: 'yellow',
  [Status.Done]: 'green',
  [Status.Error]: 'red',
};

export default function EnumTypesDemo() {
  const results: string[] = [];

  // String enum simulation
  results.push(`Status.Idle: ${Status.Idle}`);
  results.push(`Status.Loading: ${Status.Loading}`);
  results.push(`Status.Done: ${Status.Done}`);
  results.push(`Status.Error: ${Status.Error}`);

  // Numeric enum simulation
  results.push(`Priority.Low: ${Priority.Low}`);
  results.push(`Priority.Medium: ${Priority.Medium}`);
  results.push(`Priority.High: ${Priority.High}`);

  // Interface object
  const task: Task = {
    name: 'Implement feature',
    status: Status.Loading,
    priority: Priority.High,
  };
  results.push(`Task: ${task.name}, ${task.status}, priority ${task.priority}`);

  // Type assertion
  results.push(`processValue('hello'): ${processValue('hello')}`);

  // Record type
  results.push(`statusColors[Status.Done]: ${statusColors[Status.Done]}`);

  // Union type
  const success: Result = { kind: 'success', value: 42 };
  const errorResult: Result = { kind: 'error', message: 'Failed' };

  if (success.kind === 'success') {
    results.push(`Success: ${success.value}`);
  }
  if (errorResult.kind === 'error') {
    results.push(`Error: ${errorResult.message}`);
  }

  // Array of typed objects
  const tasks: TaskList = [
    { name: 'Task 1', status: Status.Done, priority: Priority.High },
    { name: 'Task 2', status: Status.Loading, priority: Priority.Medium },
    { name: 'Task 3', status: Status.Idle, priority: Priority.Low },
  ];

  for (const t of tasks) {
    results.push(`${t.name}: ${t.status} (priority ${t.priority})`);
  }

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Enum & Types Demo</Text>
      <Text dimColor>Using const objects (actual enums need HIR update)</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
