// Static example — exercises the `<Static>`
import React from 'react';
// component which renders a list of items
// once and never re-renders them. Items below
// the Static can be updated without disturbing
// the already-rendered static content.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import { Box, Text, Static } from 'ink';

export default function StaticExample() {
  const log = [
    'Server started on port 3000',
    'Connected to database',
    'Ready to accept connections',
  ];
  return (
    <Box flexDirection="column" paddingX={2} paddingY={1} borderStyle="round">
      <Static items={log}>
        {(line, index) => (
          <Box key={index}>
            <Text color="green">[{index}] {line}</Text>
          </Box>
        )}
      </Static>
      <Text color="cyan">Live status: OK</Text>
    </Box>
  );
}
