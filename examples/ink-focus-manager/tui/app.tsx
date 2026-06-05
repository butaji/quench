// Focus manager example — exercises the useFocusManager hook.
// Demonstrates programmatic focus control.
//
// The focus manager allows moving focus between elements
// using Tab and Shift+Tab, as well as programmatic control.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text, useFocusManager, useFocus } from 'ink';

function FocusableBox({ id, label }: { id: string; label: string }) {
  const { isFocused } = useFocus({ id });

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
  const { focusNext, focusPrevious } = useFocusManager();

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Focus Manager</Text>
      <Box marginTop={1} flexDirection="column" gap={1}>
        <FocusableBox id="first" label="First element" />
        <FocusableBox id="second" label="Second element" />
        <FocusableBox id="third" label="Third element" />
      </Box>
      <Box marginTop={1}>
        <Text dimColor>Use Tab to move focus forward.</Text>
      </Box>
    </Box>
  );
}
