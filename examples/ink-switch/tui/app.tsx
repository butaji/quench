// Switch example — demonstrates boolean toggle state.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React, { useState } from 'react';
import { Box, Text } from 'ink';

export default function SwitchExample() {
  const [enabled, setEnabled] = useState(false);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Switch Demo</Text>
      <Text></Text>
      <Box flexDirection="row" gap={1}>
        <Text>Feature:</Text>
        <Text color={enabled ? 'green' : 'red'} bold>
          {enabled ? 'ON' : 'OFF'}
        </Text>
      </Box>
      <Text dimColor>Use space to toggle (interactive in deno/compile).</Text>
    </Box>
  );
}
