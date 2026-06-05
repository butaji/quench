// Focus manager example — demonstrates programmatic focus control.
// NOTE: useFocus and useFocusManager hooks are not yet supported in runts HIR runtime.
// Shows static focus state for parity testing.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

function FocusableBox({ label, isFocused }: { label: string; isFocused: boolean }) {
  return (
    <Box
      padding={1}
      borderStyle="round"
      borderColor={isFocused ? 'cyan' : 'white'}
    >
      <Text color={isFocused ? 'cyan' : 'white'}>
        {isFocused ? '> ' : '  '}{label}
      </Text>
    </Box>
  );
}

export default function FocusManager() {
  // NOTE: For runts HIR runtime, useFocus/useFocusManager are not supported.
  // For parity testing, we show static focus state (first element focused).
  const focusedIndex = 0;

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Focus Manager</Text>
      <Box marginTop={1} flexDirection="column" gap={1}>
        <FocusableBox label="First element" isFocused={focusedIndex === 0} />
        <FocusableBox label="Second element" isFocused={focusedIndex === 1} />
        <FocusableBox label="Third element" isFocused={focusedIndex === 2} />
      </Box>
      <Box marginTop={1}>
        <Text dimColor>Tab to move focus forward.</Text>
      </Box>
    </Box>
  );
}
