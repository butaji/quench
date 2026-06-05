// Overflow example — exercises `overflowX`
import React from 'react';
// and `overflowY` to control how content
// that exceeds the box dimensions is handled.
// Options: "visible" (default), "hidden",
// "scroll".
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import { Box, Text } from 'ink';

export default function Overflow() {
  return (
    <Box flexDirection="column" paddingX={2} paddingY={1} borderStyle="single">
      <Box width={20} height={3} borderStyle="round" overflowX="hidden" overflowY="hidden">
        <Text>This text is too long to fit in the 20-wide box and should be truncated or hidden.</Text>
      </Box>
      <Box width={20} height={2} borderStyle="round" overflowX="hidden" overflowY="visible">
        <Text>Short text</Text>
      </Box>
    </Box>
  );
}
