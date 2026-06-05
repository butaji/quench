// Ink-style Todo component - pure TypeScript version.
// This replaces the old Rust-only implementation.

import React, { useState } from 'react';
import { Box, Text, Spacer, Newline, useInput } from 'ink';

interface Todo {
  done: boolean;
  text: string;
}

export default function TodoApp() {
  const [todos, setTodos] = useState<Todo[]>([
    { done: false, text: 'Write more tests' },
    { done: false, text: 'Fix HIR overflow' },
    { done: true, text: 'Add ink examples' },
  ]);

  useInput((input, key) => {
    if (input === 'q' || key.escape) {
      process.exit(0);
    }
  });

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
