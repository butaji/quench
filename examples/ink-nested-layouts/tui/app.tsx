// Nested layouts example — demonstrates complex flexbox nesting.
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function NestedLayouts() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Nested Layout Demo</Text>
      <Box flexDirection="row" gap={1} marginTop={1}>
        <Box borderStyle="single" padding={1} flexGrow={1}>
          <Text>Left Panel</Text>
        </Box>
        <Box borderStyle="double" padding={1} flexGrow={1}>
          <Text>Right Panel</Text>
        </Box>
      </Box>
      <Box marginTop={1}>
        <Box borderStyle="round" padding={1}>
          <Text dimColor>Footer content</Text>
        </Box>
      </Box>
    </Box>
  );
}
