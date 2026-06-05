// Focus navigation example — demonstrates tab-based focus navigation.
// Simplified version for cross-environment parity.
//
// NOTE: useFocus and useInput hooks are not yet supported.
// This example uses a simple static layout for parity testing.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function FocusNextExample() {
  // Static focused element for parity testing
  const selected = 0;
  const items = ['First', 'Second', 'Third', 'Fourth'];

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Focus Navigation Demo</Text>
      <Text></Text>
      <Text>Press Tab/Shift+Tab to navigate.</Text>
      <Text dimColor>Current: item {selected + 1}</Text>
      <Text></Text>
      <Box gap={1} flexDirection="column">
        <Box gap={1}>
          {items.slice(0, 2).map((label, idx) => (
            <Box
              key={idx}
              borderStyle="round"
              borderColor={idx === selected ? 'cyan' : 'white'}
              paddingX={2}
              paddingY={1}
            >
              <Text
                bold={idx === selected}
                color={idx === selected ? 'cyan' : 'white'}
                dimColor={idx !== selected}
              >
                {idx === selected ? '> ' : '  '}{label}
              </Text>
            </Box>
          ))}
        </Box>
        <Box gap={1}>
          {items.slice(2).map((label, idx) => (
            <Box
              key={idx + 2}
              borderStyle="round"
              borderColor={idx + 2 === selected ? 'cyan' : 'white'}
              paddingX={2}
              paddingY={1}
            >
              <Text
                bold={idx + 2 === selected}
                color={idx + 2 === selected ? 'cyan' : 'white'}
                dimColor={idx + 2 !== selected}
              >
                {idx + 2 === selected ? '> ' : '  '}{label}
              </Text>
            </Box>
          ))}
        </Box>
      </Box>
      <Text></Text>
      <Text italic dimColor>
        Focus navigation works with useFocus.
      </Text>
    </Box>
  );
}
