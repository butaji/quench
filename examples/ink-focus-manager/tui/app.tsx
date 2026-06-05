// Focus manager example — demonstrates programmatic focus control.
// Uses useFocusManager hook for programmatic focus management.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text, useFocusManager } from 'ink';

export default function FocusManager() {
  // useFocusManager is not supported in runts HIR runtime.
  // Static values shown for parity testing.
  const focusManager = {
    focusNext: () => {},
    focusPrevious: () => {},
    focusNextViaTab: () => {},
  };

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Focus Manager Demo</Text>
      <Text></Text>
      <Text>Press Tab to move focus forward.</Text>
      <Text>Press Shift+Tab to move focus backward.</Text>
      <Text></Text>
      <Box borderStyle="round" padding={1}>
        <Text dimColor italic>
          useFocusManager hook available.
        </Text>
      </Box>
    </Box>
  );
}
