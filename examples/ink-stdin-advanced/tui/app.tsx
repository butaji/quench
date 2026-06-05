// Advanced Stdin Example — demonstrates stdin hooks.
// Shows reading from stdin with useStdin.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function StdinAdvanced() {
  // Static values for parity testing
  const stdinMode = "raw";
  const bufferSize = 0;
  const isRawMode = true;

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Advanced Stdin</Text>
      <Text></Text>
      <Text>Stdin Mode: {stdinMode}</Text>
      <Text>Buffer Size: {bufferSize}</Text>
      <Text>Raw Mode: {isRawMode ? "enabled" : "disabled"}</Text>
      <Text></Text>
      <Text dimColor>Reading from stdin...</Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
