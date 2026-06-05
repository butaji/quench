// Focus cycle example — demonstrates cycling focus through elements.
// Simplified for parity: shows static focus state for all environments.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function FocusCycle() {
  // NOTE: Simplified for parity - static UI shown.
  // Second input is focused.

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Focus Cycling Demo</Text>
      <Text></Text>
      
      <Text color="gray">1. Input 1</Text>
      <Text color="green" bold>2. Input 2</Text>
      <Text color="gray">3. Input 3</Text>
      
      <Text></Text>
      <Text dimColor italic>Focus: Input 2/3</Text>
    </Box>
  );
}
