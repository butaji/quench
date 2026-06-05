// Text props example — exercises text styling props.
// Simplified for parity: uses basic text styling
// that renders consistently across all environments.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function TextProps() {
  return (
    <Box flexDirection="column" paddingX={2} paddingY={1} borderStyle="single">
      <Text bold>Text Styling Props</Text>
      <Text strikethrough color="red">Deprecated feature</Text>
      <Text bold color="yellow">HIGHLIGHTED</Text>
      <Text dimColor>Dimmed status text</Text>
    </Box>
  );
}
