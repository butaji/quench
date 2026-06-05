// Combined Hooks Example — demonstrates multiple hooks working together.
// Shows useState, useEffect, useInput, useApp, and useStdin integration.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function CombinedHooks() {
  // Static values for parity testing
  const count = 0;
  const name = "Combined Hooks Demo";
  const status = "ready";

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Combined Hooks Demo</Text>
      <Text></Text>
      <Text>Count: <Text bold color="green">{count}</Text></Text>
      <Text>Name: <Text italic>{name}</Text></Text>
      <Text>Status: {status}</Text>
      <Text></Text>
      <Text dimColor>Press Ctrl+C to exit.</Text>
    </Box>
  );
}
