// Multiple colors example — demonstrates various text colors.
// Simplified for parity: shows color palette with basic text.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function MultipleColors() {
  const colors = [
    { name: 'black', color: 'black' },
    { name: 'red', color: 'red' },
    { name: 'green', color: 'green' },
    { name: 'yellow', color: 'yellow' },
    { name: 'blue', color: 'blue' },
    { name: 'magenta', color: 'magenta' },
    { name: 'cyan', color: 'cyan' },
    { name: 'white', color: 'white' },
  ];

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Color Palette</Text>
      <Box flexDirection="column" marginTop={1}>
        {colors.map((c) => (
          <Box key={c.name} marginTop={1}>
            <Text>
              <Text color={c.color as any}>{c.name}</Text>
            </Text>
          </Box>
        ))}
      </Box>
    </Box>
  );
}
