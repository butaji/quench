// Uncontrolled Input example — demonstrates text input.
// Simplified for cross-environment parity.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function UncontrolledInputDemo() {
  // Static display for parity testing
  const value = "Hello";

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Uncontrolled Input Demo</Text>
      <Text></Text>
      <Box>
        <Text>Name: </Text>
        <Text backgroundColor="blue" color="white">
          {value || "Type here..."}
        </Text>
      </Box>
      <Text></Text>
      <Text dimColor>Type to input text.</Text>
      <Text dimColor>Press enter to submit.</Text>
    </Box>
  );
}
