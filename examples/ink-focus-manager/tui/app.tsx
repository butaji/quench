// Focus manager example — demonstrates programmatic focus control.
// Simplified version for cross-environment parity.
//
// NOTE: useFocus and useFocusManager hooks are not yet supported.
// This example uses a simple static layout for parity testing.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function FocusManager() {
  // Static focused element for parity testing
  const focusedIndex = 0;
  const elements = ['First element', 'Second element', 'Third element'];

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Focus Manager Demo</Text>
      <Box marginTop={1} flexDirection="column" gap={1}>
        {elements.map((label, index) => (
          <Box
            key={index}
            padding={1}
            borderStyle="round"
            borderColor={index === focusedIndex ? 'cyan' : 'white'}
          >
            <Text color={index === focusedIndex ? 'cyan' : 'white'}>
              {index === focusedIndex ? '> ' : '  '}{label}
            </Text>
          </Box>
        ))}
      </Box>
      <Box marginTop={1}>
        <Text dimColor>Tab to move focus forward.</Text>
      </Box>
    </Box>
  );
}
