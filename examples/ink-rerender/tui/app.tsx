// Re-render example — demonstrates component updates.
// NOTE: useState is not fully supported in runts HIR runtime.
// Static values shown for parity testing.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime) - shows static
//   3. runts build (codegen->runts-ink) - full interactivity

import React from 'react';
import { Box, Text } from 'ink';

export default function RerenderDemo() {
  // Static values for parity testing
  const count = 0;
  const name = "Static User";
  const isVisible = true;

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Component Update Demo</Text>
      <Text></Text>
      
      <Text>Count: <Text bold color="green">{count}</Text></Text>
      <Text>Name: <Text italic>{name}</Text></Text>
      <Text>Visible: {isVisible ? "Yes" : "No"}</Text>
      
      <Text></Text>
      <Text bold>Items:</Text>
      <Box flexDirection="column" gap={1}>
        <Text dimColor>- Item A</Text>
        <Text dimColor>- Item B</Text>
        <Text dimColor>- Item C</Text>
      </Box>
      
      <Text></Text>
      <Text dimColor italic>
        Hooks show static values in runts dev mode.
      </Text>
    </Box>
  );
}
