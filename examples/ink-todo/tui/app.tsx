// Ink-style Todo component - pure TypeScript version.
// NOTE: useInput hook is not yet supported in runts HIR runtime.
// Shows static values for parity testing.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text, Spacer, Newline } from 'ink';

interface Todo {
  done: boolean;
  text: string;
}

const TODOS: Todo[] = [
  { done: false, text: 'Write more tests' },
  { done: false, text: 'Fix HIR overflow' },
  { done: true, text: 'Add ink examples' },
];

export default function TodoApp() {
  // NOTE: For runts HIR runtime, useInput is not supported.
  // For parity testing, we show static todo list.
  const todos = TODOS;
  const remaining = todos.filter((t) => !t.done).length;

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Ink Todo</Text>
      <Newline />
      {todos.map((t, i) => (
        <Text key={i}>{t.done ? "[x] " : "[ ] "}{t.text}</Text>
      ))}
      <Newline />
      <Text italic>{remaining} of {todos.length} remaining</Text>
      <Spacer />
      <Text>Press q to quit.</Text>
    </Box>
  );
}
