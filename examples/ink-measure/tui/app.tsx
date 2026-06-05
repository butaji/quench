// Measure example — demonstrates box dimension display.
// Simplified for parity: shows dimension values
// outside of the measured box for consistent rendering.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function Measure() {
  // Static dimension display for parity testing
  const width = 15;
  const height = 3;

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Dimensions</Text>
      <Text dimColor>Width: {width}</Text>
      <Text dimColor>Height: {height}</Text>
      <Box width={width} height={height} marginTop={1} borderStyle="round" justifyContent="center" alignItems="center">
        <Text>Box</Text>
      </Box>
    </Box>
  );
}
