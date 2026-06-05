// Partial border example — exercises `borderTop`,
import React from 'react';
// `borderBottom`, `borderLeft`, `borderRight` props
// on Box to draw partial borders.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import { Box, Text } from 'ink';

export default function PartialBorder() {
  return (
    <Box flexDirection="column" paddingX={2} paddingY={1}>
      <Box borderStyle="single" borderTop={true} borderBottom={true} paddingX={1}>
        <Text>Top + Bottom border</Text>
      </Box>
      <Box borderStyle="single" borderLeft={true} borderRight={true} paddingX={1}>
        <Text>Left + Right border</Text>
      </Box>
      <Box borderStyle="single" borderTop={true} paddingX={1}>
        <Text>Top border only</Text>
      </Box>
      <Box borderStyle="single" borderBottom={true} paddingX={1}>
        <Text>Bottom border only</Text>
      </Box>
    </Box>
  );
}
