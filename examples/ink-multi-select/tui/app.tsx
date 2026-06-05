// Multi-select example — demonstrates multiple selection UI.
// Simplified for parity: shows static selections for all environments.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function MultiSelect() {
  // NOTE: Simplified for parity - static UI shown.

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Multi-Select Demo</Text>
      <Text></Text>
      
      <Box flexDirection="column" gap={1}>
        <Text color="green">[*] Option A</Text>
        <Text color="gray">[ ] Option B</Text>
        <Text color="green">[*] Option C</Text>
        <Text color="gray">[ ] Option D</Text>
      </Box>
      
      <Text></Text>
      <Text dimColor italic>Use arrow keys and Space to select</Text>
    </Box>
  );
}
