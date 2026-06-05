// Todo component - simplified for cross-environment parity.
// NOTE: Complex array mapping may not work in all environments.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function TodoApp() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Todo List</Text>
      <Text></Text>
      <Text>[ ] Task 1</Text>
      <Text>[ ] Task 2</Text>
      <Text>[x] Task 3</Text>
      <Text></Text>
      <Text italic>2 of 3 remaining</Text>
    </Box>
  );
}
