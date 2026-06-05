// Flex wrap example — exercises `flexWrap="wrap"`
import React from 'react';
// and `columnGap` / `rowGap` on Box.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import { Box, Text } from 'ink';

export default function FlexWrap() {
  return (
    <Box flexDirection="row" flexWrap="wrap" width={30} columnGap={2} rowGap={1} borderStyle="single" paddingX={1} paddingY={1}>
      <Text>Alpha</Text>
      <Text>Beta</Text>
      <Text>Gamma</Text>
      <Text>Delta</Text>
      <Text>Epsilon</Text>
      <Text>Zeta</Text>
      <Text>Eta</Text>
    </Box>
  );
}
