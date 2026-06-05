// Checkbox Example — demonstrates checkbox UI component.
// Shows checkbox patterns for multi-select state.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

function Checkbox({ checked, label }: { checked: boolean; label: string }) {
  return (
    <Box>
      <Text color={checked ? "green" : "gray"}>
        {checked ? "[×]" : "[ ]"}
      </Text>
      <Text> {label}</Text>
    </Box>
  );
}

export default function FormCheckbox() {
  // Static values for parity testing
  const options = [
    { label: "Option A", checked: true },
    { label: "Option B", checked: false },
    { label: "Option C", checked: true },
  ];
  const checkedCount = options.filter(o => o.checked).length;

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Select Options</Text>
      <Text></Text>
      {options.map((opt, i) => (
        <Checkbox key={i} checked={opt.checked} label={opt.label} />
      ))}
      <Text></Text>
      <Text dimColor>{checkedCount} of {options.length} selected</Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
