// Input example — demonstrates user input handling with useState.
// Exercises the `bold`, `underline`, and `dimColor` props on Text.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React, { useState } from 'react';
import { Box, Text, useInput } from 'ink';

export default function Input() {
  const [value, setValue] = useState('');

  useInput((input, key) => {
    if (key.return) {
      // submit
    } else if (key.escape || input === 'q') {
      process.exit(0);
    } else if (key.backspace || key.delete) {
      setValue(v => v.slice(0, -1));
    } else if (!key.ctrl && !key.meta) {
      setValue(v => v + input);
    }
  });

  return (
    <Box flexDirection="column" borderStyle="round" paddingX={2} paddingY={1}>
      <Text bold underline>User input</Text>
      <Text>{value || 'Type something here'}</Text>
      <Text dimColor>Press Enter to submit, q to quit.</Text>
    </Box>
  );
}
