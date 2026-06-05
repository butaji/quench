// Static component example — demonstrates Static for non-reactive rendering.
// Static renders children once without re-rendering on state changes.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text, Static } from 'ink';

export default function StaticExample() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Static Component Demo</Text>
      <Text></Text>
      <Text>Static prevents re-rendering of fixed content:</Text>
      <Text></Text>
      <Static items={['Item A', 'Item B', 'Item C']}>
        {(item) => (
          <Box key={item}>
            <Text>- {item}</Text>
          </Box>
        )}
      </Static>
      <Text></Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
