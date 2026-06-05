// Import example — demonstrates Ink module imports.
// Shows how to import and use Ink components.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text, Newline, Spacer } from 'ink';

export default function ImportExample() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Import Example</Text>
      <Newline />
      <Text>Box from 'ink' - flexbox container</Text>
      <Text>Text from 'ink' - styled text</Text>
      <Text>Newline from 'ink' - vertical spacer</Text>
      <Text>Spacer from 'ink' - flexible space</Text>
      <Newline />
      <Box borderStyle="round" padding={1}>
        <Text dimColor italic>
          This example demonstrates standard Ink imports.
        </Text>
      </Box>
      <Spacer />
      <Text dimColor>End of imports demo</Text>
    </Box>
  );
}
