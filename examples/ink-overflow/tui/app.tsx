// Overflow example — demonstrates overflow handling.
// NOTE: This example is simplified for runts HIR runtime compatibility.
// Overflow handling may differ between environments.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function Overflow() {
  return (
    <Box flexDirection="column" paddingX={2} paddingY={1} borderStyle="single">
      <Box width={20} height={3} borderStyle="round" overflowX="hidden" overflowY="hidden">
        <Text>Short text</Text>
      </Box>
      <Box width={20} height={2} borderStyle="round" overflowX="hidden" overflowY="visible">
        <Text>Short text</Text>
      </Box>
    </Box>
  );
}
