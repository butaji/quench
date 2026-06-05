// Align self example — exercises `alignSelf`
import React from 'react';
// on individual Box children to override the
// parent's `alignItems` for that child.
// Values: "flex-start", "center", "flex-end",
// "auto", "stretch".
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import { Box, Text } from 'ink';

export default function AlignSelf() {
  return (
    <Box flexDirection="row" width={40} height={6} borderStyle="single" paddingX={1} paddingY={1}>
      <Box borderStyle="round" alignSelf="flex-start">
        <Text>start</Text>
      </Box>
      <Box borderStyle="round" alignSelf="center">
        <Text>center</Text>
      </Box>
      <Box borderStyle="round" alignSelf="flex-end">
        <Text>end</Text>
      </Box>
    </Box>
  );
}
