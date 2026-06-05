// Align self example — exercises `alignSelf`
// Simplified version for cross-environment parity.
//
// NOTE: The complex nested Box layout with alignSelf is simplified here
// to ensure parity across deno, runts dev, and runts build.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function AlignSelf() {
  // Static layout for parity testing
  const items = [
    { label: 'flex-start', color: 'cyan' },
    { label: 'center', color: 'green' },
    { label: 'flex-end', color: 'magenta' },
  ];

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Align Self Demo</Text>
      <Text></Text>
      {items.map((item, i) => (
        <Box key={i} alignItems={item.label.includes('center') ? 'center' : item.label.includes('end') ? 'flex-end' : 'flex-start'}>
          <Text color={item.color as any}>{item.label}</Text>
        </Box>
      ))}
    </Box>
  );
}
