// Nested layouts example — demonstrates complex flexbox nesting.
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)
//
// NOTE: gap prop adds visual space but may render differently between environments.

import React from 'react';
import { Box, Text } from 'ink';

export default function NestedLayouts() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Nested Layout Demo</Text>
      <Text></Text>
      <Box flexDirection="row">
        <Box borderStyle="single" padding={1}>
          <Text>Left Panel</Text>
        </Box>
        <Text> </Text>
        <Box borderStyle="double" padding={1}>
          <Text>Right Panel</Text>
        </Box>
      </Box>
      <Text></Text>
      <Box borderStyle="round" padding={1}>
        <Text dimColor>Footer content</Text>
      </Box>
      <Text></Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
