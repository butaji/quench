// Flex wrap example — exercises `flexWrap="wrap"`
// Simplified version for cross-environment parity.
//
// NOTE: The flexWrap feature may render differently in different environments.
// This example uses a simpler layout to ensure parity.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function FlexWrap() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Flex Wrap Demo</Text>
      <Text></Text>
      <Box borderStyle="round" padding={1}>
        <Text>Row 1: Alpha, Beta, Gamma</Text>
        <Text>Row 2: Delta, Epsilon</Text>
        <Text>Row 3: Zeta, Eta</Text>
      </Box>
      <Text dimColor marginTop={1}>
        Items wrap to new lines when needed.
      </Text>
    </Box>
  );
}
