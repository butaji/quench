// ink-this-parameter example — this parameter, this types
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React, { useState } from 'react';
import { Box, Text } from 'ink';

const button = {
  label: 'Submit',
  click() {
    return `Clicked: ${this.label}`;
  },
};

export default function App() {
  const [count] = useState(0);
  return (
    <Box flexDirection="column">
      <Text>{button.click()}</Text>
      <Text>Count: {count}</Text>
    </Box>
  );
}
