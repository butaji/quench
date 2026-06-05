// Partial border example — exercises `borderTop`,
// `borderBottom`, `borderLeft`, `borderRight` props on Box.
// Simplified version for cross-environment parity.
//
// NOTE: Partial borders may render differently due to terminal width.
// This example uses a simpler layout to ensure parity.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function PartialBorder() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Partial Border Demo</Text>
      <Text></Text>
      <Box borderTop={true} paddingY={1}>
        <Text>Top border only</Text>
      </Box>
      <Box borderBottom={true} paddingY={1}>
        <Text>Bottom border only</Text>
      </Box>
      <Box borderLeft={true} paddingY={1}>
        <Text>Left border only</Text>
      </Box>
      <Box borderRight={true} paddingY={1}>
        <Text>Right border only</Text>
      </Box>
    </Box>
  );
}
