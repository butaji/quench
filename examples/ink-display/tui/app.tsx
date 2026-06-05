// Display example — exercises display prop behavior.
// Simplified for parity: uses conditional rendering
// to demonstrate visibility control.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function Display() {
  const showHidden = false; // Simulates display="none" behavior

  return (
    <Box flexDirection="column" paddingX={2} paddingY={1} borderStyle="single">
      <Text>Visible item 1</Text>
      {showHidden && <Text>Hidden item</Text>}
      <Text>Visible item 2</Text>
      <Text>Visible item 3</Text>
    </Box>
  );
}
