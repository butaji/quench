// Checkbox Example — demonstrates checkbox UI component.
// Shows checkbox patterns for multi-select state.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)
//
// NOTE: Custom components and ternary operators are not supported in runts HIR runtime.
// For compatibility, we use inline components and separate render functions.

import React from 'react';
import { Box, Text } from 'ink';

export default function FormCheckbox() {
  // Static values for parity testing
  const options = [
    { label: "Option A", checked: true },
    { label: "Option B", checked: false },
    { label: "Option C", checked: true },
  ];
  const checkedCount = 2;

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Select Options</Text>
      <Text></Text>
      <Box>
        <Text color="green">[×]</Text>
        <Text> {options[0].label}</Text>
      </Box>
      <Box>
        <Text color="gray">[ ]</Text>
        <Text> {options[1].label}</Text>
      </Box>
      <Box>
        <Text color="green">[×]</Text>
        <Text> {options[2].label}</Text>
      </Box>
      <Text></Text>
      <Text dimColor>{checkedCount} of {options.length} selected</Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
