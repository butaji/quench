// Focus navigation example — demonstrates tab-based focus navigation.
// NOTE: useFocus and useInput hooks are not yet supported in runts HIR runtime.
// Shows static focus state for parity testing.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

function FocusableBox({ children, isFocused }: { children: React.ReactNode; isFocused: boolean }) {
  return (
    <Box
      borderStyle="round"
      borderColor={isFocused ? 'cyan' : 'white'}
      paddingX={2}
      paddingY={1}
      minWidth={15}
    >
      <Text
        bold={isFocused}
        color={isFocused ? 'cyan' : 'white'}
        dimColor={!isFocused}
      >
        {children}
      </Text>
    </Box>
  );
}

export default function FocusNextExample() {
  // NOTE: For runts HIR runtime, useFocus/useInput are not supported.
  // For parity testing, we show static focus state (first element focused).
  const selected = 0;
  const ids = ['first', 'second', 'third', 'fourth'];

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Focus Navigation Demo</Text>
      <Text></Text>
      <Text>Press Tab/Shift+Tab to navigate.</Text>
      <Text dimColor>Current: {ids[selected]}</Text>
      <Text></Text>
      
      <Box gap={1} flexDirection="column">
        <Box gap={1}>
          <FocusableBox isFocused={selected === 0}>First</FocusableBox>
          <FocusableBox isFocused={selected === 1}>Second</FocusableBox>
        </Box>
        <Box gap={1}>
          <FocusableBox isFocused={selected === 2}>Third</FocusableBox>
          <FocusableBox isFocused={selected === 3}>Fourth</FocusableBox>
        </Box>
      </Box>
      
      <Text></Text>
      <Text italic dimColor>
        Focus navigation works with useFocus and useFocusManager.
      </Text>
    </Box>
  );
}
