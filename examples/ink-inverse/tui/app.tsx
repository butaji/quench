// Inverse example — exercises the inverse prop on Text.
// Demonstrates inverted foreground/background colors.
//
// The inverse property swaps foreground and background colors.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function Inverse() {
  return (
    <Box flexDirection="column" padding={1} gap={1}>
      <Text bold color="cyan">Inverse Text</Text>
      
      <Box borderStyle="round" padding={1}>
        <Text inverse>Inverse text (swap fg/bg)</Text>
      </Box>
      
      <Box borderStyle="round" padding={1}>
        <Text inverse color="red" backgroundColor="white">
          Inverse red on white
        </Text>
      </Box>
      
      <Box borderStyle="round" padding={1}>
        <Text inverse color="yellow" backgroundColor="black">
          Inverse yellow on black
        </Text>
      </Box>
      
      <Box borderStyle="round" padding={1}>
        <Text inverse color="cyan">
          Inverse cyan
        </Text>
      </Box>
      
      <Text dimColor marginTop={1}>
        Inverse swaps the foreground and background colors.
      </Text>
    </Box>
  );
}
